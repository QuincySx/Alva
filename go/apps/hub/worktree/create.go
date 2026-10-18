// INPUT: Saved repository-root workspace, product Worker identity and explicit approved Git Manager
// OUTPUT: New managed no-checkout worktree populated through bounded direct commit blobs
// POS: [L7 Hub product] Creation never adopts changes, runs repository hooks/filters, resets user worktrees or commits/pushes
package worktree

import (
	"bytes"
	"context"
	"encoding/hex"
	"errors"
	"os"
	"path/filepath"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/go/apps/hub"
)

type treeEntry struct{ mode, oid, path string }

func validWorker(id string) bool {
	if !strings.HasPrefix(id, "worker-") || len(id) > 80 {
		return false
	}
	for _, c := range id {
		if !(c >= 'a' && c <= 'z' || c >= '0' && c <= '9' || c == '-') {
			return false
		}
	}
	return len(id) > 7
}
func validOID(id string) bool {
	if len(id) != 40 && len(id) != 64 {
		return false
	}
	_, err := hex.DecodeString(id)
	return err == nil && strings.ToLower(id) == id
}
func (m *Manager) repository(ctx context.Context, workspace string) (string, string, error) {
	if !filepath.IsAbs(workspace) || strings.ContainsAny(workspace, "\x00\r\n") {
		return "", "", errors.New("saved absolute repository-root workspace required")
	}
	canonical, err := filepath.EvalSymlinks(workspace)
	if err != nil {
		return "", "", err
	}
	data, err := m.run(ctx, canonical, []string{"rev-parse", "--show-toplevel"}, nil, nil, 4096)
	if err != nil {
		return "", "", errors.New("isolated Worker requires an existing Git repository")
	}
	top, err := filepath.EvalSymlinks(strings.TrimSuffix(string(data), "\n"))
	if err != nil || top != canonical {
		return "", "", errors.New("isolated Worker requires the repository toplevel, not a subdirectory")
	}
	data, err = m.run(ctx, canonical, []string{"rev-parse", "--verify", "HEAD^{commit}"}, nil, nil, 256)
	if err != nil {
		return "", "", errors.New("isolated Worker requires at least one existing commit")
	}
	oid := strings.TrimSpace(string(data))
	if !validOID(oid) {
		return "", "", errors.New("Git returned an invalid commit identity")
	}
	return canonical, oid, nil
}
func parseTree(data []byte) ([]treeEntry, error) {
	var entries []treeEntry
	for _, row := range bytes.Split(data, []byte{0}) {
		if len(row) == 0 {
			continue
		}
		prefix, path, ok := bytes.Cut(row, []byte{'\t'})
		fields := strings.Fields(string(prefix))
		if !ok || len(fields) != 3 || (fields[0] != "100644" && fields[0] != "100755") || fields[1] != "blob" || !validOID(fields[2]) || len(path) > 4096 || !utf8.Valid(path) {
			return nil, errors.New("worktree source contains unsupported mode, symlink, submodule or invalid path")
		}
		name := string(path)
		if !validPath(name) {
			return nil, errors.New("worktree source path is not clean and local")
		}
		for _, part := range strings.Split(filepath.ToSlash(name), "/") {
			if strings.EqualFold(part, ".git") {
				return nil, errors.New("worktree source cannot replace Git metadata")
			}
		}
		entries = append(entries, treeEntry{fields[0], fields[2], name})
		if len(entries) > 4096 {
			return nil, errors.New("worktree source exceeds 4096 file budget")
		}
	}
	return entries, nil
}

func (m *Manager) Create(ctx context.Context, workspace, workerID string) (handle hub.WorktreeBinding, result error) {
	owned, end, err := m.operation(ctx)
	if err != nil {
		return handle, err
	}
	defer end()
	ctx = owned
	if !validWorker(workerID) {
		return handle, errors.New("actual generated product Worker identity required")
	}
	canonical, base, err := m.repository(ctx, workspace)
	if err != nil {
		return handle, err
	}
	if strings.HasPrefix(m.config.Root+string(filepath.Separator), canonical+string(filepath.Separator)) {
		return handle, errors.New("managed worktree root must be outside the shared repository")
	}
	data, err := m.run(ctx, canonical, []string{"ls-tree", "-rz", "--full-tree", base}, nil, nil, 1<<20)
	if err != nil {
		return handle, err
	}
	entries, err := parseTree(data)
	if err != nil {
		return handle, err
	}
	path := filepath.Join(m.config.Root, "trees", workerID)
	if _, err = os.Lstat(path); !os.IsNotExist(err) {
		return handle, errors.New("managed target already exists or cannot be safely resolved")
	}
	handle = hub.WorktreeBinding{Version: 1, WorkerID: workerID, Workspace: canonical, Path: path, Branch: "alva/" + workerID, BaseCommit: base}
	// Never checkout: repository-controlled post-checkout hooks and smudge
	// filters are not part of the approved Git management operation.
	if _, err = m.run(ctx, canonical, []string{"worktree", "add", "--no-checkout", "-b", handle.Branch, "--", path, base}, nil, nil, 64<<10); err != nil {
		return handle, err
	}
	if err = os.Chmod(path, 0700); err != nil {
		return handle, err
	}
	root, err := os.OpenRoot(path)
	if err != nil {
		return handle, err
	}
	defer root.Close()
	total := 0
	for _, entry := range entries {
		body, err := m.run(ctx, canonical, []string{"cat-file", "blob", entry.oid}, nil, nil, 20<<20)
		if err != nil {
			return handle, err
		}
		total += len(body)
		if total > 128<<20 {
			return handle, errors.New("worktree source exceeds 128MiB total blob budget; partial managed target retained")
		}
		if parent := filepath.Dir(entry.path); parent != "." {
			if err = root.MkdirAll(parent, 0700); err != nil {
				return handle, err
			}
		}
		mode := os.FileMode(0600)
		if entry.mode == "100755" {
			mode = 0700
		}
		file, err := root.OpenFile(entry.path, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
		if err != nil {
			return handle, err
		}
		_, err = file.Write(body)
		err = errors.Join(err, file.Sync(), file.Close())
		if err != nil {
			return handle, err
		}
	}
	if _, err = m.run(ctx, path, []string{"read-tree", base}, nil, nil, 64<<10); err != nil {
		return handle, err
	}
	return handle, nil
}
