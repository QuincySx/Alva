// INPUT: Explicit pinned companion Skill scope and the same transaction used by Turn admission.
// OUTPUT: Idle native-session allowlist persistence with deep-copy and membership checks.
// POS: [L7 product storage] Constrains the actual next-Turn assembly, never broadens ordinary imports.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

func (s *Store) SetCompanionSkills(ctx context.Context, id string, scope *hub.CompanionSkillScope) (result error) {
	defer func() { result = normalizeStoreError(result) }()
	scope = scope.Clone()
	if err := scope.Validate(); err != nil {
		return err
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish the current Turn before changing companion Skills")
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
		if err != nil {
			return err
		}
		if branch.Target.EngineID != "alva" {
			return engine.ErrUnsupported
		}
		var companion string
		if err := tx.QueryRowContext(ctx, `SELECT companion FROM hub_companion_conversations WHERE session=?`, id).Scan(&companion); err != nil {
			return err
		}
		// A companion conversation must never become an unrestricted session.
		if scope == nil {
			return errors.New("companion Skills require an explicit restricted scope")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.CompanionSkills = scope
		value.SkillsEnabled = len(scope.Skills) > 0
		return saveSettings(ctx, tx, value)
	})
}
