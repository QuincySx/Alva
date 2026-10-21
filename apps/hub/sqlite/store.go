// INPUT: context, database/sql, encoding/json, hub, storage/sqlite and system/privatefs
// OUTPUT: Store implementing isolated durable Hub product facts and exclusive recovery
// POS: [L7 Hub product] Product SQLite authority separate from native SessionRepository execution records
package sqlite

import (
	"context"
	"database/sql"
	"encoding/json"
	"errors"
	"path/filepath"
	"strings"
	"time"
	"unicode/utf8"

	"github.com/QuincySx/Alva/apps/hub"
	"github.com/QuincySx/Alva/integration/engine"
	private "github.com/QuincySx/Alva/platform/storage/sqlite"
)

type Store struct {
	db   *sql.DB
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
	if version != 0 && version != 1 {
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
	if version == 0 {
		if _, err = initialization.ExecContext(ctx, `PRAGMA user_version=1`); err != nil {
			initialization.Rollback()
			s.Close()
			return nil, err
		}
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
	return s, nil
}
func (s *Store) Close() (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return errors.Join(s.db.Close(), s.lock.Close())
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
func (s *Store) CreateSession(ctx context.Context, title, workspace string, target hub.Target) (providerResult0 hub.Session, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var value hub.Session
	if err := target.Validate(); err != nil {
		return value, err
	}
	if strings.TrimSpace(title) == "" || len(title) > 256 || !utf8.ValidString(title) || !filepath.IsAbs(workspace) {
		return value, errors.New("bounded title and absolute workspace required")
	}
	id, err := hub.NewID("hub-")
	if err != nil {
		return value, err
	}
	branchID, err := hub.NewID("branch-")
	if err != nil {
		return value, err
	}
	value = hub.Session{ID: id, Title: title, Workspace: workspace, ActiveBranchID: branchID, Status: engine.Succeeded, CreatedAt: now(), UpdatedAt: now()}
	branch := hub.Branch{ID: branchID, SessionID: id, Target: target}
	err = s.transaction(ctx, func(tx *sql.Tx) error {
		sessionData, err := body(value)
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_sessions VALUES(?,?,?,?)`, id, value.Status, value.UpdatedAt, sessionData); err != nil {
			return err
		}
		return insertBranch(ctx, tx, branch)
	})
	return value, err
}
func insertBranch(ctx context.Context, tx *sql.Tx, value hub.Branch) error {
	data, err := body(value)
	if err != nil {
		return err
	}
	_, err = tx.ExecContext(ctx, `INSERT INTO hub_branches VALUES(?,?,?,?)`, value.ID, value.SessionID, hub.RouteKey(value.Target), data)
	return err
}
func (s *Store) ListSessions(ctx context.Context, limit int) (providerResult0 []hub.Session, providerResult1 error) {
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	if limit < 1 || limit > 256 {
		return nil, errors.New("session page must be 1..256")
	}
	rows, err := s.db.QueryContext(ctx, `SELECT data FROM hub_sessions ORDER BY updated DESC,id LIMIT ?`, limit)
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
	var result hub.Detail
	session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
	if err != nil {
		return result, err
	}
	result.Session = session
	rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_branches WHERE session=? ORDER BY id LIMIT 65`, id)
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
func selectBranch(ctx context.Context, tx *sql.Tx, session hub.Session, target hub.Target) (hub.Branch, error) {
	branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE session=? AND route=?`, session.ID, hub.RouteKey(target))
	if err == nil {
		return branch, nil
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return branch, err
	}
	var count int
	if err = tx.QueryRowContext(ctx, `SELECT count(*) FROM hub_branches WHERE session=?`, session.ID).Scan(&count); err != nil {
		return branch, err
	}
	if count >= 64 {
		return branch, errors.New("maximum 64 branches per session")
	}
	id, err := hub.NewID("branch-")
	if err != nil {
		return branch, err
	}
	branch = hub.Branch{ID: id, SessionID: session.ID, Target: target}
	return branch, insertBranch(ctx, tx, branch)
}
func activate(ctx context.Context, tx *sql.Tx, session *hub.Session, target hub.Target) (hub.Branch, error) {
	branch, err := selectBranch(ctx, tx, *session, target)
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
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("stop or finish the active Turn before switching")
		}
		previous := session.ActiveBranchID
		result, err = activate(ctx, tx, &session, target)
		if err != nil {
			return err
		}
		if previous == result.ID {
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
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
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
	id, err := hub.NewID("attempt-")
	if err != nil {
		return hub.Attempt{}, err
	}
	attempt := hub.Attempt{ID: id, SessionID: session.ID, TurnID: turn.ID, BranchID: branch.ID, Target: branch.Target, NativeSessionID: branch.NativeSessionID, Status: hub.Running, CreatedAt: now()}
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
	rows, err := tx.QueryContext(ctx, `SELECT ht.data,json_extract(ha.data,'$.branch_id'),coalesce((SELECT min(seq)-1 FROM hub_events he WHERE he.session=ht.session AND he.turn=ht.id),0) FROM hub_turns ht JOIN hub_attempts ha ON ha.id=json_extract(ht.data,'$.active_attempt_id') WHERE ht.session=? AND ht.seq>? AND ht.status!=? ORDER BY ht.seq DESC LIMIT 33`, session.ID, branch.Watermark, hub.Running)
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
	var claim hub.Claim
	if strings.TrimSpace(prompt) == "" || len(prompt) > 64<<10 || !utf8.ValidString(prompt) {
		return claim, errors.New("prompt must be valid text of 1..64 KiB")
	}
	err := func() error {
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
		if err != nil {
			return err
		}
		if hub.BlocksInput(session.Status) {
			return errors.New("Hub session already claimed")
		}
		if err = checkWorkerWorkspaceAdmission(ctx, tx, id); err != nil {
			return err
		}
		branch, err := read[hub.Branch](ctx, tx, `SELECT data FROM hub_branches WHERE id=? AND session=?`, session.ActiveBranchID, id)
		if err != nil {
			return err
		}
		h, err := handoff(ctx, tx, session, branch)
		if err != nil {
			return err
		}
		turnID, err := hub.NewID("turn-")
		if err != nil {
			return err
		}
		var sequence uint64
		if err = tx.QueryRowContext(ctx, `SELECT COALESCE(max(seq),0)+1 FROM hub_turns WHERE session=?`, id).Scan(&sequence); err != nil {
			return err
		}
		turn := hub.Turn{ID: turnID, SessionID: id, Sequence: sequence, Prompt: prompt, Status: hub.Running, CreatedAt: now()}
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
		claim = hub.Claim{Session: session, Branch: branch, Turn: turn, Attempt: attempt, Handoff: h}
		input, err := body(hub.HistoryEntry{Input: prompt, SessionID: id, TurnID: turn.ID, AttemptID: attempt.ID, Time: now()})
		if err != nil {
			return err
		}
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, id, turn.ID, attempt.ID, input); err != nil {
			return err
		}
		return saveHandoff(ctx, tx, id, turn.ID, attempt.ID, h)

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
	defer func() {
		providerResult1 = normalizeStoreError(providerResult1)
	}()
	var claim hub.Claim
	err := s.transaction(ctx, func(tx *sql.Tx) error {
		if expectedBranch != "" {
			current, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, id)
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
		claim, err = beginTurn(ctx, tx, id, prompt)
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
		branch, err := activate(ctx, tx, &detail.Session, target)
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
		if _, err = tx.ExecContext(ctx, `INSERT INTO hub_events(session,turn,attempt,data) VALUES(?,?,?,?)`, attempt.SessionID, attempt.TurnID, attempt.ID, data); err != nil {
			return err
		}
		if err = writeBranch(ctx, tx, branch); err != nil {
			return err
		}
		return writeAttempt(ctx, tx, attempt)
	})
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
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, turn.SessionID)
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
	if _, err := read[hub.Session](ctx, s.db, `SELECT data FROM hub_sessions WHERE id=?`, id); err != nil {
		return nil, err
	}
	rows, err := s.db.QueryContext(ctx, `SELECT seq,data FROM hub_events WHERE session=? AND seq>? ORDER BY seq LIMIT ?`, id, after, limit)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var result []hub.HistoryEntry
	for rows.Next() {
		var seq uint64
		var data []byte
		var entry hub.HistoryEntry
		if err := rows.Scan(&seq, &data); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &entry) != nil {
			return nil, errors.New("history record is corrupt")
		}
		entry.Sequence = seq
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
	if _, err := read[hub.Session](ctx, s.db, `SELECT data FROM hub_sessions WHERE id=?`, id); err != nil {
		return nil, err
	}
	query := `SELECT seq,data FROM hub_events WHERE session=?`
	args := []any{id}
	if before != 0 {
		query += ` AND seq<?`
		args = append(args, before)
	}
	query += ` ORDER BY seq DESC LIMIT ?`
	args = append(args, limit)
	rows, err := s.db.QueryContext(ctx, query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	var result []hub.HistoryEntry
	for rows.Next() {
		var seq uint64
		var data []byte
		var entry hub.HistoryEntry
		if err := rows.Scan(&seq, &data); err != nil {
			return nil, err
		}
		if len(data) > 128<<10 || json.Unmarshal(data, &entry) != nil {
			return nil, errors.New("history record is corrupt")
		}
		entry.Sequence = seq
		result = append(result, entry)
	}
	for i, j := 0, len(result)-1; i < j; i, j = i+1, j-1 {
		result[i], result[j] = result[j], result[i]
	}
	return result, rows.Err()
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
		session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, sessionID)
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
func (s *Store) Recover(ctx context.Context) (providerResult0 error) {
	defer func() {
		providerResult0 = normalizeStoreError(providerResult0)
	}()
	return s.transaction(ctx, func(tx *sql.Tx) error {
		rows, err := tx.QueryContext(ctx, `SELECT data FROM hub_turns WHERE status=?`, hub.Running)
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
			session, err := read[hub.Session](ctx, tx, `SELECT data FROM hub_sessions WHERE id=?`, turn.SessionID)
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
		return recoverAutomations(ctx, tx)
	})
}

var _ hub.Store = (*Store)(nil)
