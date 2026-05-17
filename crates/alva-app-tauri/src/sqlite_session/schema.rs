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

    // Note: the legacy `models` table (proto-design predating the frontend
    // localStorage model store) is no longer created here — it had zero
    // callers. Existing on-disk DBs keep the orphan table; we don't drop it
    // since dropping is destructive and a stray empty table is harmless.

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
    //
    // Mix of two concerns until the legacy SqliteEvalSessionManager is fully
    // retired in favour of SqliteSessionRegistry:
    //
    //   * Legacy fields used by SqliteEvalSessionManager: preview /
    //     plugin_config / model_id / workspace_id / turns / total_tokens /
    //     duration_ms
    //   * SessionRegistry trait fields (alva-app-core::SessionMetadata):
    //     status / agent_id / title / metadata_json / updated_at /
    //     archived_at / stats_json / usage_json
    //
    // The two sets coexist on one row — splitting them would just JOIN every
    // read with no extra information. ALTER ADD COLUMN below backfills
    // existing DBs.
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
            duration_ms       INTEGER,
            -- SessionRegistry trait fields:
            status            TEXT NOT NULL DEFAULT 'idle',  -- SessionStatus serde tag
            agent_id          TEXT,
            title             TEXT,
            metadata_json     TEXT NOT NULL DEFAULT '{}',    -- BTreeMap<String,String>
            updated_at        INTEGER NOT NULL DEFAULT 0,
            archived_at       INTEGER,
            stats_json        TEXT NOT NULL DEFAULT '{}',    -- ThreadStats
            usage_json        TEXT NOT NULL DEFAULT '{}'     -- ThreadUsage
        );
    ")?;

    // Backfill the new columns on existing databases. ADD COLUMN errors with
    // "duplicate column" if the column is already present — for an existing
    // schema that's expected; for a fresh DB the CREATE above already added
    // them and these ALTERs will all be no-ops. Swallow errors per column.
    for ddl in [
        "ALTER TABLE sessions ADD COLUMN status TEXT NOT NULL DEFAULT 'idle'",
        "ALTER TABLE sessions ADD COLUMN agent_id TEXT",
        "ALTER TABLE sessions ADD COLUMN title TEXT",
        "ALTER TABLE sessions ADD COLUMN metadata_json TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE sessions ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0",
        "ALTER TABLE sessions ADD COLUMN archived_at INTEGER",
        "ALTER TABLE sessions ADD COLUMN stats_json TEXT NOT NULL DEFAULT '{}'",
        "ALTER TABLE sessions ADD COLUMN usage_json TEXT NOT NULL DEFAULT '{}'",
    ] {
        let _ = conn.execute(ddl, []);
    }

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
