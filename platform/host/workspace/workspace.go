// INPUT: os.Root, context and explicit security Guard/access grants
// OUTPUT: Workspace, OpenWorkspace, OpenFileSystem and confined native filesystem lifecycle
// POS: [host capability] Owns pinned roots and serialized native filesystem operations without model/tool policy.
package workspace

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"

	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

const maxFileBytes = 20 << 20

type fileRoot struct {
	path   string
	access abi.Access
	root   *os.Root
}
type Workspace struct {
	mu    sync.Mutex
	cwd   string
	roots []fileRoot
}

func OpenWorkspace(directory string) (*Workspace, error) { return OpenFileSystem(directory, nil) }
func OpenFileSystem(directory string, extra []security.AuthorizedRoot) (*Workspace, error) {
	if !filepath.IsAbs(directory) || len(extra) > 16 {
		return nil, errors.New("absolute workspace and bounded explicit roots required")
	}
	w := &Workspace{cwd: filepath.Clean(directory)}
	configs := append([]security.AuthorizedRoot{{Path: w.cwd, Access: abi.AccessWrite}}, extra...)
	for _, config := range configs {
		if !filepath.IsAbs(config.Path) || (config.Access != abi.AccessRead && config.Access != abi.AccessWrite) {
			w.Close()
			return nil, errors.New("invalid filesystem grant")
		}
		p := filepath.Clean(config.Path)
		st, err := os.Lstat(p)
		if err != nil || !st.IsDir() || st.Mode()&os.ModeSymlink != 0 {
			w.Close()
			return nil, errors.New("filesystem root must be an existing real directory")
		}
		root, err := os.OpenRoot(p)
		if err != nil {
			w.Close()
			return nil, err
		}
		w.roots = append(w.roots, fileRoot{p, config.Access, root})
	}
	sort.Slice(w.roots, func(i, j int) bool { return len(w.roots[i].path) > len(w.roots[j].path) })
	return w, nil
}
func (w *Workspace) Close() error {
	var err error
	for _, r := range w.roots {
		err = errors.Join(err, r.root.Close())
	}
	return err
}
func (w *Workspace) locate(path string, access abi.Access) (fileRoot, string, error) {
	if !filepath.IsAbs(path) {
		path = filepath.Join(w.cwd, path)
	}
	path = filepath.Clean(path)
	for _, r := range w.roots {
		if access == abi.AccessWrite && r.access != abi.AccessWrite {
			continue
		}
		relative, err := filepath.Rel(r.path, path)
		if err == nil && filepath.IsLocal(relative) {
			return r, relative, nil
		}
	}
	return fileRoot{}, "", security.ErrPermissionDenied
}
func inspect(root *os.Root, relative string, write bool) (os.FileInfo, error) {
	parts := strings.Split(filepath.ToSlash(relative), "/")
	var last os.FileInfo
	for i := range parts {
		st, err := root.Lstat(filepath.FromSlash(strings.Join(parts[:i+1], "/")))
		if errors.Is(err, os.ErrNotExist) && write && i == len(parts)-1 {
			return nil, nil
		}
		if err != nil {
			return nil, err
		}
		if st.Mode()&os.ModeSymlink != 0 {
			return nil, errors.New("file tool symlinks are not allowed")
		}
		last = st
	}
	return last, nil
}
func (w *Workspace) Resolve(ctx context.Context, path string, access abi.Access) (string, error) {
	if err := ctx.Err(); err != nil {
		return "", err
	}
	r, relative, err := w.locate(path, access)
	if err != nil {
		return "", err
	}
	if _, err = inspect(r.root, relative, access == abi.AccessWrite); err != nil {
		return "", err
	}
	return filepath.Join(r.path, relative), nil
}

// MakeDirectory is a native filesystem capability for explicitly authorized
// host recipes. It does not invent approval or recursively create parents.
// A model-facing caller must pass the normal SecurityPlugin before calling it.
func (w *Workspace) MakeDirectory(ctx context.Context, path string, guard *security.Guard) error {
	if guard == nil {
		return errors.New("filesystem directory mutation requires security guard")
	}
	resolved, err := guard.CheckPath(ctx, path, abi.AccessWrite)
	if err != nil {
		return err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	r, relative, err := w.locate(resolved, abi.AccessWrite)
	if err != nil {
		return err
	}
	st, err := inspect(r.root, relative, true)
	if err != nil {
		return err
	}
	if st != nil {
		if st.IsDir() {
			return nil
		}
		return errors.New("directory target already exists and is not a directory")
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	return r.root.Mkdir(relative, 0700)
}
func readFile(ctx context.Context, root *os.Root, relative string) ([]byte, error) {
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	before, err := inspect(root, relative, false)
	if err != nil {
		return nil, err
	}
	if before == nil || !before.Mode().IsRegular() || before.Size() > maxFileBytes {
		return nil, errors.New("file must be regular and at most 20 MiB")
	}
	f, err := root.Open(relative)
	if err != nil {
		return nil, err
	}
	defer f.Close()
	actual, err := f.Stat()
	if err != nil || !os.SameFile(before, actual) {
		return nil, errors.New("file changed during open")
	}
	data, err := io.ReadAll(io.LimitReader(f, maxFileBytes+1))
	if err != nil {
		return nil, err
	}
	if len(data) > maxFileBytes {
		return nil, errors.New("file exceeds 20 MiB")
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}

func atomicWrite(ctx context.Context, root *os.Root, name string, data []byte, before os.FileInfo) error {
	var random [12]byte
	if _, err := rand.Read(random[:]); err != nil {
		return err
	}
	stage := filepath.Join(filepath.Dir(name), ".alva-write-"+hex.EncodeToString(random[:]))
	mode := os.FileMode(0600)
	if before != nil {
		mode = before.Mode().Perm()
	}
	f, err := root.OpenFile(stage, os.O_WRONLY|os.O_CREATE|os.O_EXCL, mode)
	if err != nil {
		return err
	}
	defer root.Remove(stage)
	_, writeErr := f.Write(data)
	syncErr := f.Sync()
	closeErr := f.Close()
	if err = errors.Join(writeErr, syncErr, closeErr); err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	current, err := root.Lstat(name)
	if before == nil {
		if !errors.Is(err, os.ErrNotExist) {
			return errors.New("file appeared before atomic replace")
		}
	} else if err != nil || !os.SameFile(before, current) || before.Size() != current.Size() || !before.ModTime().Equal(current.ModTime()) {
		return errors.New("file changed before atomic replace")
	}
	return root.Rename(stage, name)
}

// Directory reports the configured workspace identity, never a reopenable capability.
func (w *Workspace) Directory() string { return w.cwd }
