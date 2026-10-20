// INPUT: Explicit private directory, SQLite host plumbing and shared binding contracts.
// OUTPUT: Open and Store implementing owner/workspace create-or-verify identity bindings.
// POS: [storage adapter] Retains external runtime routing metadata without owning runtime execution or choosing its consumers.
package bindings

import (
	"context"
	"database/sql"
	"errors"
	"sync"

	"github.com/QuincySx/Alva/platform/storage"
	"github.com/QuincySx/Alva/platform/storage/sqlite"
)

type Store struct {
	mu     sync.Mutex
	db     *sql.DB
	lock   *sqlite.Lock
	closed bool
}

func Open(ctx context.Context, directory string) (*Store, error) {
	lock, err := sqlite.LockDatabase(directory, "engine.sqlite")
	if err != nil {
		return nil, err
	}
	db, err := sqlite.Open(ctx, directory, "engine.sqlite")
	if err != nil {
		lock.Close()
		return nil, err
	}
	if _, err = db.ExecContext(ctx, `CREATE TABLE IF NOT EXISTS native_bindings(id TEXT PRIMARY KEY,owner TEXT NOT NULL,cwd TEXT NOT NULL)`); err != nil {
		db.Close()
		lock.Close()
		return nil, err
	}
	return &Store{db: db, lock: lock}, nil
}
func (s *Store) ReadBinding(ctx context.Context, id string) (storage.SessionBinding, error) {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return storage.SessionBinding{}, storage.ErrClosed
	}
	var value storage.SessionBinding
	value.ID = id
	err := s.db.QueryRowContext(ctx, `SELECT owner,cwd FROM native_bindings WHERE id=?`, id).Scan(&value.Owner, &value.Workspace)
	if errors.Is(err, sql.ErrNoRows) {
		return storage.SessionBinding{}, storage.ErrNotFound
	}
	return value, err
}
func (s *Store) PutBinding(ctx context.Context, value storage.SessionBinding) error {
	if value.ID == "" || len(value.ID) > 128 || value.Owner == "" || len(value.Owner) > 256 || value.Workspace == "" || len(value.Workspace) > 4096 {
		return errors.New("bounded actual identity/owner/workspace required")
	}
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return storage.ErrClosed
	}
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if _, err = tx.ExecContext(ctx, `INSERT INTO native_bindings(id,owner,cwd) VALUES(?,?,?) ON CONFLICT(id) DO NOTHING`, value.ID, value.Owner, value.Workspace); err != nil {
		return err
	}
	var owner, workspace string
	if err = tx.QueryRowContext(ctx, `SELECT owner,cwd FROM native_bindings WHERE id=?`, value.ID).Scan(&owner, &workspace); err != nil {
		return err
	}
	if owner != value.Owner || workspace != value.Workspace {
		return storage.ErrBindingConflict
	}
	return tx.Commit()
}
func (s *Store) Close() error {
	s.mu.Lock()
	defer s.mu.Unlock()
	if s.closed {
		return nil
	}
	err := errors.Join(s.db.Close(), s.lock.Close())
	if err == nil {
		s.closed = true
	}
	return err
}

var _ storage.BindingStore = (*Store)(nil)
