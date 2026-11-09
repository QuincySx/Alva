// INPUT: Explicit product permission selection and the same transaction used by Turn admission.
// OUTPUT: Idle/native-only policy persistence with explicit Full confirmation and no capability grants.
// POS: [L7 product storage] Updates JSON next-Turn settings; no schema change or execution.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *Store) SetPermissionPolicy(ctx context.Context, id string, policy hub.PermissionPolicy, confirmed bool) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	if err := policy.Validate(); err != nil {
		return err
	}
	policy = policy.Effective()
	if policy == hub.PermissionFull && !confirmed {
		return errors.New("explicit Full scope confirmation required")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish the current Turn before changing permission policy")
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
		value.PermissionPolicy, value.PlanMode = policy, false
		return saveSettings(ctx, tx, value)
	})
}
