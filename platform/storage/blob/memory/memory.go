// INPUT: context, bytes, sync and the immutable blob storage port.
// OUTPUT: Store and New with explicit object/byte budgets and owned buffers.
// POS: [storage adapter] Provides process-lifetime objects without filesystem, SQLite or SDK dependencies.
package memory

import (
	"bytes"
	"context"
	"sync"

	"github.com/QuincySx/Alva/platform/storage/blob"
)

type Limits struct {
	Bytes   int64
	Objects int
}

type Store struct {
	mu     sync.Mutex
	limits Limits
	used   int64
	values map[string][]byte
}

func New(limits Limits) (*Store, error) {
	if limits.Bytes < 1 || limits.Objects < 1 {
		return nil, blob.ErrInvalid
	}
	return &Store{limits: limits, values: make(map[string][]byte)}, nil
}

func (s *Store) Put(ctx context.Context, key string, data []byte) error {
	if err := blob.ValidatePut(key, data); err != nil {
		return err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return err
	}
	if previous, exists := s.values[key]; exists {
		if !bytes.Equal(previous, data) {
			return blob.ErrConflict
		}
		return nil
	}
	if len(s.values) >= s.limits.Objects || int64(len(data)) > s.limits.Bytes-s.used {
		return blob.ErrLimit
	}
	s.values[key] = bytes.Clone(data)
	s.used += int64(len(data))
	return nil
}

func (s *Store) Read(ctx context.Context, key string, offset int64, limit int) ([]byte, error) {
	if err := blob.ValidateRead(key, offset, limit); err != nil {
		return nil, err
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if err := ctx.Err(); err != nil {
		return nil, err
	}
	data, exists := s.values[key]
	if !exists {
		return nil, blob.ErrNotFound
	}
	if offset > int64(len(data)) {
		return nil, blob.ErrInvalid
	}
	end := min(int64(len(data)), offset+int64(limit))
	return bytes.Clone(data[offset:end]), nil
}

var _ blob.Store = (*Store)(nil)
