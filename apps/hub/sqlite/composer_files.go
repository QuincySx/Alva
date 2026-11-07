// INPUT: Shared immutable composer-file policy and explicit SQLite transactions.
// OUTPUT: Durable pre-session file catalog with exact immutable ID/hash receipts and scope-bound recent metadata.
// POS: [L7 SQLite adapter] Saves file metadata only; original and derived bytes remain in the product-owned private folder.
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"reflect"

	"github.com/QuincySx/Alva/apps/hub"
)

func composerFilesSchema(ctx context.Context, tx *sql.Tx) error {
	for _, statement := range []string{`CREATE TABLE hub_composer_files(id TEXT PRIMARY KEY,scope_kind TEXT NOT NULL,scope_id TEXT NOT NULL,created INTEGER NOT NULL,data BLOB NOT NULL)`, `CREATE INDEX hub_composer_files_scope ON hub_composer_files(scope_kind,scope_id,created,id)`, `CREATE TABLE hub_composer_file_sessions(file TEXT NOT NULL REFERENCES hub_composer_files(id),session TEXT NOT NULL REFERENCES hub_sessions(id),sha TEXT NOT NULL,PRIMARY KEY(file,session))`} {
		if _, err := tx.ExecContext(ctx, statement); err != nil {
			return err
		}
	}
	return nil
}
func validateComposerFilesSchema(ctx context.Context, tx *sql.Tx) error {
	rows, err := tx.QueryContext(ctx, `SELECT id,scope_kind,scope_id,created,data FROM hub_composer_files`)
	if err != nil {
		return err
	}
	for rows.Next() {
		if _, err = scanComposerFile(rows); err != nil {
			rows.Close()
			return err
		}
	}
	if err = errors.Join(rows.Err(), rows.Close()); err != nil {
		return err
	}
	grants, err := tx.QueryContext(ctx, `SELECT g.file,g.session,g.sha,f.data,s.id FROM hub_composer_file_sessions g LEFT JOIN hub_composer_files f ON f.id=g.file LEFT JOIN hub_sessions s ON s.id=g.session`)
	if err != nil {
		return err
	}
	defer grants.Close()
	for grants.Next() {
		var id, session, sha string
		var data []byte
		var owner sql.NullString
		if err = grants.Scan(&id, &session, &sha, &data, &owner); err != nil {
			return err
		}
		var file hub.ComposerFile
		if !owner.Valid || owner.String != session || json.Unmarshal(data, &file) != nil || file.Validate() != nil || file.ID != id || file.SHA256 != sha || file.Scope.Kind != "home" {
			return errors.New("corrupt captured file ownership grant")
		}
	}
	return grants.Err()
}
func scanComposerFile(row interface{ Scan(...any) error }) (hub.ComposerFile, error) {
	var f hub.ComposerFile
	var id, kind, scope string
	var created int64
	var data []byte
	if err := row.Scan(&id, &kind, &scope, &created, &data); err != nil {
		return f, err
	}
	if len(data) > 128<<10 || json.Unmarshal(data, &f) != nil || f.ID != id || f.Scope.Kind != kind || f.Scope.ID != scope || f.CreatedAt != created || f.Validate() != nil {
		return hub.ComposerFile{}, errors.New("corrupt composer file metadata")
	}
	return f, nil
}
func composerFile(ctx context.Context, q querier, id string) (hub.ComposerFile, error) {
	return scanComposerFile(q.QueryRowContext(ctx, `SELECT id,scope_kind,scope_id,created,data FROM hub_composer_files WHERE id=?`, id))
}
func (s *Store) SaveComposerFile(ctx context.Context, f hub.ComposerFile) (result hub.ComposerFile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = f.Validate(); err != nil {
		return result, err
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		old, e := composerFile(ctx, tx, f.ID)
		if e == nil {
			if !reflect.DeepEqual(old, f) {
				return hub.ErrComposerFileChanged
			}
			result = old
			return nil
		}
		if !errors.Is(e, sql.ErrNoRows) {
			return e
		}
		data, e := body(f)
		if e != nil {
			return e
		}
		_, e = tx.ExecContext(ctx, `INSERT INTO hub_composer_files(id,scope_kind,scope_id,created,data) VALUES(?,?,?,?,?)`, f.ID, f.Scope.Kind, f.Scope.ID, f.CreatedAt, data)
		if e == nil {
			result = f
		}
		return e
	})
	return result, err
}
func (s *Store) ComposerFile(ctx context.Context, id string) (result hub.ComposerFile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { result, err = composerFile(ctx, tx, id); return err })
	return result, err
}
func (s *Store) ListComposerFiles(ctx context.Context, scope hub.ComposerDraftScope, limit int) (result []hub.ComposerFile, err error) {
	defer func() { err = normalizeStoreError(err) }()
	if err = scope.Validate(); err != nil {
		return nil, err
	}
	if limit < 1 || limit > 100 {
		return nil, errors.New("composer recent file limit must be 1..100")
	}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		rows, e := tx.QueryContext(ctx, `SELECT id FROM hub_composer_files WHERE scope_kind=? AND scope_id=? ORDER BY created DESC,id DESC LIMIT ?`, scope.Kind, scope.ID, limit)
		if e != nil {
			return e
		}
		var ids []string
		for rows.Next() {
			var id string
			if e = rows.Scan(&id); e != nil {
				rows.Close()
				return e
			}
			ids = append(ids, id)
		}
		if e = errors.Join(rows.Err(), rows.Close()); e != nil {
			return e
		}
		for _, id := range ids {
			f, e := composerFile(ctx, tx, id)
			if e != nil {
				return e
			}
			result = append(result, f)
		}
		return nil
	})
	return result, err
}

var _ hub.ComposerFileStore = (*Store)(nil)

func composerFileSession(ctx context.Context, q querier, session, id, sha string) (bool, error) {
	var stored string
	err := q.QueryRowContext(ctx, `SELECT sha FROM hub_composer_file_sessions WHERE file=? AND session=?`, id, session).Scan(&stored)
	if errors.Is(err, sql.ErrNoRows) {
		return false, nil
	}
	if err != nil {
		return false, err
	}
	if stored != sha {
		return false, hub.ErrComposerFileChanged
	}
	return true, nil
}
func (s *Store) ComposerFileSession(ctx context.Context, session, id, sha string) (allowed bool, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		f, e := composerFile(ctx, tx, id)
		if e != nil {
			return e
		}
		if f.SHA256 != sha {
			return hub.ErrComposerFileChanged
		}
		allowed, e = composerFileSession(ctx, tx, session, id, sha)
		return e
	})
	return allowed, err
}

// Called by ordinary draft Save only after its PendingSessionID invariant.
// A session DTO alone never mints ownership for a home/foreign-session original.
func validateComposerDraftFiles(ctx context.Context, tx *sql.Tx, draft hub.ComposerDraft) error {
	for _, ref := range draft.References {
		if ref.Kind != "file" {
			continue
		}
		f, err := composerFile(ctx, tx, ref.ID)
		if err != nil {
			return err
		}
		if f.SHA256 != ref.SHA256 {
			return hub.ErrComposerFileChanged
		}
		if f.Scope == draft.Scope {
			continue
		}
		if f.Scope.Kind != "home" || draft.Scope.Kind != "session" {
			return hub.ErrComposerFileChanged
		}
		allowed, err := composerFileSession(ctx, tx, draft.Scope.ID, f.ID, f.SHA256)
		if err != nil {
			return err
		}
		if !allowed {
			return hub.ErrComposerFileChanged
		}
	}
	return nil
}

// Called only after the actual atomic PendingSessionID has been persisted in
// this transaction. Reading it back prevents a caller-created DTO/reference
// from becoming a grant. Original scope and bytes remain immutable.
func bindPendingComposerFiles(ctx context.Context, tx *sql.Tx, home hub.ComposerDraft) error {
	if home.Scope.Kind != "home" || home.PendingSessionID == "" {
		return errors.New("actual home-to-session binding required")
	}
	saved, err := composerDraft(ctx, tx, home.Scope)
	if err != nil {
		return err
	}
	if !reflect.DeepEqual(saved, home) {
		return hub.ErrComposerDraftConflict
	}
	if _, err = read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, home.PendingSessionID); err != nil {
		return err
	}
	for _, ref := range saved.References {
		if ref.Kind != "file" {
			continue
		}
		f, err := composerFile(ctx, tx, ref.ID)
		if err != nil {
			return err
		}
		if f.Scope != saved.Scope || f.SHA256 != ref.SHA256 {
			return hub.ErrComposerFileChanged
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_composer_file_sessions(file,session,sha) VALUES(?,?,?) ON CONFLICT(file,session) DO NOTHING`, f.ID, saved.PendingSessionID, f.SHA256); err != nil {
			return err
		}
		allowed, err := composerFileSession(ctx, tx, saved.PendingSessionID, f.ID, f.SHA256)
		if err != nil {
			return err
		}
		if !allowed {
			return hub.ErrComposerFileChanged
		}
	}
	return nil
}

var _ hub.ComposerFileOwnershipStore = (*Store)(nil)
