// INPUT: Shared Hub workspace view policy and existing private SQLite transactions.
// OUTPUT: Session-scoped durable resource view CAS and atomic schema helpers.
// POS: [L7 SQLite adapter] Persists bounded observation locations without touching execution or native records.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func workspaceViewsSchema(ctx context.Context, tx *sql.Tx) error {
	_, err := tx.ExecContext(ctx, `CREATE TABLE hub_workspace_views(session TEXT PRIMARY KEY REFERENCES hub_sessions(id),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`)
	return err
}

func validateWorkspaceViewsSchema(ctx context.Context, q querier) error {
	var count int
	if err := q.QueryRowContext(ctx, `SELECT count(*) FROM hub_workspace_views WHERE session IS NULL OR revision<=0 OR data IS NULL`).Scan(&count); err != nil {
		return err
	}
	if count != 0 {
		return errors.New("workspace view schema contains invalid rows")
	}
	return nil
}

func workspaceView(ctx context.Context, q querier, id string) (hub.WorkspaceView, error) {
	if _, err := read[hub.Session](ctx, q, liveSessionQuery, id); err != nil {
		return hub.WorkspaceView{}, err
	}
	v, err := read[hub.WorkspaceView](ctx, q, `SELECT data FROM hub_workspace_views WHERE session=?`, id)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.WorkspaceView{SessionID: id}, nil
	}
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	var revision uint64
	if err = q.QueryRowContext(ctx, `SELECT revision FROM hub_workspace_views WHERE session=?`, id).Scan(&revision); err != nil {
		return hub.WorkspaceView{}, err
	}
	if v.SessionID != id || revision != v.Revision || v.Revision == 0 || v.Validate() != nil {
		return hub.WorkspaceView{}, errors.New("workspace view is corrupt")
	}
	return v, nil
}

func (s *Store) WorkspaceView(ctx context.Context, id string) (value hub.WorkspaceView, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { value, err = workspaceView(ctx, tx, id); return err })
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	return value, nil
}

func (s *Store) SaveWorkspaceView(ctx context.Context, next hub.WorkspaceView, expected uint64) (value hub.WorkspaceView, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = next.Validate(); err != nil {
		return value, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := workspaceView(ctx, tx, next.SessionID)
		if e != nil {
			return e
		}
		if current.Revision != expected || expected >= 1<<63-1 {
			return hub.ErrWorkspaceViewConflict
		}
		for _, tab := range next.Tabs {
			var owner string
			if e = tx.QueryRowContext(ctx, `SELECT session FROM hub_branches WHERE id=?`, tab.BranchID).Scan(&owner); e != nil {
				return e
			}
			if owner != next.SessionID {
				return errors.New("workspace tab belongs to another session branch")
			}
		}
		value = hub.CloneWorkspaceView(next)
		value.Revision = expected + 1
		data, e := body(value)
		if e != nil {
			return e
		}
		_, e = tx.ExecContext(ctx, `INSERT INTO hub_workspace_views(session,revision,data) VALUES(?,?,?) ON CONFLICT(session) DO UPDATE SET revision=excluded.revision,data=excluded.data`, value.SessionID, value.Revision, data)
		return e
	})
	if err != nil {
		return hub.WorkspaceView{}, err
	}
	return value, nil
}

var _ hub.WorkspaceViewStore = (*Store)(nil)
