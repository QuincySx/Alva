// INPUT:  rusqlite
// OUTPUT: init_schema
// POS:    SQLite schema — models, workspaces, sessions, events, snapshots. No migration compat.

use rusqlite::Connection;

/// Run all DDL. Idempotent (IF NOT EXISTS). Dev stage — no migration shims.
pub fn init_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=NORMAL;
         PRAGMA foreign_keys=ON;",
    )?;

    // -- Models: globally configured LLM providers --------------------------
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS models (
            model_id     TEXT PRIMARY KEY,
            provider     TEXT NOT NULL,            -- anthropic / openai / openai-responses
            api_key      TEXT NOT NULL DEFAULT '',
            base_url     TEXT,
            display_name TEXT NOT NULL DEFAULT ''
        );
    ")?;

    // -- Workspaces: directories the agent operates in ----------------------
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS workspaces (
            workspace_id TEXT PRIMARY KEY,
            path         TEXT NOT NULL UNIQUE,
            permissions  TEXT NOT NULL DEFAULT '{}', -- JSON: permission rules
            created_at   INTEGER NOT NULL
        );
    ")?;

    // -- Sessions: one per conversation -------------------------------------
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS sessions (
            session_id        TEXT PRIMARY KEY,
            parent_session_id TEXT,
            model_id          TEXT,                  -- FK → models (nullable, set on first send)
            workspace_id      TEXT,                  -- FK → workspaces (nullable)
            plugin_config     TEXT NOT NULL DEFAULT '{}', -- JSON: {plugin_name: bool}
            preview           TEXT NOT NULL DEFAULT '',
            created_at        INTEGER NOT NULL,
            turns             INTEGER,
            total_tokens      INTEGER,
            duration_ms       INTEGER
        );
    ")?;

    // -- Events: the session event stream -----------------------------------
    conn.execute_batch("
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
        CREATE INDEX IF NOT EXISTS idx_events_uuid ON events(uuid);
        CREATE INDEX IF NOT EXISTS idx_events_type ON events(session_id, event_type);
    ")?;

    // -- Snapshots: compressed session state for fast restore ----------------
    conn.execute_batch("
        CREATE TABLE IF NOT EXISTS snapshots (
            session_id TEXT PRIMARY KEY,
            data       BLOB NOT NULL,
            updated_at INTEGER NOT NULL,
            FOREIGN KEY (session_id) REFERENCES sessions(session_id) ON DELETE CASCADE
        );
    ")?;

    Ok(())
}
