// INPUT: context, database/sql, encoding/json, hub, storage/sqlite and system/privatefs
// OUTPUT: Store implementing isolated durable Hub facts, tombstone-safe Attempt claims, superseded-delta deletion, Turn-sequenced history and exclusive recovery
// POS: [L7 Hub product] Product SQLite authority separate from native SessionRepository execution records
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"fmt"
	"net/url"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/ncruces/go-sqlite3"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

type Store struct {
	db *sql.DB
	// list is a second, read-only connection to the same database for the
	// bounded session-list, search and navigation reads. The write connection
	// is single (SetMaxOpenConns(1)) so every transaction serializes; a list
	// read queued behind a long write or read there used to leave the sidebar
	// and search on "loading". The read connection never writes, so write
	// serialization is unchanged; SQLite's own lock only makes a commit wait
	// for an in-flight millisecond-scale read (busy_timeout bounds it).
	list *sql.DB
	lock *private.Lock
}

func Open(ctx context.Context, dir string) (*Store, error) {
	lock, err := private.LockDatabase(dir, "product.sqlite")
	if err != nil {
		return nil, err
	}
	db, err := private.Open(ctx, dir, "product.sqlite")
	if err != nil {
		lock.Close()
		return nil, err
	}
	s := &Store{db: db, lock: lock}
	var version, existing int
	if err = db.QueryRowContext(ctx, `PRAGMA user_version`).Scan(&version); err != nil {
		s.Close()
		return nil, err
	}
	if version < 0 || version > 11 {
		s.Close()
		return nil, errors.New("unsupported Go product database schema; existing data was not modified")
	}
	if version == 0 {
		if err = db.QueryRowContext(ctx, `SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'`).Scan(&existing); err != nil {
			s.Close()
			return nil, err
		}
		if existing != 0 {
			s.Close()
			return nil, errors.New("unrecognized unversioned product database; choose a new explicit data directory, existing data was not modified")
		}
	}
	schema := []string{
		`CREATE TABLE IF NOT EXISTS hub_sessions(id TEXT PRIMARY KEY,status TEXT NOT NULL,updated INTEGER NOT NULL,data BLOB NOT NULL)`,
		`CREATE TABLE IF NOT EXISTS hub_branches(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),route TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(session,route))`,
		`CREATE INDEX IF NOT EXISTS hub_native_route ON hub_branches(json_extract(data,'$.target.engine_id'),json_extract(data,'$.native_session_id'))`,
		`CREATE TABLE IF NOT EXISTS hub_turns(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),seq INTEGER NOT NULL,status TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(session,seq))`,
		`CREATE TABLE IF NOT EXISTS hub_attempts(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),turn TEXT NOT NULL REFERENCES hub_turns(id),status TEXT NOT NULL,data BLOB NOT NULL)`,
		`CREATE TABLE IF NOT EXISTS hub_events(seq INTEGER PRIMARY KEY AUTOINCREMENT,session TEXT NOT NULL REFERENCES hub_sessions(id),turn TEXT REFERENCES hub_turns(id),attempt TEXT REFERENCES hub_attempts(id),data BLOB NOT NULL)`,
		`CREATE INDEX IF NOT EXISTS hub_events_session ON hub_events(session,seq)`,
		`CREATE INDEX IF NOT EXISTS hub_events_turn ON hub_events(turn,seq)`,
		`CREATE TABLE IF NOT EXISTS hub_fallback(session TEXT PRIMARY KEY REFERENCES hub_sessions(id),data BLOB NOT NULL)`,
		`CREATE TABLE IF NOT EXISTS hub_handoffs(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),data BLOB NOT NULL)`,
		`CREATE TABLE IF NOT EXISTS hub_settings(session TEXT PRIMARY KEY REFERENCES hub_sessions(id),data BLOB NOT NULL)`,
		`CREATE TABLE IF NOT EXISTS hub_teams(id TEXT PRIMARY KEY,lead TEXT NOT NULL REFERENCES hub_sessions(id),status TEXT NOT NULL,data BLOB NOT NULL)`,
		`CREATE UNIQUE INDEX IF NOT EXISTS hub_team_active_lead ON hub_teams(lead) WHERE status='active'`,
		`CREATE TABLE IF NOT EXISTS hub_workers(id TEXT PRIMARY KEY,team TEXT NOT NULL REFERENCES hub_teams(id),session TEXT NOT NULL UNIQUE REFERENCES hub_sessions(id),state TEXT NOT NULL,name TEXT NOT NULL,data BLOB NOT NULL,UNIQUE(team,name))`,
		`CREATE TABLE IF NOT EXISTS hub_worker_tasks(seq INTEGER PRIMARY KEY AUTOINCREMENT,id TEXT NOT NULL UNIQUE,team TEXT NOT NULL REFERENCES hub_teams(id),worker TEXT NOT NULL REFERENCES hub_workers(id),status TEXT NOT NULL,turn TEXT UNIQUE REFERENCES hub_turns(id),data BLOB NOT NULL)`,
		`CREATE INDEX IF NOT EXISTS hub_worker_queue ON hub_worker_tasks(worker,status,seq)`,
		`CREATE TABLE IF NOT EXISTS hub_automations(id TEXT PRIMARY KEY,session TEXT NOT NULL REFERENCES hub_sessions(id),enabled INTEGER NOT NULL,next INTEGER NOT NULL,running TEXT NOT NULL,data BLOB NOT NULL)`,
		`CREATE INDEX IF NOT EXISTS hub_automation_due ON hub_automations(enabled,running,next)`,
	}
	initialization, err := db.BeginTx(ctx, nil)
	if err != nil {
		s.Close()
		return nil, err
	}
	for _, statement := range schema {
		if _, err = initialization.ExecContext(ctx, statement); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if version < 2 {
		if _, err = initialization.ExecContext(ctx, `CREATE TABLE hub_preferences(id INTEGER PRIMARY KEY CHECK(id=1),revision INTEGER NOT NULL CHECK(revision>0),data BLOB NOT NULL)`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		data, encodeErr := body(hub.DefaultPreferences())
		if encodeErr != nil {
			initialization.Rollback()
			s.Close()
			return nil, encodeErr
		}
		if _, err = initialization.ExecContext(ctx, `INSERT INTO hub_preferences(id,revision,data) VALUES(1,1,?)`, data); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=2`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if _, err = preferences(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 3 {
		for _, statement := range []string{
			`CREATE TABLE hub_connections(id TEXT PRIMARY KEY,revision INTEGER NOT NULL CHECK(revision>0),created INTEGER NOT NULL,data BLOB NOT NULL)`,
			`CREATE TABLE hub_connection_credentials(connection TEXT PRIMARY KEY REFERENCES hub_connections(id),data BLOB NOT NULL)`,
			`PRAGMA user_version=3`,
		} {
			if _, err = initialization.ExecContext(ctx, statement); err != nil {
				initialization.Rollback()
				s.Close()
				return nil, err
			}
		}
	}
	if _, err = connections(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 4 {
		for _, statement := range []string{
			`CREATE TABLE hub_goals(id TEXT PRIMARY KEY,request TEXT NOT NULL UNIQUE,created INTEGER NOT NULL,scope_kind TEXT NOT NULL,workspace TEXT NOT NULL,session TEXT NOT NULL,data BLOB NOT NULL)`,
			`CREATE INDEX hub_goal_scope ON hub_goals(scope_kind,workspace,session,created,id)`,
		} {
			if _, err = initialization.ExecContext(ctx, statement); err != nil {
				initialization.Rollback()
				s.Close()
				return nil, err
			}
		}
		if err = modelPreferencesSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=4`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateGoalsSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if _, err = modelPreferences(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 5 {
		if err = companionSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if err = workspaceViewsSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=5`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateCompanionSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if err = validateWorkspaceViewsSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 6 {
		if err = composerDraftsSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if err = composerFilesSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=6`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateComposerDraftsSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if err = validateComposerFilesSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 7 {
		if err = connectionCreationsSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=7`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateConnectionCreationsSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 8 {
		if err = developerWorkspacesSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=8`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateDeveloperWorkspacesSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if version < 9 {
		if err = appSettingsSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=9`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if version < 10 {
		if err = pendingInputsSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=10`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if version < 11 {
		if err = versionLinesSchema(ctx, initialization); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=11`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
	}
	if err = validateAppSettingsSchema(ctx, initialization); err != nil {
		initialization.Rollback()
		s.Close()
		return nil, err
	}
	if err = initialization.Commit(); err != nil {
		s.Close()
		return nil, err
	}
	if err = s.Recover(ctx); err != nil {
		s.Close()
		return nil, err
	}
	if err = s.RecoverWorktrees(ctx); err != nil {
		s.Close()
		return nil, err
	}
	if err = s.RecoverCompanions(ctx); err != nil {
		s.Close()
		return nil, err
	}
	if s.list, err = private.Open(ctx, dir, "product.sqlite", func(connection *sqlite3.Conn) error { return connection.Exec("PRAGMA query_only=ON") }); err != nil {
		s.Close()
		return nil, err
	}
	return s, nil
}
func (s *Store) Close() (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	var listErr error
	if s.list != nil {
		listErr = s.list.Close()
	}
	return errors.Join(listErr, s.db.Close(), s.lock.Close())
}
func now() int64 { return time.Now().UTC().UnixMilli() }

type querier interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

func read[T any](ctx context.Context, q querier, query string, args ...any) (T, error) {
	var value T
	var body []byte
	if err := q.QueryRowContext(ctx, query, args...).Scan(&body); err != nil {
		return value, err
	}
	if len(body) > 128<<10 {
		return value, errors.New("product row exceeds metadata budget")
	}
	if err := json.Unmarshal(body, &value); err != nil {
		return value, errors.New("product record is corrupt")
	}
	return value, nil
}
func body(value any) ([]byte, error) {
	data, err := json.Marshal(value)
	if err != nil {
		return nil, err
	}
	if len(data) > 128<<10 {
		return nil, errors.New("product metadata exceeds 128 KiB")
	}
	return data, nil
}
func (s *Store) transaction(ctx context.Context, fn func(*sql.Tx) error) error {
	tx, err := s.db.BeginTx(ctx, nil)
	if err != nil {
		return err
	}
	defer tx.Rollback()
	if err = fn(tx); err != nil {
		return err
	}
	return tx.Commit()
}
func writeSession(ctx context.Context, tx *sql.Tx, value hub.Session) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_sessions SET status=?,updated=?,data=? WHERE id=?`, value.Status, value.UpdatedAt, data, value.ID)
	return err
}
func writeBranch(ctx context.Context, tx *sql.Tx, value hub.Branch) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_branches SET data=? WHERE id=? AND session=?`, data, value.ID, value.SessionID)
	return err
}
func writeTurn(ctx context.Context, tx *sql.Tx, value hub.Turn) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_turns SET status=?,data=? WHERE id=? AND session=?`, value.Status, data, value.ID, value.SessionID)
	return err
}
func writeAttempt(ctx context.Context, tx *sql.Tx, value hub.Attempt) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `UPDATE hub_attempts SET status=?,data=? WHERE id=? AND session=?`, value.Status, data, value.ID, value.SessionID)
	return err
}
func (s *Store) CreateSession(ctx context.Context, title, workspace string, target hub.Target) (value hub.Session, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		var createErr error
		value, createErr = createSessionTx(ctx, tx, title, workspace, target)
		return createErr
	})
	return value, err
}

func insertBranch(ctx context.Context, tx *sql.Tx, value hub.Branch) error {
	return insertLineBranch(ctx, tx, value, "")
}

// insertLineBranch stores a branch in a version line (” for a session
// without versions).
func insertLineBranch(ctx context.Context, tx *sql.Tx, value hub.Branch, line string) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	if line == "" {
		// A session without versions keeps the original statement.
		_, err = tx.ExecContext(ctx, `INSERT INTO hub_branches(id,session,route,data) VALUES(?,?,?,?)`, value.ID, value.SessionID, branchRoute(value.Target), data)
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_branches(id,session,route,data,line) VALUES(?,?,?,?,?)`, value.ID, value.SessionID, branchRoute(value.Target), data, line)
	return err
}
func (s *Store) ListSessions(ctx context.Context, limit int) (providerResult0 []hub.Session, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	if limit < 1 || limit > 256 {
		return nil, errors.New("session page must be 1..256")
	}
	rows, err := s.list.QueryContext(ctx, `SELECT data FROM hub_sessions WHERE `+notDeletedFilter+` ORDER BY updated DESC,id LIMIT ?`, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var result []hub.Session
	for rows.Next() {
		var data []byte
		var item hub.Session
		if err := rows.Scan(&data); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &item) != nil {
			return nil, errors.New("product session is corrupt")
		}
		result = append(result, item)
	}
	return result, rows.Err()
}
func detail(ctx context.Context, tx *sql.Tx, id string) (hub.Detail, error) {
	return detailOf(ctx, tx, id, liveSessionQuery)
}
func detailOf(ctx context.Context, tx *sql.Tx, id, sessionQuery string) (hub.Detail, error) {
	var result hub.Detail
	session, err := read[hub.Session](ctx, tx, sessionQuery, id)
	if err != nil {
		return result, err
	}
	result.Session = session
	rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_branches WHERE session=?`+lineFilter(session)+` ORDER BY id LIMIT 65`, lineArgs(session, id)...)
	if err != nil {
		return result, err
	}
	for rows.Next() {
		var data []byte
		var branch hub.Branch
		if err = rows.Scan(&data); err != nil {
			rows.Close()
			return result, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &branch) != nil {
			rows.Close()
			return result, errors.New("branch is corrupt")
		}
		result.Branches = append(result.Branches, branch)
	}
	err = rows.Err()
	rows.Close()
	if err != nil {
		return result, err
	}
	if len(result.Branches) > 64 {
		return result, errors.New("Hub branch budget exceeded")
	}
	policy, err := read[[]hub.Target](ctx, tx, `SELECT data FROM hub_fallback WHERE session=?`, id)
	if err != nil && !errors.Is(err, sql.ErrNoRows) {
		return result, err
	}
	result.Fallback = policy
	return result, nil
}
func (s *Store) Detail(ctx context.Context, id string) (providerResult0 hub.Detail, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var result hub.Detail
	err := s.transaction(ctx, func(tx *sql.Tx) error { var err error; result, err = detail(ctx, tx, id); return err })
	return result, err
}

// DetailIncludingDeleted is the internal/admin read that still sees a
// soft-deleted session. Product entries use Detail, which answers NotFound.
func (s *Store) DetailIncludingDeleted(ctx context.Context, id string) (result hub.Detail, err error) {
	defer func() { err = normalizeStoreError(err) }()
	err = s.transaction(ctx, func(tx *sql.Tx) error { var e error; result, e = detailOf(ctx, tx, id, rawSessionQuery); return e })
	return result, err
}

// branchRoute is the stored branch identity: one branch per (session, engine).
// Rows written by older builds hold a full route key here and are found by
// the engine recorded in their data instead, so they keep working unchanged.
func branchRoute(target hub.Target) string { return "engine:" + target.EngineID }

// selectBranch finds the session's branch for target's engine, preferring the
// active one, or creates it. A model/connection change inside one engine keeps
// the branch and its native conversation; only the engine selects a branch.
func selectBranch(ctx context.Context, tx *sql.Tx, session hub.Session, target hub.Target) (hub.Branch, bool, error) {
	branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE session=?`+lineFilter(session)+` AND json_extract(data,'$.target.engine_id')=? ORDER BY (id=?) DESC,rowid DESC LIMIT 1`, append(lineArgs(session, session.ID), target.EngineID, session.ActiveBranchID)...)
	if err == nil {
		return branch, false, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return branch, false, err
	}
	var count, total int
	if err = tx.QueryRowContext(ctx, `SELECT count(*),(SELECT count(*) FROM hub_branches WHERE session=?) FROM hub_branches WHERE session=?`+lineFilter(session), append([]any{session.ID}, lineArgs(session, session.ID)...)...).Scan(&count, &total); err != nil {
		return branch, false, err
	}
	if count >= 64 || total >= hub.MaxBranchesPerSession {
		return branch, false, errors.New("maximum 64 branches per version of a session")
	}
	id, err := hub.NewID("branch-")
	if err != nil {
		return branch, false, err
	}
	branch = hub.Branch{ID: id, SessionID: session.ID, Target: target}
	return branch, true, insertLineBranch(ctx, tx, branch, session.ActiveLineID)
}

// attemptBranch selects the branch one fallback attempt runs on without
// changing the session's active branch or the branch's primary route. The
// returned copy carries the fallback target for this attempt only.
func attemptBranch(ctx context.Context, tx *sql.Tx, session hub.Session, target hub.Target) (hub.Branch, error) {
	branch, created, err := selectBranch(ctx, tx, session, target)
	if err != nil {
		return branch, err
	}
	if created {
		branch.Parked = true
		if err = writeBranch(ctx, tx, branch); err != nil {
			return branch, err
		}
	}
	branch.Target = target
	return branch, nil
}
func activate(ctx context.Context, tx *sql.Tx, session *hub.Session, target hub.Target) (hub.Branch, error) {
	branch, _, err := selectBranch(ctx, tx, *session, target)
	if err != nil {
		return branch, err
	}
	old, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, session.ID)
	if err != nil {
		return branch, err
	}
	if old.ID != branch.ID {
		old.Parked = true
		if err = writeBranch(ctx, tx, old); err != nil {
			return branch, err
		}
	}
	branch.Parked = false
	branch.Target = target
	if err = writeBranch(ctx, tx, branch); err != nil {
		return branch, err
	}
	session.ActiveBranchID = branch.ID
	session.UpdatedAt = now()
	return branch, writeSession(ctx, tx, *session)
}
func (s *Store) Switch(ctx context.Context, id string, target hub.Target, reason string) (providerResult0 hub.Branch, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var result hub.Branch
	if err := target.Validate(); err != nil {
		return result, err
	}
	if strings.TrimSpace(reason) == "" || len(reason) > 1024 {
		return result, errors.New("switch requires a bounded reason")
	}
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("stop or finish the active Turn before switching")
		}
		previous := session.ActiveBranchID
		before, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, previous, id)
		if err != nil {
			return err
		}
		result, err = activate(ctx, tx, &session, target)
		if err != nil {
			return err
		}
		// Same branch and same route: nothing changed. Same branch with another
		// model of that engine still records the switch; its native session continues.
		if previous == result.ID && hub.RouteKey(before.Target) == hub.RouteKey(target) {
			return nil
		}
		return timeline(ctx, tx, id, "", "", hub.TimelineEvent{Kind: "branch_switch", Text: reason, FromBranchID: previous, ToBranchID: result.ID, Target: &target})
	})
	return result, err
}
func (s *Store) SetFallback(ctx context.Context, id string, targets []hub.Target) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	if len(targets) > 7 {
		return errors.New("at most 7 fallback targets")
	}
	seen := make(map[string]bool)
	for _, target := range targets {
		if err := target.Validate(); err != nil {
			return err
		}
		key := hub.RouteKey(target)
		if seen[key] {
			return errors.New("duplicate fallback route")
		}
		seen[key] = true
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("fallback policy is immutable during a Turn")
		}
		data, err := body(targets)
		if err != nil {
			return err
		}
		_, err = tx.ExecContext(ctx, `INSERT INTO hub_fallback VALUES(?,?) ON CONFLICT(session) DO UPDATE SET data=excluded.data`, id, data)
		return err
	})
}
func beginAttempt(ctx context.Context, tx *sql.Tx, session hub.Session, branch hub.Branch, turn *hub.Turn) (hub.Attempt, error) {
	if err := companionExecutionAllowed(ctx, tx, session.ID); err != nil {
		return hub.Attempt{}, err
	}
	if err := checkPlanTarget(ctx, tx, session.ID, branch.Target); err != nil {
		return hub.Attempt{}, err
	}
	id, err := hub.NewID("attempt-")
	if err != nil {
		return hub.Attempt{}, err
	}
	attempt := hub.Attempt{ID: id, SessionID: session.ID, TurnID: turn.ID, BranchID: branch.ID, Target: branch.Target, NativeSessionID: branch.NativeSessionID, Status: hub.Running, CreatedAt: now()}
	attempt.ModelOptions, err = captureModelOptions(ctx, tx, session, branch, *turn)
	if err != nil {
		return hub.Attempt{}, err
	}
	data, err := body(attempt)
	if err != nil {
		return attempt, err
	}
	if _, err = tx.ExecContext(ctx, `INSERT INTO hub_attempts VALUES(?,?,?,?,?)`, id, session.ID, turn.ID, attempt.Status, data); err != nil {
		return attempt, err
	}
	turn.ActiveAttemptID = id
	return attempt, writeTurn(ctx, tx, *turn)
}
func handoff(ctx context.Context, tx *sql.Tx, session hub.Session, branch hub.Branch) (*hub.Handoff, error) {
	visible, visibleArgs, err := versionPredicate(ctx, tx, session, "ht.seq", true)
	if err != nil {
		return nil, err
	}
	filter := ""
	if visible != "" {
		filter = " AND " + visible
	}
	rows, err := tx.QueryContext(ctx, `SELECT ht.data,json_extract(ha.data,'$.branch_id'),coalesce((SELECT min(seq)-1 FROM hub_events he WHERE he.session=ht.session AND he.turn=ht.id),0) FROM hub_turns ht JOIN hub_attempts ha ON ha.id=json_extract(ht.data,'$.active_attempt_id') WHERE ht.session=? AND ht.seq>? AND ht.status!=?`+filter+` ORDER BY ht.seq DESC LIMIT 33`, append([]any{session.ID, branch.Watermark, hub.Running}, visibleArgs...)...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var entries []hub.HandoffEntry
	var through uint64
	truncated := false
	for rows.Next() {
		var data []byte
		var turn hub.Turn
		var sourceBranch string
		var historyAfter uint64
		if err := rows.Scan(&data, &sourceBranch, &historyAfter); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &turn) != nil {
			return nil, errors.New("handoff source is corrupt")
		}
		if len(entries) >= 32 {
			truncated = true
			break
		}
		prompt := turn.Prompt
		if len(prompt) > 256 {
			prompt = prompt[:256]
			for !utf8.ValidString(prompt) {
				prompt = prompt[:len(prompt)-1]
			}
			prompt += " [excerpt; retrieve complete Turn]"
		}
		entries = append(entries, hub.HandoffEntry{TurnID: turn.ID, Prompt: prompt, Status: turn.Status, BranchID: sourceBranch, HistoryAfter: historyAfter})
		if turn.Sequence > through {
			through = turn.Sequence
		}
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	if err := rows.Close(); err != nil {
		return nil, err
	}
	team, err := teamSnapshot(ctx, tx, session.ID)
	if err != nil {
		return nil, err
	}
	if len(entries) == 0 && team == nil {
		return nil, nil
	}
	for left, right := 0, len(entries)-1; left < right; left, right = left+1, right-1 {
		entries[left], entries[right] = entries[right], entries[left]
	}
	id, err := hub.NewID("handoff-")
	if err != nil {
		return nil, err
	}
	value := &hub.Handoff{ID: id, SessionID: session.ID, TargetBranchID: branch.ID, Workspace: session.Workspace, Through: through, Entries: entries, Team: team, Processing: "Bounded product Turn/Team index only. No hidden reasoning, original files, MCP catalogs or native transcripts delivered. Known credential fields/markers are redacted; this is not arbitrary-secret detection."}
	if len(entries) > 0 {
		value.SourceBranchID = entries[0].BranchID
	}
	for _, entry := range entries {
		if entry.BranchID != value.SourceBranchID {
			value.SourceBranchID = ""
			break
		}
	}
	if truncated {
		value.Processing += " Older unseen Turns omitted from this bounded index; hub_history must be used to retrieve relevant earlier evidence."
	}
	// Preserve the exact Team roster/task indices while making older Turn
	// excerpts explicitly retrievable instead of expanding the model payload.
	for {
		if _, err = hub.EncodeHandoff(value); err == nil {
			break
		}
		if len(value.Entries) == 0 {
			return nil, err
		}
		value.Entries = value.Entries[1:]
		if !strings.Contains(value.Processing, "Additional older Turn excerpts omitted") {
			value.Processing += " Additional older Turn excerpts omitted to keep the Team handoff within 16 KiB; retrieve original evidence explicitly."
		}
	}
	value.SourceBranchID = ""
	if len(value.Entries) > 0 {
		value.SourceBranchID = value.Entries[0].BranchID
		for _, entry := range value.Entries {
			if entry.BranchID != value.SourceBranchID {
				value.SourceBranchID = ""
				break
			}
		}
	}
	return protectHandoff(value)
}
func beginTurn(ctx context.Context, tx *sql.Tx, id, prompt string) (hub.Claim, error) {
	return beginComposerTurn(ctx, tx, id, hub.ComposerTurnInput{Prompt: prompt, UserText: prompt})
}

func beginComposerTurn(ctx context.Context, tx *sql.Tx, id string, input hub.ComposerTurnInput) (hub.Claim, error) {
	return beginComposerTurnAs(ctx, tx, id, input, nil)
}

// beginComposerTurnAs admits a Turn on the active branch. A non-nil route runs
// this Turn's attempt on that exact route (an automation's frozen approval)
// instead of the branch's current manual route; the branch itself is unchanged.
func beginComposerTurnAs(ctx context.Context, tx *sql.Tx, id string, input hub.ComposerTurnInput, route *hub.Target) (hub.Claim, error) {
	prompt := input.Prompt
	if err := engine.ValidateInputFiles(input.Files); err != nil {
		return hub.Claim{}, err
	}
	if err := engine.ValidateInputSkills(input.Skills); err != nil {
		return hub.Claim{}, err
	}
	if input.Origin != "" && input.Origin != hub.TurnOriginContinue {
		return hub.Claim{}, errors.New("unknown Turn origin")
	}
	if len(input.UserText) > 64<<10 || !utf8.ValidString(input.UserText) {
		return hub.Claim{}, errors.New("user composer text exceeds its UTF-8 budget")
	}

	var claim hub.Claim
	if strings.TrimSpace(prompt) == "" || len(prompt) > 64<<10 || !utf8.ValidString(prompt) {
		return claim, errors.New("prompt must be valid text of 1..64 KiB")
	}
	err := func() error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
		if err != nil {
			return err
		}
		if session.DeletedAt != 0 {
			return hub.ErrNotFound
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("Hub session already claimed")
		}
		if input.Origin == hub.TurnOriginContinue {
			// Decided in the admission transaction, so a Stop landing or a new
			// Turn starting between the caller's read and now cannot slip by.
			latest, latestErr := currentTurn(ctx, tx, id)
			if errors.Is(latestErr, sql.ErrNoRows) || latestErr == nil && latest.Status != engine.Cancelled {
				return hub.ErrNothingToContinue
			}
			if latestErr != nil {
				return latestErr
			}
		}
		if err = checkWorkerWorkspaceAdmission(ctx, tx, id); err != nil {
			return err
		}
		var newLine *pendingLine
		if input.Line != nil {
			if route != nil {
				return errors.New("a scheduled Turn cannot start a version line")
			}
			if newLine, err = forkLine(ctx, tx, &session, *input.Line); err != nil {
				return err
			}
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
		if err != nil {
			return err
		}
		if route != nil {
			branch.Target = *route
		}
		h, err := handoff(ctx, tx, session, branch)
		if err != nil {
			return err
		}
		usedIndex := false
		if h == nil {
			if h, err = indexHandoff(session, branch); err != nil {
				return err
			}
			usedIndex = h != nil
		}
		turnID, err := hub.NewID("turn-")
		if err != nil {
			return err
		}
		var sequence uint64
		if err = tx.QueryRowContext(ctx, `SELECT COALESCE(max(seq),0)+1 FROM hub_turns WHERE session=?`, id).Scan(&sequence); err != nil {
			return err
		}
		turn := hub.Turn{Origin: input.Origin, ID: turnID, SessionID: id, Sequence: sequence, Prompt: prompt, InputFiles: engine.CloneInputFiles(input.Files), InputSkills: engine.CloneInputSkills(input.Skills), Status: hub.Running, CreatedAt: now()}
		data, err := body(turn)
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_turns VALUES(?,?,?,?,?)`, turnID, id, sequence, turn.Status, data); err != nil {
			return err
		}
		attempt, err := beginAttempt(ctx, tx, session, branch, &turn)
		if err != nil {
			return err
		}
		session.Status = hub.Running
		session.UpdatedAt = now()
		if err = writeSession(ctx, tx, session); err != nil {
			return err
		}
		if err = finishLine(ctx, tx, newLine, turn); err != nil {
			return err
		}
		if input.QueueID != "" {
			// The queued input becomes this Turn in one transaction.
			gone, delErr := tx.ExecContext(ctx, `DELETE FROM hub_pending_inputs WHERE id=? AND session=? AND state IN (?,?)`, input.QueueID, id, hub.PendingQueued, hub.PendingUncertain)
			if delErr != nil {
				return delErr
			}
			if n, _ := gone.RowsAffected(); n != 1 {
				return hub.ErrPendingGone
			}
		}
		claim = hub.Claim{Session: session, Branch: branch, Turn: turn, Attempt: attempt, Handoff: h}
		input, err := body(hub.HistoryEntry{Origin: input.Origin, Input: input.UserText, InputFiles: engine.CloneInputFiles(input.Files), InputSkills: engine.CloneInputSkills(input.Skills), SessionID: id, TurnID: turn.ID, AttemptID: attempt.ID, Time: now()})
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, id, turn.ID, attempt.ID, input); err != nil {
			return err
		}
		if err = saveHandoff(ctx, tx, id, turn.ID, attempt.ID, h); err != nil {
			return err
		}
		if usedIndex {
			if err = timeline(ctx, tx, id, turn.ID, attempt.ID, hub.TimelineEvent{Kind: "fork_degraded", Text: "此引擎不能原生分叉，模型只看到摘要"}); err != nil {
				return err
			}
		}
		if lineInput := claim.Turn; newLine != nil && newLine.degraded {
			// An engine without native fork starts the new version with the
			// Turn index only; say so where the reader can see it.
			return timeline(ctx, tx, id, lineInput.ID, attempt.ID, hub.TimelineEvent{Kind: "fork_degraded", Text: "此引擎不能原生分叉，模型只看到摘要"})
		}
		return nil

	}()
	return claim, err
}
func (s *Store) BeginTurn(ctx context.Context, id, prompt string) (providerResult0 hub.Claim, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	return s.BeginTurnOnBranch(ctx, id, "", prompt)
}
func (s *Store) BeginTurnOnBranch(ctx context.Context, id, expectedBranch, prompt string) (providerResult0 hub.Claim, providerResult1 error) {
	return s.BeginComposerTurn(ctx, id, expectedBranch, hub.ComposerTurnInput{Prompt: prompt, UserText: prompt})
}
func (s *Store) BeginComposerTurn(ctx context.Context, id, expectedBranch string, input hub.ComposerTurnInput) (providerResult0 hub.Claim, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var claim hub.Claim
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if expectedBranch != "" {
			current, err := read[hub.Session](ctx, tx, liveSessionQuery, id)
			if err != nil {
				return err
			}
			if current.ActiveBranchID != expectedBranch {
				return errors.New("product routing changed before Turn admission; draft references were not sent")
			}
		}
		var worker int
		if err := tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_workers WHERE session=?`, id).Scan(&worker); err != nil {
			return err
		}
		if worker != 0 {
			return errors.New("Worker session must consume its durable task queue, not bypass it with a plain Turn")
		}
		var err error
		claim, err = beginComposerTurn(ctx, tx, id, input)
		return err
	})
	return claim, err
}
func (s *Store) BeginFallback(ctx context.Context, previous string, target hub.Target) (providerResult0 hub.Claim, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var claim hub.Claim
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		old, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, previous)
		if err != nil {
			return err
		}
		if old.Status != engine.Failed || old.SideEffectsPossible || (old.Failure != engine.FailureStartup && old.Failure != engine.FailureRateLimit) {
			return errors.New("attempt is not safe for automatic fallback")
		}
		turn, err := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE id=? AND session=?`, old.TurnID, old.SessionID)
		if err != nil {
			return err
		}
		if turn.Status != hub.Running || turn.ActiveAttemptID != old.ID {
			return errors.New("fallback claim is stale")
		}
		if turn.AutomationID != "" {
			return errors.New("scheduled Turns cannot automatically change their approved account/billing route")
		}
		detail, err := detail(ctx, tx, old.SessionID)
		if err != nil {
			return err
		}
		authorized := false
		for _, approved := range detail.Fallback {
			if approved.AutomaticAuthorized && hub.RouteKey(approved) == hub.RouteKey(target) {
				authorized = true
				target = approved
				break
			}
		}
		if !authorized {
			return errors.New("fallback account/billing route not explicitly authorized")
		}
		branch, err := attemptBranch(ctx, tx, detail.Session, target)
		if err != nil {
			return err
		}
		h, err := handoff(ctx, tx, detail.Session, branch)
		if err != nil {
			return err
		}
		attempt, err := beginAttempt(ctx, tx, detail.Session, branch, &turn)
		if err != nil {
			return err
		}
		claim = hub.Claim{Session: detail.Session, Branch: branch, Turn: turn, Attempt: attempt, Handoff: h}
		if err = saveHandoff(ctx, tx, old.SessionID, turn.ID, attempt.ID, h); err != nil {
			return err
		}
		return timeline(ctx, tx, old.SessionID, turn.ID, attempt.ID, hub.TimelineEvent{Kind: "automatic_fallback", Text: "Startup/rate-limit fallback with explicit account/billing approval and no possible side effects", FromBranchID: old.BranchID, ToBranchID: branch.ID, Target: &target})
	})
	return claim, err
}
func (s *Store) Record(ctx context.Context, attemptID string, event engine.Event, sideEffects bool) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	waiting := event.Kind == engine.Completed && event.Outcome == engine.Waiting
	if event.Wait != nil && !waiting || waiting && (event.Wait == nil || event.Wait.Validate() != nil) {
		return errors.New("waiting completion requires an exclusive bounded checkpoint receipt")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		attempt, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, attemptID)
		if err != nil {
			return err
		}
		turn, err := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE id=? AND session=?`, attempt.TurnID, attempt.SessionID)
		if err != nil {
			return err
		}
		if attempt.Status != hub.Running || turn.Status != hub.Running || turn.ActiveAttemptID != attemptID {
			return errors.New("event belongs to a stale attempt")
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, attempt.BranchID, attempt.SessionID)
		if err != nil {
			return err
		}
		if event.Kind == engine.SessionStarted || event.Kind == engine.ExecutionActivity && event.SessionID != "" {
			if attempt.NativeSessionID != "" && attempt.NativeSessionID != event.SessionID || branch.NativeSessionID != "" && branch.NativeSessionID != event.SessionID {
				return errors.New("engine changed resumed native identity")
			}
			attempt.NativeSessionID = event.SessionID
			branch.NativeSessionID = event.SessionID
			// The native conversation exists now; the fork intent and the
			// Turn index have done their work.
			branch.Fork, branch.Index = nil, nil
		}
		if event.Kind == engine.InputDelivered {
			if err = deliverPending(ctx, tx, attempt.SessionID, event.ID); err != nil {
				return err
			}
		}
		if event.Reference != nil && event.Reference.SessionID != attempt.NativeSessionID {
			return errors.New("event reference belongs to another native session")
		}
		attempt.SideEffectsPossible = attempt.SideEffectsPossible || sideEffects
		if event.Kind == engine.Error && !event.Recoverable {
			attempt.Failure = event.Failure
			attempt.Error = event.Text
		}
		if event.Kind == engine.Completed {
			if event.Outcome == engine.Waiting && (attempt.NativeSessionID == "" || attempt.Error != "") {
				return errors.New("waiting completion requires a known native session without a fatal error")
			}
			attempt.Status = event.Outcome
			attempt.Wait = event.Wait
			attempt.EndedAt = now()
			if event.Outcome == engine.Succeeded {
				branch.Watermark = turn.Sequence
			}
		}
		entry := hub.HistoryEntry{SessionID: attempt.SessionID, TurnID: attempt.TurnID, AttemptID: attempt.ID, Event: event, Time: now()}
		data, err := body(entry)
		if err != nil {
			return err
		}
		inserted, err := tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, attempt.SessionID, attempt.TurnID, attempt.ID, data)
		if err != nil {
			return err
		}
		if deltas := mergedDeltaKind(event); deltas != "" {
			seq, err := inserted.LastInsertId()
			if err != nil {
				return err
			}
			// Deltas are derived display drafts. Once the final text (or its
			// reference) is committed they are redundant; delete them in this
			// same transaction. hub_events_turn serves the turn/seq range.
			if _, err = tx.ExecContext(ctx, `DELETE FROM hub_events WHERE turn=? AND seq<? AND attempt=? AND json_extract(data,'$.event.kind')=? AND json_extract(data,'$.event.id')=?`, attempt.TurnID, seq, attempt.ID, string(deltas), event.ID); err != nil {
				return err
			}
		}
		if err = writeBranch(ctx, tx, branch); err != nil {
			return err
		}
		return writeAttempt(ctx, tx, attempt)
	})
}

// mergedDeltaKind names the delta rows a final event supersedes. A truncated
// final event without a reference keeps them: they are the only full text.
func mergedDeltaKind(event engine.Event) engine.Kind {
	if event.ID == "" || event.Truncated && event.Reference == nil {
		return ""
	}
	switch {
	case event.Kind == engine.Message:
		return engine.MessageDelta
	case event.Kind == engine.ContentBlock && event.Name == "reasoning":
		return engine.ReasoningDelta
	}
	return ""
}
func (s *Store) FinishTurn(ctx context.Context, id string, status engine.Outcome) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	if status != engine.Succeeded && status != engine.Failed && status != engine.Cancelled && status != engine.Interrupted && status != engine.Waiting {
		return errors.New("Turn requires a terminal status")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		turn, err := read[hub.Turn](ctx, tx, `SELECT data FROM hub_turns WHERE id=?`, id)
		if err != nil {
			return err
		}
		if turn.Status != hub.Running {
			return errors.New("Turn already terminal")
		}
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, turn.SessionID)
		if err != nil {
			return err
		}
		attempt, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=? AND turn=?`, turn.ActiveAttemptID, id)
		if err != nil {
			return err
		}
		if status == engine.Waiting && (attempt.Status != engine.Waiting || attempt.Wait == nil || attempt.Wait.Validate() != nil) {
			return errors.New("waiting Turn requires actual checkpoint receipt")
		}
		if status == engine.Succeeded && attempt.Status != engine.Succeeded {
			return errors.New("cannot claim Turn success without successful actual attempt completion")
		}
		if attempt.Status != hub.Running && attempt.Status != status {
			if err = timeline(ctx, tx, turn.SessionID, turn.ID, attempt.ID, hub.TimelineEvent{Kind: "execution_owner_failure", Text: "Execution owner did not confirm the engine's terminal observation; product Turn ends " + string(status) + ". No automatic replay; inspect retained native evidence."}); err != nil {
				return err
			}
		}
		if attempt.Status == hub.Running {
			attempt.Status = status
			attempt.EndedAt = now()
			attempt.Error = "Attempt observation or persistence ended before terminal delivery"
			entry := hub.HistoryEntry{SessionID: attempt.SessionID, TurnID: attempt.TurnID, AttemptID: attempt.ID, Time: now(), Event: engine.Event{Kind: engine.Completed, Outcome: status, Text: attempt.Error}}
			data, err := body(entry)
			if err != nil {
				return err
			}
			if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, attempt.SessionID, attempt.TurnID, attempt.ID, data); err != nil {
				return err
			}
			if err = writeAttempt(ctx, tx, attempt); err != nil {
				return err
			}
		}
		turn.Status = status
		turn.EndedAt = now()
		if turn.Status == engine.Waiting {
			turn.EndedAt = 0
		}
		session.Status = status
		session.UpdatedAt = now()
		if err = writeTurn(ctx, tx, turn); err != nil {
			return err
		}
		if err = requeueAllInterjections(ctx, tx, turn.SessionID); err != nil {
			return err
		}
		if err = writeSession(ctx, tx, session); err != nil {
			return err
		}
		if err = settleWorkerTask(ctx, tx, turn); err != nil {
			return err
		}
		return settleAutomation(ctx, tx, turn)
	})
}
func (s *Store) History(ctx context.Context, id string, after uint64, limit int) (providerResult0 []hub.HistoryEntry, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	if limit < 1 || limit > 128 {
		return nil, errors.New("history page must be 1..128")
	}
	session, err := read[hub.Session](ctx, s.db, liveSessionQuery, id)
	if err != nil {
		return nil, err
	}
	if line, ok, lineErr := activeLineOf(ctx, s.db, session); lineErr != nil {
		return nil, lineErr
	} else if ok {
		return s.historySegments(ctx, id, line, after, 0, limit, false)
	}
	rows, err := s.db.QueryContext(ctx, `SELECT e.seq,e.data,COALESCE(t.seq,0) FROM hub_events e LEFT JOIN hub_turns t ON t.id=e.turn WHERE e.session=? AND e.seq>? ORDER BY e.seq LIMIT ?`, id, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	return historyRows(rows)
}

// historyRows decodes history pages. TurnSequence comes from the joined Turn
// row at read time; events without a Turn (Team notices) keep 0.
func historyRows(rows *sql.Rows) ([]hub.HistoryEntry, error) {
	var result []hub.HistoryEntry
	for rows.Next() {
		var seq, turn uint64
		var data []byte
		var entry hub.HistoryEntry
		if err := rows.Scan(&seq, &data, &turn); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &entry) != nil {
			return nil, errors.New("history record is corrupt")
		}
		entry.Sequence = seq
		entry.TurnSequence = turn
		result = append(result, entry)
	}
	return result, rows.Err()
}
func (s *Store) HistoryBefore(ctx context.Context, id string, before uint64, limit int) (providerResult0 []hub.HistoryEntry, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	if limit < 1 || limit > 128 {
		return nil, errors.New("history page must be 1..128")
	}
	session, err := read[hub.Session](ctx, s.db, liveSessionQuery, id)
	if err != nil {
		return nil, err
	}
	if line, ok, lineErr := activeLineOf(ctx, s.db, session); lineErr != nil {
		return nil, lineErr
	} else if ok {
		return s.historySegments(ctx, id, line, 0, before, limit, true)
	}
	query := `SELECT e.seq,e.data,COALESCE(t.seq,0) FROM hub_events e LEFT JOIN hub_turns t ON t.id=e.turn WHERE e.session=?`
	args := []any{id}
	if before != 0 {
		query += ` AND e.seq<?`
		args = append(args, before)
	}
	query += ` ORDER BY e.seq DESC LIMIT ?`
	args = append(args, limit)
	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	result, err := historyRows(rows)
	if err != nil {
		return nil, err
	}
	for i, j := 0, len(result)-1; i < j; i, j = i+1, j-1 {
		result[i], result[j] = result[j], result[i]
	}
	return result, nil
}

// BindNative records an already-created, owner-scoped empty native session for
// uploads. It never executes a model, and cannot rewrite a running/owned branch.
func (s *Store) BindNative(ctx context.Context, sessionID, branchID, nativeID string) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	if nativeID == "" || len(nativeID) > 256 {
		return errors.New("bounded native identity required")
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		session, err := read[hub.Session](ctx, tx, liveSessionQuery, sessionID)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) || session.ActiveBranchID != branchID {
			return errors.New("branch is running or no longer active")
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, branchID, sessionID)
		if err != nil {
			return err
		}
		if branch.NativeSessionID != "" && branch.NativeSessionID != nativeID {
			return errors.New("native branch binding is immutable")
		}
		branch.NativeSessionID = nativeID
		return writeBranch(ctx, tx, branch)
	})
}
func (s *Store) Recover(ctx context.Context) (providerResult0 error) { return s.recover(ctx, "") }

// RecoverSession relies on single-process ownership: Open takes an exclusive
// database lock (private.LockDatabase; TestRecoveryAndExclusiveDatabaseOwnership),
// so desktop and server never share one product database and no other process
// can own the Turn. Within this process the caller guarantees no live owner.
// RecoverSession interrupts one session's running Turn whose execution owner
// is gone (for example after FinishTurn failed), so the user can continue
// without restarting. The caller guarantees no live owner exists.
func (s *Store) RecoverSession(ctx context.Context, id string) error {
	if id == "" {
		return errors.New("session identity required")
	}
	return s.recover(ctx, id)
}
func (s *Store) recover(ctx context.Context, only string) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_turns WHERE status=? AND (?='' OR session=?)`, hub.Running, only, only)
		if err != nil {
			return err
		}
		var turns []hub.Turn
		for rows.Next() {
			var data []byte
			var turn hub.Turn
			if err = rows.Scan(&data); err != nil {
				rows.Close()
				return err
			}
			if json.Unmarshal(data, &turn) != nil {
				rows.Close()
				return errors.New("interrupted Turn metadata corrupt")
			}
			turns = append(turns, turn)
		}
		err = rows.Err()
		rows.Close()
		if err != nil {
			return err
		}
		for _, turn := range turns {
			attempt, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, turn.ActiveAttemptID)
			if err != nil {
				return err
			}
			if attempt.Status == hub.Running {
				attempt.Status = engine.Interrupted
				attempt.EndedAt = now()
				attempt.Error = "Application exited without a committed terminal outcome; execution is unknown and is not replayed"
				if err = writeAttempt(ctx, tx, attempt); err != nil {
					return err
				}
				entry := hub.HistoryEntry{SessionID: attempt.SessionID, TurnID: attempt.TurnID, AttemptID: attempt.ID, Time: now(), Event: engine.Event{Kind: engine.Completed, Outcome: engine.Interrupted, Text: attempt.Error}}
				data, err := body(entry)
				if err != nil {
					return err
				}
				if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, attempt.SessionID, attempt.TurnID, attempt.ID, data); err != nil {
					return err
				}
			}
			// A terminal attempt committed before a crash remains authoritative;
			// recovery finishes its product edge, never overwrites known success.
			if attempt.Status == engine.Waiting && (attempt.Wait == nil || attempt.Wait.Validate() != nil || attempt.NativeSessionID == "" || attempt.Error != "") {
				return errors.New("interrupted product edge has an invalid waiting receipt")
			}
			turn.Status = attempt.Status
			turn.EndedAt = now()
			if turn.Status == engine.Waiting {
				turn.EndedAt = 0
			}
			if err = writeTurn(ctx, tx, turn); err != nil {
				return err
			}
			session, err := read[hub.Session](ctx, tx, liveSessionQuery, turn.SessionID)
			if err != nil {
				return err
			}
			session.Status = turn.Status
			session.UpdatedAt = now()
			if err = writeSession(ctx, tx, session); err != nil {
				return err
			}
			if err = settleWorkerTask(ctx, tx, turn); err != nil {
				return err
			}
		}
		if only != "" {
			return nil
		}
		if err := recoverPending(ctx, tx); err != nil {
			return err
		}
		return recoverAutomations(ctx, tx)
	})
}

// RecordRoute persists which connection revision and endpoint an attempt
// actually used, once per change (audit), and a visible notice when the
// endpoint moved since the branch's previous attempt.
func (s *Store) RecordRoute(ctx context.Context, attemptID string, facts hub.RouteFacts) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	if facts.Revision == 0 {
		return nil
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		attempt, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, attemptID)
		if err != nil {
			return err
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, attempt.BranchID, attempt.SessionID)
		if err != nil {
			return err
		}
		key := hub.RouteKey(facts.Target)
		prev, known := branch.RoutesSeen[key]
		if !known && hub.RouteKey(branch.Target) == key && branch.Target.BillingRoute != "" {
			// Data saved before per-route memory: the branch's own route
			// baseline is the endpoint it was last configured with.
			prev, known = hub.RouteSeen{Endpoint: branch.Target.BillingRoute}, true
		}
		if known && prev.Revision == facts.Revision && prev.Endpoint == facts.Endpoint {
			return nil
		}
		target := facts.Target
		if err = timeline(ctx, tx, attempt.SessionID, attempt.TurnID, attempt.ID, hub.TimelineEvent{Kind: "route_resolved", Text: fmt.Sprintf("connection revision %d", facts.Revision), Target: &target, Revision: facts.Revision, Endpoint: facts.Endpoint}); err != nil {
			return err
		}
		if known && prev.Endpoint != "" && prev.Endpoint != facts.Endpoint {
			host := facts.Endpoint
			if u, perr := url.Parse(facts.Endpoint); perr == nil && u.Hostname() != "" {
				host = u.Hostname()
			}
			if err = timeline(ctx, tx, attempt.SessionID, attempt.TurnID, attempt.ID, hub.TimelineEvent{Kind: "route_endpoint_changed", Text: "连接地址已更新为 " + host}); err != nil {
				return err
			}
		}
		if branch.RoutesSeen == nil {
			branch.RoutesSeen = map[string]hub.RouteSeen{}
		}
		if _, exists := branch.RoutesSeen[key]; !exists && len(branch.RoutesSeen) >= 16 {
			for old := range branch.RoutesSeen {
				delete(branch.RoutesSeen, old)
				break
			}
		}
		branch.RoutesSeen[key] = hub.RouteSeen{Revision: facts.Revision, Endpoint: facts.Endpoint}
		return writeBranch(ctx, tx, branch)
	})
}

// RecordFallbackSkip tells the user why an approved fallback route was not used.
func (s *Store) RecordFallbackSkip(ctx context.Context, attemptID string, target hub.Target, reason string) (err error) {
	defer func() { err = normalizeStoreError(err) }()
	if len(reason) > 1024 {
		reason = reason[:1024]
		for !utf8.ValidString(reason) {
			reason = reason[:len(reason)-1]
		}
	}
	return s.transaction(ctx, func(tx *sql.Tx) error {
		attempt, err := read[hub.Attempt](ctx, tx, `SELECT data FROM hub_attempts WHERE id=?`, attemptID)
		if err != nil {
			return err
		}
		return timeline(ctx, tx, attempt.SessionID, attempt.TurnID, attempt.ID, hub.TimelineEvent{Kind: "fallback_unavailable", Text: reason, Target: &target})
	})
}

var _ hub.Store = (*Store)(nil)
