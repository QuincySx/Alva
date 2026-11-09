// INPUT: Shared hub scope/approval policy and the private product SQLite transaction helpers.
// OUTPUT: Application capability intent with revision CAS, workspace approvals and session override transitions.
// POS: [L7 SQLite adapter] Persistence and atomicity only; ResolveSettings stays the single effective-settings rule.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"

	"github.com/QuincySx/Alva/apps/hub"
)

// appSettingsSchema is migration v9. Application intent is a singleton beside
// appearance preferences; approvals are per saved workspace and item.
func appSettingsSchema(ctx context.Context, tx *sql.Tx) error {
	for _, statement := range []string{
		`CREATE TABLE hub_app_settings(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`,
		`CREATE TABLE hub_workspace_approvals(workspace TEXT NOT NULL,kind TEXT NOT NULL,item TEXT NOT NULL,fingerprint TEXT NOT NULL,data BLOB NOT NULL,PRIMARY KEY(workspace,kind,item))`,
	} {
		if _, err := tx.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	data, err := body(hub.AppSettings{Revision: 1, Capabilities: hub.Capabilities{MCP: []hub.MCPServer{}}})
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_app_settings(id,revision,data) VALUES(1,1,?)`, data)
	return err
}

func validateAppSettingsSchema(ctx context.Context, tx *sql.Tx) error {
	if _, err := appSettings(ctx, tx); err != nil {
		return err
	}
	var columns int
	if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM pragma_table_info('hub_workspace_approvals') WHERE name IN ('workspace','kind','item') AND pk>0`).Scan(&columns); err != nil {
		return err
	}
	if columns != 3 {
		return errors.New("workspace approval schema lacks its exact identity key")
	}
	return nil
}

func appSettings(ctx context.Context, q querier) (hub.AppSettings, error) {
	value, err := read[hub.AppSettings](ctx, q, `SELECT data FROM hub_app_settings WHERE id=1`)
	if err != nil {
		return hub.AppSettings{}, err
	}
	var revision uint64
	if err = q.QueryRowContext(ctx, `SELECT revision FROM hub_app_settings WHERE id=1`).Scan(&revision); err != nil {
		return hub.AppSettings{}, err
	}
	if revision != value.Revision || value.Validate() != nil {
		return hub.AppSettings{}, errors.New("application settings are corrupt")
	}
	return value, nil
}

func (s *Store) AppSettings(ctx context.Context) (value hub.AppSettings, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { value, err = appSettings(ctx, tx); return err })
	return value, err
}

// SaveAppSettings stores intent only: any approval flag in the input is cleared.
func (s *Store) SaveAppSettings(ctx context.Context, expected uint64, capabilities hub.Capabilities) (value hub.AppSettings, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, err := appSettings(ctx, tx)
		if err != nil {
			return err
		}
		if current.Revision != expected {
			return hub.ErrAppSettingsConflict
		}
		value = hub.AppSettings{Revision: current.Revision + 1, Capabilities: capabilities.Intent()}
		if value.Capabilities.MCP == nil {
			value.Capabilities.MCP = []hub.MCPServer{}
		}
		if err = value.Validate(); err != nil {
			return err
		}
		data, err := body(value)
		if err != nil {
			return err
		}
		result, err := tx.ExecContext(ctx, `UPDATE hub_app_settings SET revision=?,data=? WHERE id=1 AND revision=?`, value.Revision, data, expected)
		if err != nil {
			return err
		}
		if n, err := result.RowsAffected(); err != nil || n != 1 {
			return errors.Join(err, hub.ErrAppSettingsConflict)
		}
		return nil
	})
	if err != nil {
		return hub.AppSettings{}, err
	}
	return value, nil
}

func (s *Store) WorkspaceApprovals(ctx context.Context, workspace string) (values []hub.WorkspaceApproval, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		rows, err := tx.QueryContext(ctx, `SELECT kind,item,fingerprint,data FROM hub_workspace_approvals WHERE workspace=? ORDER BY kind,item LIMIT 1025`, workspace)
		if err != nil {
			return err
		}
		defer rows.Close()
		for rows.Next() {
			var kind, item, fingerprint string
			var data []byte
			if err = rows.Scan(&kind, &item, &fingerprint, &data); err != nil {
				return err
			}
			var value hub.WorkspaceApproval
			if len(data) > 16<<10 || json.Unmarshal(data, &value) != nil || value.Workspace != workspace || value.Kind != kind || value.Item != item || value.Fingerprint != fingerprint || value.Validate() != nil {
				return errors.New("workspace approval is corrupt")
			}
			values = append(values, value)
		}
		if len(values) > 1024 {
			return errors.New("workspace approvals exceed 1024 items")
		}
		return rows.Err()
	})
	return values, err
}

func (s *Store) SaveWorkspaceApproval(ctx context.Context, approval hub.WorkspaceApproval) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	if approval.ApprovedAt == 0 {
		approval.ApprovedAt = now()
	}
	if err = approval.Validate(); err != nil {
		return err
	}
	data, err := body(approval)
	if err != nil {
		return err
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		_, err := tx.ExecContext(ctx, `INSERT INTO hub_workspace_approvals(workspace,kind,item,fingerprint,data) VALUES(?,?,?,?,?) ON CONFLICT(workspace,kind,item) DO UPDATE SET fingerprint=excluded.fingerprint,data=excluded.data`,
			approval.Workspace, approval.Kind, approval.Item, approval.Fingerprint, data)
		return err
	})
}

func (s *Store) DeleteWorkspaceApproval(ctx context.Context, workspace, kind, item string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		_, err := tx.ExecContext(ctx, `DELETE FROM hub_workspace_approvals WHERE workspace=? AND kind=? AND item=?`, workspace, kind, item)
		return err
	})
}

func (s *Store) idleSettings(ctx context.Context, tx *sql.Tx, id string) (hub.Settings, error) {
	session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
	if err != nil {
		return hub.Settings{}, err
	}
	if hub.BlocksInput(session.Status) {
		return hub.Settings{}, errors.New("finish current Turn before changing this session's own settings")
	}
	return settings(ctx, tx, id)
}

// ReplaceSessionCapabilities edits an overriding session's configuration. It
// never enables anything; a host item whose configuration changed is switched off.
func (s *Store) ReplaceSessionCapabilities(ctx context.Context, id string, next hub.Capabilities) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, err := s.idleSettings(ctx, tx, id)
		if err != nil {
			return err
		}
		if !value.OverridesCapabilities() {
			return errors.New("this session uses application settings; choose session-only settings first")
		}
		value = value.WithCapabilities(hub.CarryEnablement(value.Capabilities(), next, true))
		value.CapabilityOverride = true
		return saveSettings(ctx, tx, value)
	})
}

// SetSessionCapabilityEnabled is the explicit per-session consent: enabling a
// host item in an overriding session approves exactly its current snapshot.
func (s *Store) SetSessionCapabilityEnabled(ctx context.Context, id, kind, item string, enabled bool) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, err := s.idleSettings(ctx, tx, id)
		if err != nil {
			return err
		}
		if !value.OverridesCapabilities() {
			return errors.New("this session uses application settings; choose session-only settings first")
		}
		capabilities := value.Capabilities()
		if err = capabilities.SetEnabled(kind, item, enabled, enabled); err != nil {
			return err
		}
		value = value.WithCapabilities(capabilities)
		value.CapabilityOverride = true
		return saveSettings(ctx, tx, value)
	})
}

// SetCapabilityOverride starts a session-only snapshot from initial (already
// resolved for this workspace by the product), or returns to application
// settings by clearing the session's capability sections.
func (s *Store) SetCapabilityOverride(ctx context.Context, id string, override bool, initial hub.Capabilities) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		value, err := s.idleSettings(ctx, tx, id)
		if err != nil {
			return err
		}
		if override {
			if value.OverridesCapabilities() {
				return errors.New("this session already keeps its own settings")
			}
			value = value.WithCapabilities(initial)
		} else {
			value = value.WithCapabilities(hub.Capabilities{})
		}
		value.CapabilityOverride = override
		return saveSettings(ctx, tx, value)
	})
}

var _ hub.AppSettingsStore = (*Store)(nil)
