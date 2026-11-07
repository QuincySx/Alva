// INPUT: Explicit model preference edits and the selected product transaction.
// OUTPUT: Independent revision CAS, corruption rejection and defaults frozen at actual Attempt admission.
// POS: [L7 product storage] Shares the v4 migration transaction; never changes a connection credential revision.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func modelPreferencesSchema(ctx context.Context, tx *sql.Tx) error {
	if _, err := tx.ExecContext(ctx, `CREATE TABLE hub_model_preferences(id INTEGER PRIMARY KEY CHECK(id=1),data BLOB NOT NULL)`); err != nil {
		return err
	}
	data, err := body(hub.ModelPreferencesSnapshot{Revision: 1, Entries: []hub.ModelPreference{}})
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_model_preferences(id,data) VALUES(1,?)`, data)
	return err
}
func modelPreferences(ctx context.Context, q querier) (hub.ModelPreferencesSnapshot, error) {
	v, err := read[hub.ModelPreferencesSnapshot](ctx, q, `SELECT data FROM hub_model_preferences WHERE id=1`)
	if err != nil {
		return v, err
	}
	return v, v.Validate()
}
func (s *Store) ModelPreferences(ctx context.Context) (v hub.ModelPreferencesSnapshot, result error) {
	defer func() { result = normalizeStoreError(result) }()
	result = s.transaction(ctx, func(tx *sql.Tx) error { var err error; v, err = modelPreferences(ctx, tx); return err })
	return
}
func (s *Store) SaveModelPreference(ctx context.Context, p hub.ModelPreference) (v hub.ModelPreferencesSnapshot, result error) {
	defer func() { result = normalizeStoreError(result) }()
	if result = p.Validate(); result != nil {
		return
	}
	result = s.transaction(ctx, func(tx *sql.Tx) error {
		var err error
		v, err = modelPreferences(ctx, tx)
		if err != nil {
			return err
		}
		index := -1
		for i, x := range v.Entries {
			if x.Key == p.Key {
				index = i
				if x.Revision != p.Revision {
					return hub.ErrModelPreferenceConflict
				}
				break
			}
		}
		if index < 0 {
			if p.Revision != 0 {
				return hub.ErrModelPreferenceConflict
			}
			if len(v.Entries) == 512 {
				return errors.New("model preference budget reached")
			}
		}
		p = p.Clone()
		p.Revision++
		v.Revision++
		if index < 0 {
			v.Entries = append(v.Entries, p)
		} else {
			v.Entries[index] = p
		}
		if err = v.Validate(); err != nil {
			return err
		}
		data, err := body(v)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `UPDATE hub_model_preferences SET data=? WHERE id=1`, data)
		return err
	})
	return
}
func captureModelOptions(ctx context.Context, tx *sql.Tx, session hub.Session, branch hub.Branch, turn hub.Turn) (hub.ModelOptions, error) {
	if session.Status == engine.Waiting && turn.ActiveAttemptID != "" {
		previous, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=? AND session=?`, turn.ActiveAttemptID, session.ID)
		if err != nil {
			return hub.ModelOptions{}, err
		}
		return previous.ModelOptions.Clone(), nil
	}
	value, err := modelPreferences(ctx, tx)
	if err != nil {
		return hub.ModelOptions{}, err
	}
	options := hub.ModelOptions{}
	key := hub.ModelPreferenceKey(branch.Target)
	for _, p := range value.Entries {
		if p.Key == key {
			options = p.Options.Clone()
			break
		}
	}
	if err = options.ValidateTarget(branch.Target); err != nil {
		return options, err
	}
	settings, err := settings(ctx, tx, session.ID)
	if err != nil {
		return options, err
	}
	settings.ModelOptions = options.Clone()
	if err = saveSettings(ctx, tx, settings); err != nil {
		return options, err
	}
	return options, nil
}
