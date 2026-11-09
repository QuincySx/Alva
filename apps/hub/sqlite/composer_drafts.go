// INPUT: Shared scoped composer drafts and private SQLite transactions.
// OUTPUT: Reopen-safe draft reads, monotonic compare-and-swap saves and schema validation.
// POS: [L7 SQLite adapter] Stores manual input without execution recovery, expiry or original deletion.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"reflect"
	"strings"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
)

// Dedicated drafts allow JSON escaping of the full 64 KiB text and 32 KiB
// preparations without silently shrinking either product budget.
const maxComposerDraftData = 640 << 10

// The product-owned preparation JSON is stored as bytes so JSON encoding does
// not compact or HTML-escape it and change a successfully committed receipt.
type composerDraftRecord struct {
	Draft            hub.ComposerDraft `json:"draft"`
	PreparedCommands []byte            `json:"prepared_commands,omitempty"`
}

func composerDraftsSchema(ctx context.Context, tx *sql.Tx) error {
	_, err := tx.ExecContext(ctx, `CREATE TABLE hub_composer_drafts(scope_kind TEXT NOT NULL CHECK(scope_kind IN ('home','session')),scope_id TEXT NOT NULL,revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL CHECK(length(data)<=655360),PRIMARY KEY(scope_kind,scope_id))`)
	return err
}

func validateComposerDraftsSchema(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT scope_kind,scope_id,revision,data FROM hub_composer_drafts`)
	if err != nil {
		return err
	}
	defer rows.Close()
	for rows.Next() {
		var scope hub.ComposerDraftScope
		var revision uint64
		var data []byte
		if err = rows.Scan(&scope.Kind, &scope.ID, &revision, &data); err != nil {
			return err
		}
		if _, err = decodeComposerDraft(data, scope, revision); err != nil {
			return err
		}
	}
	return rows.Err()
}

func decodeComposerDraft(data []byte, scope hub.ComposerDraftScope, revision uint64) (hub.ComposerDraft, error) {
	var record composerDraftRecord
	if len(data) > maxComposerDraftData || json.Unmarshal(data, &record) != nil {
		return hub.ComposerDraft{}, errors.New("composer draft record is corrupt")
	}
	value := record.Draft
	if len(value.PreparedCommands) != 0 {
		return hub.ComposerDraft{}, errors.New("composer draft contains duplicate preparation payloads")
	}
	value.PreparedCommands = record.PreparedCommands
	if value.Scope != scope || revision == 0 || value.Revision != revision || value.Validate() != nil {
		return hub.ComposerDraft{}, errors.New("composer draft record is corrupt")
	}
	return value, nil
}

func composerDraft(ctx context.Context, q querier, scope hub.ComposerDraftScope) (hub.ComposerDraft, error) {
	if err := scope.Validate(); err != nil {
		return hub.ComposerDraft{}, err
	}
	if scope.Kind == "session" {
		if _, err := read[hub.Session](ctx, q, liveSessionQuery, scope.ID); err != nil {
			return hub.ComposerDraft{}, err
		}
	}
	var revision uint64
	var data []byte
	err := q.QueryRowContext(ctx, `SELECT revision,data FROM hub_composer_drafts WHERE scope_kind=? AND scope_id=?`, scope.Kind, scope.ID).Scan(&revision, &data)
	if errors.Is(err, sql.ErrNoRows) {
		return hub.ComposerDraft{Scope: scope}, nil
	}
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	return decodeComposerDraft(data, scope, revision)
}

func (s *Store) ComposerDraft(ctx context.Context, scope hub.ComposerDraftScope) (value hub.ComposerDraft, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		value, err = composerDraft(ctx, tx, scope)
		return err
	})
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	return hub.CloneComposerDraft(value), nil
}

func (s *Store) SaveComposerDraft(ctx context.Context, next hub.ComposerDraft, expected uint64) (value hub.ComposerDraft, err error) {
	defer func() { err = normalizeStoreError(err) }()
	next = hub.CloneComposerDraft(next)
	if err = next.Validate(); err != nil {
		return hub.ComposerDraft{}, err
	}
	if expected >= 1<<63-1 {
		return hub.ComposerDraft{}, hub.ErrComposerDraftConflict
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := composerDraft(ctx, tx, next.Scope)
		if e != nil {
			return e
		}
		if current.Revision != expected {
			return hub.ErrComposerDraftConflict
		}
		if next.PendingSessionID != "" && next.PendingSessionID != current.PendingSessionID {
			return errors.New("only atomic first-Send admission may assign a pending session identity")
		}
		if e = validateComposerDraftFiles(ctx, tx, next); e != nil {
			return e
		}
		value = hub.CloneComposerDraft(next)
		value.Revision = expected + 1
		if e = writeComposerDraft(ctx, tx, value); e != nil {
			return e
		}
		if value.Scope.Kind == "home" && value.PendingSessionID != "" {
			return bindPendingComposerFiles(ctx, tx, value)
		}
		return nil
	})
	if err != nil {
		return hub.ComposerDraft{}, err
	}
	return hub.CloneComposerDraft(value), nil
}

var _ hub.ComposerDraftStore = (*Store)(nil)

func writeComposerDraft(ctx context.Context, tx *sql.Tx, value hub.ComposerDraft) error {
	record := composerDraftRecord{Draft: hub.CloneComposerDraft(value), PreparedCommands: append([]byte(nil), value.PreparedCommands...)}
	record.Draft.PreparedCommands = nil
	data, err := json.Marshal(record)
	if err != nil {
		return err
	}
	if len(data) > maxComposerDraftData {
		return errors.New("composer draft exceeds the encoded storage budget")
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_composer_drafts(scope_kind,scope_id,revision,data) VALUES(?,?,?,?) ON CONFLICT(scope_kind,scope_id) DO UPDATE SET revision=excluded.revision,data=excluded.data`, value.Scope.Kind, value.Scope.ID, value.Revision, data)
	return err
}

// createSessionTx is the single session-creation policy shared by ordinary
// CreateSession and the home draft's atomic first-Send identity binding.
func createSessionTx(ctx context.Context, tx *sql.Tx, title, workspace string, target hub.Target) (hub.Session, error) {
	if err := target.Validate(); err != nil {
		return hub.Session{}, err
	}
	if strings.TrimSpace(title) == "" || len(title) > 256 || !utf8.ValidString(title) || !filepath.IsAbs(workspace) {
		return hub.Session{}, errors.New("bounded title and absolute workspace required")
	}
	id, err := hub.NewID("hub-")
	if err != nil {
		return hub.Session{}, err
	}
	branchID, err := hub.NewID("branch-")
	if err != nil {
		return hub.Session{}, err
	}
	value := hub.Session{ID: id, Title: title, Workspace: workspace, ActiveBranchID: branchID, Status: engine.Succeeded, CreatedAt: now(), UpdatedAt: now()}
	data, err := body(value)
	if err != nil {
		return hub.Session{}, err
	}
	if _, err = tx.ExecContext(ctx, `INSERT INTO hub_sessions VALUES(?,?,?,?)`, value.ID, value.Status, value.UpdatedAt, data); err != nil {
		return hub.Session{}, err
	}
	if err = insertBranch(ctx, tx, hub.Branch{ID: branchID, SessionID: id, Target: target}); err != nil {
		return hub.Session{}, err
	}
	// Durable waits default on for every new session so a Turn waiting for a
	// human survives an app restart. Stored sessions keep their saved value.
	if err = saveSettings(ctx, tx, hub.Settings{Version: 1, SessionID: id, MCP: []hub.MCPServer{}, DurableWaits: true}); err != nil {
		return hub.Session{}, err
	}
	return value, nil
}

func (s *Store) CreateComposerSession(ctx context.Context, captured hub.ComposerDraft, title, workspace string, target hub.Target) (session hub.Session, value hub.ComposerDraft, err error) {
	defer func() { err = normalizeStoreError(err) }()
	captured = hub.CloneComposerDraft(captured)
	if err = captured.Validate(); err != nil || captured.Scope.Kind != "home" || captured.Revision == 0 || captured.Revision >= 1<<63-1 {
		return hub.Session{}, hub.ComposerDraft{}, errors.Join(errors.New("first Send requires a captured committed home draft"), err)
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		current, e := composerDraft(ctx, tx, captured.Scope)
		if e != nil {
			return e
		}
		if current.PendingSessionID != "" {
			revision := captured.Revision + 1
			if captured.PendingSessionID != "" {
				if captured.PendingSessionID != current.PendingSessionID {
					return hub.ErrComposerDraftConflict
				}
				revision = captured.Revision
			}
			check := hub.CloneComposerDraft(current)
			check.Revision, check.PendingSessionID = captured.Revision, captured.PendingSessionID
			if !reflect.DeepEqual(check, captured) || current.Revision != revision {
				return hub.ErrComposerDraftConflict
			}
			session, e = read[hub.Session](ctx, tx, liveSessionQuery, current.PendingSessionID)
			if e != nil {
				return e
			}
			branch, e := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=?`, session.ActiveBranchID)
			if e != nil {
				return e
			}
			if session.ID != current.PendingSessionID || session.Workspace != workspace || branch.SessionID != session.ID || branch.ID != session.ActiveBranchID || hub.RouteKey(branch.Target) != hub.RouteKey(target) {
				return errors.New("pending session workspace or target changed; open the actual saved session before sending")
			}
			value = current
			return nil
		}
		if !reflect.DeepEqual(current, captured) {
			return hub.ErrComposerDraftConflict
		}
		session, e = createSessionTx(ctx, tx, title, workspace, target)
		if e != nil {
			return e
		}
		value = hub.CloneComposerDraft(current)
		value.Revision++
		value.PendingSessionID = session.ID
		if e = writeComposerDraft(ctx, tx, value); e != nil {
			return e
		}
		return bindPendingComposerFiles(ctx, tx, value)
	})
	if err != nil {
		return hub.Session{}, hub.ComposerDraft{}, err
	}
	return session, hub.CloneComposerDraft(value), nil
}

var _ hub.ComposerDraftAdmissionStore = (*Store)(nil)
