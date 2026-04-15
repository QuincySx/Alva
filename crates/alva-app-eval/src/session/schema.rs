// INPUT:  rusqlite
// OUTPUT: init_schema, DDL constants
// POS:    SQLite schema DDL for the eval session store.
//         Called once at startup from SqliteEvalSessionManager::open.

use rusqlite::Connection;

pub const CREATE_SESSIONS: &str = "
CREATE TABLE IF NOT EXISTS sessions (
    session_id        TEXT PRIMARY KEY,
    parent_session_id TEXT,
    created_at        INTEGER NOT NULL,
    preview           TEXT NOT NULL DEFAULT '',
    schema_version    INTEGER NOT NULL DEFAULT 1,
    -- Run metadata populated after flush/projection
    model_id          TEXT,
    turns             INTEGER,
    total_tokens      INTEGER,
    duration_ms       INTEGER
);
";

pub const CREATE_EVENTS: &str = "
CREATE TABLE IF NOT EXISTS events (
    session_id    TEXT NOT NULL,
    seq           INTEGER NOT NULL,
    uuid          TEXT NOT NULL,
    parent_uuid   TEXT,
    timestamp     INTEGER NOT NULL,
    event_type    TEXT NOT NULL,
    emitter_json  TEXT NOT NULL,
    message_json  TEXT,
    data_json     TEXT,
    PRIMARY KEY (session_id, seq),
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
);
";

pub const CREATE_IDX_UUID: &str =
    "CREATE INDEX IF NOT EXISTS idx_events_uuid ON events(uuid);";

pub const CREATE_IDX_TYPE: &str =
    "CREATE INDEX IF NOT EXISTS idx_events_type ON events(session_id, event_type);";

pub const CREATE_SNAPSHOTS: &str = "
CREATE TABLE IF NOT EXISTS snapshots (
    session_id TEXT PRIMARY KEY,
    data       BLOB NOT NULL,
    updated_at INTEGER NOT NULL,
    FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
);
";

/// Run all CREATE TABLE / CREATE INDEX statements and configure WAL mode.
/// Idempotent — safe to call on every startup.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;",
    )?;
    conn.execute_batch(CREATE_SESSIONS)?;
    conn.execute_batch(CREATE_EVENTS)?;
    conn.execute_batch(CREATE_IDX_UUID)?;
    conn.execute_batch(CREATE_IDX_TYPE)?;
    conn.execute_batch(CREATE_SNAPSHOTS)?;
    Ok(())
}
