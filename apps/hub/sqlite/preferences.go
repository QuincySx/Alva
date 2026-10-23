// INPUT: Shared Hub preference policy and the existing private product transaction helpers.
// OUTPUT: Durable application preferences with atomic optimistic updates.
// POS: [L7 SQLite adapter] Persistence and concurrency only; no second preference policy or session settings.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func preferences(ctx context.Context, q querier) (hub.Preferences, error) {
	p, err := read[hub.Preferences](ctx, q, `SELECT data FROM hub_preferences WHERE id=1`)
	if err != nil {
		return hub.Preferences{}, err
	}
	var revision uint64
	if err = q.QueryRowContext(ctx, `SELECT revision FROM hub_preferences WHERE id=1`).Scan(&revision); err != nil {
		return hub.Preferences{}, err
	}
	if revision != p.Revision {
		return hub.Preferences{}, errors.New("product preferences revision is corrupt")
	}
	if err = p.Validate(); err != nil {
		return hub.Preferences{}, errors.New("product preferences are corrupt")
	}
	return p, nil
}

func (s *Store) Preferences(ctx context.Context) (value hub.Preferences, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { value, err = preferences(ctx, tx); return err })
	if err != nil {
		return hub.Preferences{}, err
	}
	return value, nil
}

func (s *Store) UpdatePreferences(ctx context.Context, update hub.PreferenceUpdate) (value hub.Preferences, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, err := preferences(ctx, tx)
		if err != nil {
			return err
		}
		value, err = current.Apply(update)
		if err != nil {
			return err
		}
		data, err := body(value)
		if err != nil {
			return err
		}
		result, err := tx.ExecContext(ctx, `UPDATE hub_preferences SET revision=?,data=? WHERE id=1 AND revision=?`, value.Revision, data, update.ExpectedRevision)
		if err != nil {
			return err
		}
		n, err := result.RowsAffected()
		if err != nil {
			return err
		}
		if n != 1 {
			return hub.ErrPreferenceConflict
		}
		return nil
	})
	if err != nil {
		return hub.Preferences{}, err
	}
	return value, nil
}

var _ hub.PreferencesStore = (*Store)(nil)
