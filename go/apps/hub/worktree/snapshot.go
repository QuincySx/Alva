// INPUT: Saved managed identity, literal bounded Git tree/index entries and rooted regular files
// OUTPUT: Deterministic SHA/Git-mode snapshots with internal unprivileged Perm, without repository filters
// POS: [L7 Hub product] Optimistic product evidence, not an OS filesystem transaction or grant to arbitrary host paths
package worktree

import (
	"bytes"
	"context"
	"crypto/sha256"
	"encoding/hex"
	"encoding/json"
	"errors"
	"io"
	"os"
	"path/filepath"
	"slices"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/apps/hub"
)

type fileState struct {
	Mode   string `json:"mode"`
	SHA256 string `json:"sha256"`
	Data   []byte `json:"-"`
	// Perm is actual unprivileged host metadata, not Git's executable/not
	// executable vocabulary. It stays internal and is never inherited from
	// the Worker's private copies to broaden shared-file read/write access.
	Perm os.FileMode `json:"-"`
}
type fileSnapshot map[string]fileState

func sha(data []byte) string { digest := sha256.Sum256(data); return hex.EncodeToString(digest[:]) }
func validPath(name string) bool {
	if len(name) == 0 || len(name) > 4096 || !utf8.ValidString(name) || !filepath.IsLocal(name) || filepath.Clean(name) != name || strings.ContainsAny(name, "\x00\r\n\t\\") {
		return false
	}
	if strings.Count(filepath.ToSlash(name), "/") >= 32 {
		return false
	}
	for _, part := range strings.Split(filepath.ToSlash(name), "/") {
		if strings.EqualFold(part, ".git") {
			return false
		}
	}
	return true
}
func (m *Manager) checkedHandle(ctx context.Context, h hub.WorktreeBinding) ([]treeEntry, error) {
	if h.Version != 1 || !validWorker(h.WorkerID) || h.Path != filepath.Join(m.config.Root, "trees", h.WorkerID) || h.Branch != "alva/"+h.WorkerID || !validOID(h.BaseCommit) {
		return nil, errors.New("saved managed worktree identity is invalid")
	}
	for _, path := range []string{m.config.Root, filepath.Join(m.config.Root, "trees"), h.Path} {
		info, err := os.Lstat(path)
		if err != nil || !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("managed worktree path was replaced or is not a directory")
		}
	}
	workspace, base, err := m.repository(ctx, h.Workspace)
	if err != nil || workspace != h.Workspace || base != h.BaseCommit {
		return nil, errors.New("shared repository HEAD changed; regenerate an isolated Worker rather than silently merging")
	}
	worker, _, err := m.repository(ctx, h.Path)
	if err != nil || worker != h.Path {
		return nil, errors.New("managed worktree repository identity changed")
	}
	branch, err := m.run(ctx, h.Path, []string{"symbolic-ref", "--short", "HEAD"}, nil, nil, 256)
	if err != nil || strings.TrimSpace(string(branch)) != h.Branch {
		return nil, errors.New("managed Worker branch changed")
	}
	data, err := m.run(ctx, h.Workspace, []string{"ls-tree", "-rz", "--full-tree", h.BaseCommit}, nil, nil, 1<<20)
	if err != nil {
		return nil, err
	}
	return parseTree(data)
}
func (m *Manager) baseSnapshot(ctx context.Context, workspace string, entries []treeEntry) (fileSnapshot, error) {
	out := fileSnapshot{}
	total := 0
	for _, entry := range entries {
		if err := ctx.Err(); err != nil {
			return nil, err
		}
		data, err := m.run(ctx, workspace, []string{"cat-file", "blob", entry.oid}, nil, nil, 20<<20)
		if err != nil {
			return nil, err
		}
		total += len(data)
		if total > 128<<20 {
			return nil, errors.New("base snapshot exceeds 128MiB")
		}
		out[entry.path] = fileState{Mode: entry.mode, SHA256: sha(data), Data: data}
	}
	return out, nil
}
func readLiteral(root *os.Root, name string) (fileState, bool, error) {
	if !validPath(name) {
		return fileState{}, false, errors.New("snapshot path is not a bounded clean relative path")
	}
	parent, base, err := pinParent(root, name, false)
	if os.IsNotExist(err) {
		return fileState{}, false, nil
	}
	if err != nil {
		return fileState{}, false, err
	}
	defer parent.Close()
	return readPinned(parent, base)
}

// A root only prevents escaping; it does not reject an ancestor replaced with
// a symlink to ANOTHER directory inside the root. Pin every actual directory
// before/after opening, then use basename operations on that descriptor only.
func pinParent(root *os.Root, name string, create bool) (*os.Root, string, error) {
	if !validPath(name) {
		return nil, "", errors.New("bounded relative target required")
	}
	parent, err := root.OpenRoot(".")
	if err != nil {
		return nil, "", err
	}
	parts := strings.Split(filepath.ToSlash(name), "/")
	for _, part := range parts[:len(parts)-1] {
		before, err := parent.Lstat(part)
		if os.IsNotExist(err) && create {
			if err = parent.Mkdir(part, 0700); err == nil {
				before, err = parent.Lstat(part)
			}
		}
		if err != nil {
			parent.Close()
			return nil, "", err
		}
		if !before.IsDir() || before.Mode()&os.ModeSymlink != 0 {
			parent.Close()
			return nil, "", errors.New("snapshot ancestor must be a real directory")
		}
		next, err := parent.OpenRoot(part)
		if err != nil {
			parent.Close()
			return nil, "", err
		}
		file, err := next.Open(".")
		if err != nil {
			next.Close()
			parent.Close()
			return nil, "", err
		}
		actual, statErr := file.Stat()
		closeErr := file.Close()
		after, afterErr := parent.Lstat(part)
		if errors.Join(statErr, closeErr, afterErr) != nil || after.Mode()&os.ModeSymlink != 0 || !os.SameFile(before, actual) || !os.SameFile(before, after) {
			next.Close()
			parent.Close()
			return nil, "", errors.New("snapshot directory changed while pinning")
		}
		if err = parent.Close(); err != nil {
			next.Close()
			return nil, "", err
		}
		parent = next
	}
	return parent, parts[len(parts)-1], nil
}
func readPinned(parent *os.Root, name string) (fileState, bool, error) {
	info, err := parent.Lstat(name)
	if os.IsNotExist(err) {
		return fileState{}, false, nil
	}
	if err != nil {
		return fileState{}, false, err
	}
	if !info.Mode().IsRegular() || info.Mode()&(os.ModeSymlink|os.ModeSetuid|os.ModeSetgid|os.ModeSticky) != 0 || info.Size() > 20<<20 {
		return fileState{}, false, errors.New("snapshot requires unprivileged regular files <=20MiB")
	}
	file, err := parent.Open(name)
	if err != nil {
		return fileState{}, false, err
	}
	opened, statErr := file.Stat()
	if statErr != nil || !os.SameFile(info, opened) {
		file.Close()
		return fileState{}, false, errors.New("snapshot file identity changed before read")
	}
	data, readErr := io.ReadAll(io.LimitReader(file, 20<<20+1))
	after, afterErr := file.Stat()
	closeErr := file.Close()
	current, currentErr := parent.Lstat(name)
	if errors.Join(readErr, afterErr, closeErr, currentErr) != nil || len(data) > 20<<20 || current.Mode()&os.ModeSymlink != 0 || !os.SameFile(info, current) || !os.SameFile(opened, after) || info.Size() != current.Size() || opened.Size() != after.Size() || !opened.ModTime().Equal(after.ModTime()) || !info.ModTime().Equal(current.ModTime()) || info.Mode() != current.Mode() {
		return fileState{}, false, errors.New("snapshot file changed during bounded read")
	}
	mode := "100644"
	if info.Mode().Perm()&0111 != 0 {
		mode = "100755"
	}
	return fileState{Mode: mode, SHA256: sha(data), Data: data, Perm: info.Mode().Perm()}, true, nil
}
func (m *Manager) currentSnapshot(ctx context.Context, directory string, base []treeEntry) (fileSnapshot, error) {
	data, err := m.run(ctx, directory, []string{"ls-files", "--cached", "--others", "--exclude-standard", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return nil, err
	}
	names := map[string]bool{}
	for _, entry := range base {
		names[entry.path] = true
	}
	for _, name := range bytes.Split(data, []byte{0}) {
		if len(name) == 0 {
			continue
		}
		if !validPath(string(name)) {
			return nil, errors.New("current worktree contains unsupported file path")
		}
		names[string(name)] = true
	}
	if len(names) > 4096 {
		return nil, errors.New("current snapshot exceeds 4096 files")
	}
	root, err := os.OpenRoot(directory)
	if err != nil {
		return nil, err
	}
	defer root.Close()
	out := fileSnapshot{}
	total := 0
	for name := range names {
		if err = ctx.Err(); err != nil {
			return nil, err
		}
		state, exists, e := readLiteral(root, name)
		if e != nil {
			return nil, e
		}
		if !exists {
			continue
		}
		total += len(state.Data)
		if total > 128<<20 {
			return nil, errors.New("current snapshot exceeds 128MiB")
		}
		out[name] = state
	}
	return out, nil
}
func (s fileSnapshot) digest() string { data, _ := json.Marshal(s); return sha(data) }
func (s fileSnapshot) digestWithPermissions() string {
	permissions := make(map[string]uint32, len(s))
	for name, state := range s {
		permissions[name] = uint32(state.Perm.Perm())
	}
	data, _ := json.Marshal(struct {
		Content     string            `json:"content"`
		Permissions map[string]uint32 `json:"permissions"`
	}{s.digest(), permissions})
	return sha(data)
}
func sameFile(a fileState, aOK bool, b fileState, bOK bool) bool {
	return aOK == bOK && (!aOK || (a.Mode == b.Mode && a.SHA256 == b.SHA256))
}
func paths(a, b fileSnapshot) []string {
	all := map[string]bool{}
	for name := range a {
		all[name] = true
	}
	for name := range b {
		all[name] = true
	}
	out := make([]string, 0, len(all))
	for name := range all {
		out = append(out, name)
	}
	slices.Sort(out)
	return out
}

func (m *Manager) cleanShared(ctx context.Context, h hub.WorktreeBinding, entries []treeEntry, base fileSnapshot) (fileSnapshot, error) {
	data, err := m.run(ctx, h.Workspace, []string{"ls-files", "--stage", "-z"}, nil, nil, 1<<20)
	if err != nil {
		return nil, err
	}
	expected := map[string]treeEntry{}
	for _, entry := range entries {
		expected[entry.path] = entry
	}
	count := 0
	for _, row := range bytes.Split(data, []byte{0}) {
		if len(row) == 0 {
			continue
		}
		prefix, path, ok := bytes.Cut(row, []byte{'\t'})
		fields := strings.Fields(string(prefix))
		entry, exists := expected[string(path)]
		if !ok || len(fields) != 3 || fields[2] != "0" || !exists || fields[0] != entry.mode || fields[1] != entry.oid {
			return nil, errors.New("shared Git index is dirty or conflicted; no adoption")
		}
		count++
	}
	if count != len(entries) {
		return nil, errors.New("shared Git index differs from saved base; no adoption")
	}
	current, err := m.currentSnapshot(ctx, h.Workspace, entries)
	if err != nil {
		return nil, err
	}
	if current.digest() != base.digest() {
		return nil, errors.New("shared workspace is dirty; adoption never overwrites or merges it")
	}
	return current, nil
}
