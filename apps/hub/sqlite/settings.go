// INPUT: product Settings and private product SQLite transaction
// OUTPUT: Settings, SaveSettings and explicit SetMCPEnabled
// POS: [L7 Hub product] Session-scoped configuration for next run, never mutates an active engine snapshot
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

func settings(ctx context.Context, tx *sql.Tx, id string) (hub.Settings, error) {
	if _, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id); err != nil {
		return hub.Settings{}, err
	}
	var data []byte
	err := tx.QueryRowContext(ctx, `SELECT data FROM hub_settings WHERE session=?`, id).Scan(&data)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.Settings{Version: 1, SessionID: id, MCP: []hub.MCPServer{}}, nil
	}
	if err != nil {
		return hub.Settings{}, err
	}
	var value hub.Settings
	if len(data) > 128<<10 || json.Unmarshal(data, &value) != nil || value.SessionID != id {
		return value, errors.New("product settings are corrupt")
	}
	return value, value.Validate()
}
func (s *Store) Settings(ctx context.Context, id string) (providerResult0 hub.Settings, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var value hub.Settings
	err := s.transaction(ctx, func(tx *sql.Tx) error { var err error; value, err = settings(ctx, tx, id); return err })
	return value, err
}
func saveSettings(ctx context.Context, tx *sql.Tx, value hub.Settings) error {
	if err := value.Validate(); err != nil {
		return err
	}
	data, err := body(value)
	if err != nil {
		return err
	}
	if len(data) > 128<<10 {
		return errors.New("product settings exceed 128 KiB durable budget")
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_settings(session,data) VALUES(?,?) ON CONFLICT(session) DO UPDATE SET data=excluded.data`, value.SessionID, data)
	return err
}
func (s *Store) SaveSettings(ctx context.Context, value hub.Settings) (providerResult0 error) {
	defer
	// Editing always creates a disabled snapshot, even if an untrusted import
	// claims it was approved. Enabling is an independent explicit operation.
	func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()

	value.MCP = append([]hub.MCPServer(nil), value.MCP...)
	for i := range value.MCP {
		value.MCP[i].Enabled = false
		value.MCP[i].HostAccessApproved = false
	}
	value.AEP = append([]hub.AEPBundle(nil), value.AEP...)
	for i := range value.AEP {
		value.AEP[i].Enabled = false
		value.AEP[i].HostAccessApproved = false
	}
	value.Commands = disabledCommands(value.Commands)
	value.Tools = disabledTools(value.Tools)
	value.MediaDecoder.Enabled = false
	value.MediaDecoder.HostAccessApproved = false
	value.Browser.Enabled, value.Browser.HostAccessApproved = false, false
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, value.SessionID)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("settings may only be edited while this product session is idle")
		}
		return saveSettings(ctx, tx, value)
	})
}
func (s *Store) SetMCPEnabled(ctx context.Context, id, serverID string, enabled, approved bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("settings changes apply next turn; finish current turn first")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		for i := range value.MCP {
			if value.MCP[i].ID == serverID {
				if enabled && !approved {
					return errors.New("explicit host/account access approval required")
				}
				value.MCP[i].Enabled = enabled
				value.MCP[i].HostAccessApproved = enabled && approved
				return saveSettings(ctx, tx, value)
			}
		}
		return errors.New("MCP server not found in this product session")
	})
}

// ReplaceMCP edits this capability only. A generic whole-settings import clears
// all imported process approvals, but the MCP page must not silently disable an
// unrelated already-approved AEP capability.
func (s *Store) ReplaceMCP(ctx context.Context, id string, servers []hub.MCPServer) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	servers = append([]hub.MCPServer(nil), servers...)
	for i := range servers {
		servers[i].Enabled = false
		servers[i].HostAccessApproved = false
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before editing next-turn MCP snapshots")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.MCP = servers
		return saveSettings(ctx, tx, value)
	})
}
func (s *Store) SetSkillsEnabled(ctx context.Context, id string, enabled bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn skills")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.SkillsEnabled = enabled
		return saveSettings(ctx, tx, value)
	})
}
func (s *Store) RecordSkillArchive(ctx context.Context, id, receipt string) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		for _, existing := range value.SkillArchives {
			if existing == receipt {
				return nil
			}
		}
		value.SkillArchives = append(value.SkillArchives, receipt)
		return saveSettings(ctx, tx, value)
	})
}

func (s *Store) SetSubagentsEnabled(ctx context.Context, id string, enabled bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn native SubAgent capability")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.SubagentsEnabled = enabled
		return saveSettings(ctx, tx, value)
	})
}

func (s *Store) SetPersistentMemory(ctx context.Context, id string, enabled bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("finish current Turn before changing next-turn workspace memory")
		}
		value, err := settings(ctx, tx, id)
		if err != nil {
			return err
		}
		value.PersistentMemory = enabled
		return saveSettings(ctx, tx, value)
	})
}
