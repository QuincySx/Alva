// INPUT: Explicit native command settings and independent product/session-scoped host consent
// OUTPUT: Disabled edits and idle-only next-turn hook/diagnostic enablement
// POS: [L7 Hub product] Durable product snapshots never start commands or accept imported approval flags
package sqlite

import (
	"context"
	"database/sql"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func disabledCommands(value hub.CommandSettings) hub.CommandSettings {
	value.Hooks = append([]hub.HookSpec(nil), value.Hooks...)
	value.Diagnostics = append([]hub.DiagnosticSpec(nil), value.Diagnostics...)
	for i := range value.Hooks {
		value.Hooks[i].Enabled = false
		value.Hooks[i].HostAccessApproved = false
	}
	for i := range value.Diagnostics {
		value.Diagnostics[i].Enabled = false
		value.Diagnostics[i].HostAccessApproved = false
	}
	return value
}
func (s *Store) ReplaceCommands(ctx context.Context, id string, commands hub.CommandSettings) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	commands = disabledCommands(commands)
	if err := commands.Validate(); err != nil {
		return err
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before editing next-turn commands")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.Commands = commands
		return saveSettings(ctx, tx, value)
	})
}
func (s *Store) SetCommandEnabled(ctx context.Context, id, kind, commandID string, enabled, approved bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	if kind != "hook" && kind != "diagnostic" {
		return errors.New("unknown native command kind")
	}
	if enabled && !approved {
		return errors.New("explicit native command full host access approval required")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn commands")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		var selected *hub.NativeCommand
		if kind == "hook" {
			for i := range value.Commands.Hooks {
				if value.Commands.Hooks[i].ID == commandID {
					selected = &value.Commands.Hooks[i].NativeCommand
					break
				}
			}
		} else {
			for i := range value.Commands.Diagnostics {
				if value.Commands.Diagnostics[i].ID == commandID {
					selected = &value.Commands.Diagnostics[i].NativeCommand
					break
				}
			}
		}
		if selected == nil {
			return errors.New("native command not found in this product session")
		}
		selected.Enabled = enabled
		selected.HostAccessApproved = enabled && approved
		return saveSettings(ctx, tx, value)
	})
}
