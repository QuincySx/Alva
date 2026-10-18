// INPUT: context, database/sql, crypto/rand, sync, storage/sqlite, kernel/abi.
// OUTPUT: Repository, Session, Open and owner-bound lifecycle/list/run-claim APIs.
// POS: [platform / durable storage] Single native execution authority independent of Hub product records and runtime construction.
package session

import (
	"context"
	"crypto/rand"
	"database/sql"
	"encoding/hex"
	"errors"
	"fmt"
	"regexp"
	"sync"
	"time"

	"github.com/QuincySx/Alva/platform/storage/sqlite"
)

var ErrNotFound = errors.New("native session or artifact is unavailable to this owner")
var ErrActive = errors.New("native session already has an active run")
var ErrClosed = errors.New("native repository is closed")
var opaqueID = regexp.MustCompile(`^[A-Za-z0-9][A-Za-z0-9._:-]{0,127}$`)

type Repository struct {
	mu     sync.Mutex
	db     *sql.DB
	lock   *sqlite.Lock
	closed bool
	active map[string]bool
}
type Session struct {
	repository *Repository
	owner, id  string
}
type SessionInfo struct {
	ID        string    `json:"id"`
	ParentID  string    `json:"parent_id,omitempty"`
	CreatedAt time.Time `json:"created_at"`
	HighWater uint64    `json:"high_water"`
}
type SessionPage struct {
	Sessions []SessionInfo `json:"sessions"`
	Next     string        `json:"next"`
	HasMore  bool          `json:"has_more"`
}

func Open(ctx context.Context, directory string) (*Repository, error) {
	lock, err := sqlite.LockDatabase(directory, "native.sqlite")
	if err != nil {
		return nil, err
	}
	db, err := sqlite.Open(ctx, directory, "native.sqlite")
	if err != nil {
		lock.Close()
		return nil, err
	}
	r := &Repository{db: db, lock: lock, active: make(map[string]bool)}
	if err = r.initialize(ctx); err == nil {
		// Auxiliary access path for the bounded model working view. Existing
		// new-Go repositories gain the index without changing facts or format.
		_, err = db.ExecContext(ctx, "CREATE INDEX IF NOT EXISTS conversation_tail ON records(session_id,seq DESC) WHERE kind='conversation'")
	}
	if err != nil {
		db.Close()
		lock.Close()
		return nil, err
	}
	return r, nil
}

func (r *Repository) initialize(ctx context.Context) error {
	var version int
	if err := r.db.QueryRowContext(ctx, "PRAGMA user_version").Scan(&version); err != nil {
		return err
	}
	if version != 0 && version != 1 && version != 2 {
		return errors.New("unsupported native database schema; no legacy import is performed")
	}
	if version == 2 {
		return nil
	}
	if version == 1 {
		// New-Go schema evolution only: this is an auxiliary causal index,
		// never an import/backfill of historical Rust facts or invented links.
		_, err := r.db.ExecContext(ctx, `BEGIN IMMEDIATE;
CREATE TABLE child_links(child_id TEXT PRIMARY KEY REFERENCES sessions(id),parent_id TEXT NOT NULL REFERENCES sessions(id),link_seq INTEGER NOT NULL,link_id TEXT NOT NULL,FOREIGN KEY(parent_id,link_seq) REFERENCES records(session_id,seq));
PRAGMA user_version=2;
COMMIT;`)
		return err
	}
	var tables int
	if err := r.db.QueryRowContext(ctx, "SELECT count(*) FROM sqlite_master WHERE type='table' AND name NOT LIKE 'sqlite_%'").Scan(&tables); err != nil {
		return err
	}
	if tables != 0 {
		return errors.New("unrecognized native database; refusing implicit conversion")
	}
	_, err := r.db.ExecContext(ctx, `BEGIN IMMEDIATE;
CREATE TABLE sessions(id TEXT PRIMARY KEY,owner TEXT NOT NULL,parent_id TEXT,created_ns INTEGER NOT NULL,next_seq INTEGER NOT NULL DEFAULT 1,CHECK(next_seq>0));
CREATE INDEX sessions_owner ON sessions(owner,id);
CREATE TABLE artifacts(id TEXT PRIMARY KEY,session_id TEXT NOT NULL REFERENCES sessions(id),kind TEXT NOT NULL,name TEXT NOT NULL,media_type TEXT NOT NULL,processing TEXT NOT NULL,sha256 TEXT NOT NULL,bytes INTEGER NOT NULL,is_text INTEGER NOT NULL,created_ns INTEGER NOT NULL,UNIQUE(session_id,kind,sha256,name,media_type,processing,is_text));
CREATE TABLE artifact_chunks(artifact_id TEXT NOT NULL REFERENCES artifacts(id),chunk_index INTEGER NOT NULL,data BLOB NOT NULL,sha256 TEXT NOT NULL,PRIMARY KEY(artifact_id,chunk_index));
CREATE TABLE records(session_id TEXT NOT NULL REFERENCES sessions(id),seq INTEGER NOT NULL,id TEXT NOT NULL,time_ns INTEGER NOT NULL,kind TEXT NOT NULL,namespace TEXT NOT NULL,type TEXT NOT NULL,schema_version INTEGER NOT NULL,parent_json TEXT,summary TEXT NOT NULL,artifact_id TEXT NOT NULL REFERENCES artifacts(id),request_id TEXT,run_id TEXT,call_id TEXT,status TEXT,PRIMARY KEY(session_id,seq),UNIQUE(session_id,id));
CREATE UNIQUE INDEX model_intent_identity ON records(session_id,request_id) WHERE kind='model_intent';
CREATE UNIQUE INDEX model_outcome_identity ON records(session_id,request_id) WHERE kind='model_outcome';
CREATE UNIQUE INDEX tool_intent_identity ON records(session_id,run_id,call_id) WHERE kind='tool_intent';
CREATE UNIQUE INDEX tool_outcome_identity ON records(session_id,run_id,call_id) WHERE kind='tool_outcome';
CREATE TABLE child_links(child_id TEXT PRIMARY KEY REFERENCES sessions(id),parent_id TEXT NOT NULL REFERENCES sessions(id),link_seq INTEGER NOT NULL,link_id TEXT NOT NULL,FOREIGN KEY(parent_id,link_seq) REFERENCES records(session_id,seq));
PRAGMA user_version=2;
COMMIT;`)
	return err
}

func (r *Repository) Close() error {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil
	}
	if len(r.active) > 0 {
		return errors.New("cannot close native repository while runs are active")
	}
	r.closed = true
	return errors.Join(r.db.Close(), r.lock.Close())
}

func validIdentity(value string) bool { return opaqueID.MatchString(value) }
func newID() (string, error) {
	var data [16]byte
	if _, err := rand.Read(data[:]); err != nil {
		return "", err
	}
	return hex.EncodeToString(data[:]), nil
}

func (r *Repository) Create(ctx context.Context, owner, id, parentID string) (*Session, error) {
	if !validIdentity(owner) || !validIdentity(id) || (parentID != "" && !validIdentity(parentID)) {
		return nil, errors.New("bounded opaque owner and session identities required")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	tx, err := r.db.BeginTx(ctx, nil)
	if err != nil {
		return nil, err
	}
	defer tx.Rollback()
	if parentID != "" {
		if err = authorize(ctx, tx, owner, parentID); err != nil {
			return nil, err
		}
	}
	var parent any
	if parentID != "" {
		parent = parentID
	}
	if _, err = tx.ExecContext(ctx, "INSERT INTO sessions(id,owner,parent_id,created_ns) VALUES(?,?,?,?)", id, owner, parent, time.Now().UTC().UnixNano()); err != nil {
		return nil, fmt.Errorf("create native session: %w", err)
	}
	if err = tx.Commit(); err != nil {
		return nil, err
	}
	return &Session{repository: r, owner: owner, id: id}, nil
}

type queryer interface {
	QueryRowContext(context.Context, string, ...any) *sql.Row
}

func authorize(ctx context.Context, q queryer, owner, id string) error {
	if !validIdentity(owner) || !validIdentity(id) {
		return ErrNotFound
	}
	var exists int
	err := q.QueryRowContext(ctx, "SELECT 1 FROM sessions WHERE owner=? AND id=?", owner, id).Scan(&exists)
	if errors.Is(err, sql.ErrNoRows) {
		return ErrNotFound
	}
	return err
}

// OpenSession is a read-only lookup: it does not create, recover or load history.
func (r *Repository) OpenSession(ctx context.Context, owner, id string) (*Session, error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return nil, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return nil, err
	}
	return &Session{repository: r, owner: owner, id: id}, nil
}

func (r *Repository) List(ctx context.Context, owner, after string, limit int) (SessionPage, error) {
	if !validIdentity(owner) || (after != "" && !validIdentity(after)) {
		return SessionPage{}, ErrNotFound
	}
	if limit < 1 || limit > 100 {
		return SessionPage{}, errors.New("session page limit must be 1..100")
	}
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return SessionPage{}, ErrClosed
	}
	rows, err := r.db.QueryContext(ctx, "SELECT id,coalesce(parent_id,''),created_ns,next_seq-1 FROM sessions WHERE owner=? AND id>? ORDER BY id LIMIT ?", owner, after, limit+1)
	if err != nil {
		return SessionPage{}, err
	}
	defer rows.Close()
	page := SessionPage{}
	for rows.Next() {
		var info SessionInfo
		var created int64
		if err = rows.Scan(&info.ID, &info.ParentID, &created, &info.HighWater); err != nil {
			return SessionPage{}, err
		}
		if len(page.Sessions) == limit {
			page.HasMore = true
			break
		}
		info.CreatedAt = time.Unix(0, created).UTC()
		page.Sessions = append(page.Sessions, info)
		page.Next = info.ID
	}
	return page, rows.Err()
}

func (s *Session) ID() string { return s.id }
func (s *Session) Flush(ctx context.Context) error {
	if err := ctx.Err(); err != nil {
		return err
	}
	s.repository.mu.Lock()
	defer s.repository.mu.Unlock()
	if s.repository.closed {
		return ErrClosed
	}
	return nil
}

// BeginRun is the sole run-claim path: recovery and ownership precede execution.
// The release function is idempotent and must be deferred by the runtime owner.
func (r *Repository) BeginRun(ctx context.Context, owner, id string) (RecoveryReport, func(), error) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.closed {
		return RecoveryReport{}, nil, ErrClosed
	}
	if err := authorize(ctx, r.db, owner, id); err != nil {
		return RecoveryReport{}, nil, err
	}
	if r.active[id] {
		return RecoveryReport{}, nil, ErrActive
	}
	report, err := r.recoverLocked(ctx, owner, id)
	if err != nil {
		return RecoveryReport{}, nil, err
	}
	r.active[id] = true
	var once sync.Once
	release := func() { once.Do(func() { r.mu.Lock(); delete(r.active, id); r.mu.Unlock() }) }
	return report, release, nil
}
