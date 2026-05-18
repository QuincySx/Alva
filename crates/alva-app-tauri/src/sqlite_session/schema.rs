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
    // them and these ALTERs will all be no-ops. We swallow ONLY that case
    // and log all other errors (disk full, schema corruption, locked DB,
    // etc.) — silently dropping those caused queries against missing
    // columns to fail later with confusing "no such column" errors.
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
        if let Err(e) = conn.execute(ddl, []) {
            // SQLite returns "duplicate column name: <col>" when the
            // column already exists. Match the stable substring (the
            // exact message has been unchanged for years) and skip
            // logging only that case.
            let msg = e.to_string();
            if !msg.contains("duplicate column") {
                tracing::warn!(
                    ddl,
                    error = %e,
                    "ALTER TABLE backfill failed unexpectedly; subsequent queries against this column may fail",
                );
            }
        }
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

#[cfg(test)]
mod tests {
    //! Tests for `init_schema` — the DDL gate every SQLite persistence
    //! path depends on. Schema drift here surfaces only at runtime as
    //! "no such column" errors that confuse users; these tests pin the
    //! contract:
    //!   * tables + indexes present after fresh init
    //!   * SessionRegistry-trait columns (added in L82) are visible
    //!   * a second init_schema call is a no-op (idempotent — fresh DB
    //!     vs reopen path)
    //!   * "legacy" DB missing the trait columns gets backfilled via
    //!     ALTER ADD COLUMN (the duplicate-column silent-swallow path
    //!     that L83 hand-tuned)
    //!
    //! All tests use `Connection::open_in_memory()` so there's no fs
    //! side effect and no new dev-dep needed.
    use super::*;
    use rusqlite::Connection;

    fn fresh() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory");
        init_schema(&conn).expect("init_schema on fresh DB");
        conn
    }

    fn table_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='table' AND name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    fn index_exists(conn: &Connection, name: &str) -> bool {
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type='index' AND name=?1",
                [name],
                |r| r.get(0),
            )
            .unwrap_or(0);
        count > 0
    }

    fn column_names(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("PRAGMA table_info({table})"))
            .expect("prepare PRAGMA table_info");
        stmt.query_map([], |row| row.get::<_, String>(1)) // col 1 = name
            .expect("query table_info")
            .filter_map(Result::ok)
            .collect()
    }

    #[test]
    fn fresh_init_creates_all_four_tables() {
        let conn = fresh();
        for t in ["workspaces", "sessions", "events", "snapshots"] {
            assert!(table_exists(&conn, t), "table missing after init: {t}");
        }
    }

    #[test]
    fn fresh_init_creates_event_indexes() {
        let conn = fresh();
        // Both indexes back hot queries: idx_events_uuid for parent-link
        // lookups; idx_events_type for the per-session event-type filter
        // the inspector uses. Missing either silently degrades to full
        // table scans.
        assert!(index_exists(&conn, "idx_events_uuid"));
        assert!(index_exists(&conn, "idx_events_type"));
    }

    #[test]
    fn fresh_init_sessions_includes_session_registry_columns() {
        // Regression guard for the L82 column addition. The
        // SessionRegistry trait's metadata roundtrip relies on ALL of
        // these columns being decodable from sessions rows; dropping
        // any breaks list / archive / metadata immediately.
        let conn = fresh();
        let cols = column_names(&conn, "sessions");
        for required in [
            "session_id",
            "preview",
            "created_at",
            // Trait-side columns:
            "status",
            "agent_id",
            "title",
            "metadata_json",
            "updated_at",
            "archived_at",
            "stats_json",
            "usage_json",
        ] {
            assert!(
                cols.iter().any(|c| c == required),
                "sessions missing column: {required} (have: {cols:?})"
            );
        }
    }

    #[test]
    fn init_schema_is_idempotent_on_second_call() {
        // Pin: re-opening an existing DB must not error. This is the
        // hot path on every Tauri startup; a regression here would
        // crash the app on launch with no recovery.
        let conn = Connection::open_in_memory().expect("open in-memory");
        init_schema(&conn).expect("first init");
        init_schema(&conn).expect("second init must also succeed (idempotent)");
        // Columns must still be intact.
        let cols = column_names(&conn, "sessions");
        assert!(cols.iter().any(|c| c == "stats_json"));
    }

    #[test]
    fn legacy_sessions_table_gets_backfilled_with_trait_columns() {
        // Simulate a pre-L82 on-disk DB: create the OLD sessions table
        // shape (no SessionRegistry trait columns), then run
        // init_schema. The CREATE TABLE is a no-op (IF NOT EXISTS) but
        // the ALTER ADD COLUMN loop fills in the missing columns —
        // this is exactly the "duplicate column" swallow path L83
        // hand-tuned. Without it, legacy DBs would have to be deleted
        // by hand.
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute_batch(
            "CREATE TABLE sessions (
                session_id    TEXT PRIMARY KEY,
                preview       TEXT NOT NULL DEFAULT '',
                created_at    INTEGER NOT NULL,
                model_id      TEXT,
                workspace_id  TEXT,
                plugin_config TEXT NOT NULL DEFAULT '{}',
                turns         INTEGER,
                total_tokens  INTEGER,
                duration_ms   INTEGER
            );",
        )
        .expect("create legacy sessions table");

        // Before: trait columns absent.
        let pre = column_names(&conn, "sessions");
        assert!(!pre.iter().any(|c| c == "status"));
        assert!(!pre.iter().any(|c| c == "metadata_json"));

        init_schema(&conn).expect("init_schema must backfill legacy table");

        // After: trait columns present via ALTER.
        let post = column_names(&conn, "sessions");
        for required in [
            "status",
            "agent_id",
            "title",
            "metadata_json",
            "updated_at",
            "archived_at",
            "stats_json",
            "usage_json",
        ] {
            assert!(
                post.iter().any(|c| c == required),
                "backfill missed column: {required}"
            );
        }
    }

    #[test]
    fn fresh_init_enforces_foreign_keys_pragma() {
        // FK enforcement on `events.session_id → sessions.session_id`
        // is the ON DELETE CASCADE contract. If `PRAGMA foreign_keys`
        // doesn't stick at OFF→ON during init, cascades silently fail
        // and orphan events accumulate.
        let conn = fresh();
        let fk: i64 = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .expect("PRAGMA foreign_keys");
        assert_eq!(fk, 1, "PRAGMA foreign_keys must be ON after init");
    }
}
