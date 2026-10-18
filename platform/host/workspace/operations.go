// INPUT: Context, actual pinned Workspace, explicit security Guard and bounded update callback
// OUTPUT: ReadFile, ListDirectory and serialized Update without exporting root handles
// POS: [host capability] Retains authorization and physical IO while callers own references and transformation policy.
package workspace

import (
	"context"
	"errors"
	"io"
	"os"

	"github.com/QuincySx/Alva/sdk/agent/security"
	"github.com/QuincySx/Alva/sdk/kernel/abi"
)

var ErrDirectoryUnavailable = errors.New("directory is unavailable")

type DirectoryEntry struct {
	Name      string `json:"name"`
	Directory bool   `json:"directory"`
	Symlink   bool   `json:"symlink"`
}

type DirectoryListing struct {
	Entries        []DirectoryEntry `json:"entries"`
	Truncated      bool             `json:"truncated"`
	ContentsOpened bool             `json:"contents_opened"`
}

func (w *Workspace) authorize(ctx context.Context, path string, access abi.Access, guard *security.Guard) (fileRoot, string, error) {
	if guard == nil {
		return fileRoot{}, "", errors.New("filesystem operation requires security guard")
	}
	absolute, err := guard.CheckPath(ctx, path, access)
	if err != nil {
		return fileRoot{}, "", err
	}
	return w.locate(absolute, access)
}

func (w *Workspace) ReadFile(ctx context.Context, path string, guard *security.Guard) ([]byte, error) {
	root, relative, err := w.authorize(ctx, path, abi.AccessRead, guard)
	if err != nil {
		return nil, err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	return readFile(ctx, root.root, relative)
}

func (w *Workspace) ListDirectory(ctx context.Context, path string, guard *security.Guard) (DirectoryListing, error) {
	root, relative, err := w.authorize(ctx, path, abi.AccessRead, guard)
	if err != nil {
		return DirectoryListing{}, err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	f, err := root.root.Open(relative)
	if err != nil {
		return DirectoryListing{}, errors.Join(ErrDirectoryUnavailable, err)
	}
	defer f.Close()
	entries, err := f.ReadDir(257)
	if err != nil && !errors.Is(err, io.EOF) {
		return DirectoryListing{}, err
	}
	result := DirectoryListing{Truncated: len(entries) > 256}
	if result.Truncated {
		entries = entries[:256]
	}
	for _, entry := range entries {
		result.Entries = append(result.Entries, DirectoryEntry{entry.Name(), entry.IsDir(), entry.Type()&os.ModeSymlink != 0})
	}
	return result, nil
}

// Update keeps the current read, caller transformation and atomic replacement
// serialized with this Workspace's other operations. The callback may persist a
// backup before returning new bytes; any callback error prevents the write.
// The callback must not call another method on the same Workspace. This is not
// an OS compare-and-swap against noncooperating external filesystem writers.
func (w *Workspace) Update(ctx context.Context, path string, guard *security.Guard, transform func(previous []byte, exists bool) ([]byte, error)) error {
	if transform == nil {
		return errors.New("filesystem update requires transformation")
	}
	root, relative, err := w.authorize(ctx, path, abi.AccessWrite, guard)
	if err != nil {
		return err
	}
	w.mu.Lock()
	defer w.mu.Unlock()
	before, err := inspect(root.root, relative, true)
	if err != nil {
		return err
	}
	var previous []byte
	if before != nil {
		previous, err = readFile(ctx, root.root, relative)
		if err != nil {
			return err
		}
	}
	data, err := transform(previous, before != nil)
	if err != nil {
		return err
	}
	if len(data) > maxFileBytes {
		return errors.New("filesystem update exceeds 20 MiB")
	}
	return atomicWrite(ctx, root.root, relative, data, before)
}
