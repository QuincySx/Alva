// INPUT: Saved-session planning choice and the selected product SQLite transaction.
// OUTPUT: Atomic idle/native-only SetPlanMode preserving every other next-turn setting and approval.
// POS: [L7 product storage] Serializes plan configuration with Turn admission; JSON storage needs no schema migration.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// Every ordinary, Worker, automation, fallback and wait-resume admission creates
// its attempt through beginAttempt in the same transaction as these settings.
// A separately authorized fallback route cannot lift a native read-only policy.
func checkPlanTarget(ctx context.Context, tx *sql.Tx, sessionID string, target hub.Target) error {
	if target.EngineID == "alva" {
		return nil
	}
	value, err := settings(ctx, tx, sessionID)
	if err != nil {
		return err
	}
	if value.PlanMode {
		return engine.ErrUnsupported
	}
	return nil
}

func (s *Store) SetPlanMode(ctx context.Context, id string, enabled bool) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn plan mode")
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
		if err != nil {
			return err
		}
		if branch.Target.EngineID != "alva" {
			return engine.ErrUnsupported
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.PlanMode = enabled
		value.PermissionPolicy = hub.PermissionAsk
		return saveSettings(ctx, tx, value)
	})
}
