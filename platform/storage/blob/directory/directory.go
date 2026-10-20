// INPUT: context, os.Root, io, bytes, sync and immutable blob contracts.
// OUTPUT: Open, Store, immutable file publication and bounded reads from an explicitly supplied directory.
// POS: [storage adapter] Owns a pinned byte namespace; never interprets session identities or grants local execution.
package directory

import (
	"bytes"
	"context"
	"errors"
	"io"
	"os"
	"path/filepath"
	"sync"

	"github.com/QuincySx/Alva/platform/storage/blob"
)

type Store struct {
	mu   sync.RWMutex
	root *os.Root
}

// Open requires an existing non-symlink directory. The host owns its privacy,
// mount credentials and physical durability. The filesystem must support
// atomic hard-link publication and file/directory Sync; unsupported mounts fail
// explicitly. No rename-overwrite or unsafe direct-write fallback is used.
func Open(path string) (*Store, error) {
	if !filepath.IsAbs(path) {
		return nil, blob.ErrInvalid
	}
	// Lstat("alias/") may follow the final symlink on native filesystems.
	// Normalize before inspecting the last component, just as for file keys.
	path = filepath.Clean(path)
	info, err := os.Lstat(path)
	if err != nil {
		return nil, err
	}
	if !info.IsDir() || info.Mode()&os.ModeSymlink != 0 {
		return nil, blob.ErrInvalid
	}
	root, err := os.OpenRoot(path)
	if err != nil {
		return nil, err
	}
	actual, err := root.Stat(".")
	if err != nil || !os.SameFile(info, actual) {
		root.Close()
		return nil, errors.New("blob directory changed while opening")
	}
	return &Store{root: root}, nil
}

func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.root == nil {
		return nil
	}
	err := s.root.Close()
	s.root = nil
	return err
}

func (s *Store) Put(ctx context.Context, key string, data []byte) error {
	if err := blob.ValidatePut(key, data); err != nil {
		return err
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	if s.root == nil {
		return os.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		return err
	}
	if err := s.verifyExisting(ctx, key, data); !errors.Is(err, blob.ErrNotFound) {
		if err != nil {
			return err
		}
		return s.syncDirectory(ctx)
	}
	id, err := blob.NewKey()
	if err != nil {
		return err
	}
	stage := ".pending-" + id
	file, err := s.root.OpenFile(stage, os.O_WRONLY|os.O_CREATE|os.O_EXCL, 0600)
	if err != nil {
		return err
	}
	defer s.root.Remove(stage)
	_, writeErr := file.Write(data)
	err = errors.Join(writeErr, file.Sync(), file.Close())
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	if err = s.root.Link(stage, key); errors.Is(err, os.ErrExist) {
		if err = s.verifyExisting(ctx, key, data); err != nil {
			return err
		}
	} else if err != nil {
		return err
	}
	return s.syncDirectory(ctx)
}

func (s *Store) syncDirectory(ctx context.Context) error {
	// Publication may already have happened if Sync or cancellation fails.
	dir, err := s.root.Open(".")
	if err != nil {
		return err
	}
	if err = errors.Join(dir.Sync(), dir.Close()); err != nil {
		return err
	}
	return ctx.Err()
}

func (s *Store) open(key string) (*os.File, int64, error) {
	info, err := s.root.Lstat(key)
	if errors.Is(err, os.ErrNotExist) {
		return nil, 0, blob.ErrNotFound
	}
	if err != nil {
		return nil, 0, err
	}
	if !info.Mode().IsRegular() || info.Size() < 0 || info.Size() > blob.MaxObjectBytes {
		return nil, 0, blob.ErrInvalid
	}
	file, err := s.root.Open(key)
	if err != nil {
		return nil, 0, err
	}
	actual, err := file.Stat()
	if err != nil || !os.SameFile(info, actual) || actual.Size() != info.Size() {
		file.Close()
		return nil, 0, errors.New("blob changed while opening")
	}
	return file, info.Size(), nil
}

func (s *Store) verifyExisting(ctx context.Context, key string, expected []byte) error {
	file, size, err := s.open(key)
	if err != nil {
		return err
	}
	defer file.Close()
	if size != int64(len(expected)) {
		return blob.ErrConflict
	}
	data, err := io.ReadAll(io.LimitReader(file, blob.MaxObjectBytes+1))
	if err != nil {
		return err
	}
	if err = ctx.Err(); err != nil {
		return err
	}
	if !bytes.Equal(data, expected) {
		return blob.ErrConflict
	}
	return nil
}

func (s *Store) Read(ctx context.Context, key string, offset int64, limit int) ([]byte, error) {
	if err := blob.ValidateRead(key, offset, limit); err != nil {
		return nil, err
	}
	s.mu.RLock()
	defer s.mu.RUnlock()
	if s.root == nil {
		return nil, os.ErrClosed
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	file, size, err := s.open(key)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	if offset > size {
		return nil, blob.ErrInvalid
	}
	data := make([]byte, min(int64(limit), size-offset))
	if _, err := file.ReadAt(data, offset); err != nil {
		return nil, err
	}
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	return data, nil
}

var _ blob.Store = (*Store)(nil)
