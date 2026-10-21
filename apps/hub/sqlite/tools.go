// INPUT: Explicit product tool settings, independent consent and idle session transaction
// OUTPUT: ReplaceTools and SetToolEnabled next-turn durable capability snapshots
// POS: [L7 Hub product] Editing clears imported grants and never mutates an executing recipe.
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func disabledTools(v hub.ToolSettings) hub.ToolSettings {
	v.Planning, v.Notebook, v.Interaction, v.Utility = false, false, false, false
	v.Shell.Enabled, v.Shell.HostAccessApproved = false, false
	v.Web.Enabled, v.Web.NetworkAccessApproved = false, false
	return v
}
func (s *Store) changeTools(ctx context.Context, id string, change func(*hub.ToolSettings) error) error {
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn tools")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		if err = change(&value.Tools); err != nil {
			return err
		}
		return saveSettings(ctx, tx, value)
	})
}
func (s *Store) ReplaceTools(ctx context.Context, id string, value hub.ToolSettings) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	value = disabledTools(value)
	if err := value.Validate(); err != nil {
		return err
	}
	return s.changeTools(ctx, id, func(target *hub.ToolSettings) error { *target = value; return nil })
}
func (s *Store) SetToolEnabled(ctx context.Context, id, kind string, enabled, approved bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.changeTools(ctx, id, func(v *hub.ToolSettings) error {
		switch kind {
		case "planning":
			v.Planning = enabled
		case "notebook":
			v.Notebook = enabled
		case "interaction":
			v.Interaction = enabled
		case "utility":
			v.Utility = enabled
		case "shell":
			if enabled && !approved {
				return errors.New("independent full host shell consent required")
			}
			v.Shell.Enabled, v.Shell.HostAccessApproved = enabled, enabled && approved
		case "web":
			if enabled && !approved {
				return errors.New("independent web network consent required")
			}
			v.Web.Enabled, v.Web.NetworkAccessApproved = enabled, enabled && approved
		default:
			return errors.New("unknown tool capability")
		}
		return nil
	})
}
