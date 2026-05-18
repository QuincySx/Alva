// INPUT:  rusqlite, SqliteEvalSession, schema
// OUTPUT: SqliteEvalSessionManager, StoredWorkspace, SessionSummary
// POS:    Legacy session/workspace CRUD over SQLite. New code should prefer
//         SqliteSessionRegistry (sqlite_session/registry.rs) for the
//         SessionRegistry-shaped operations; this struct stays for the live
//         workspace + plugin_config + run_metadata methods until they're
//         migrated.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use alva_kernel_abi::agent_session::AgentSession;

use super::schema::init_schema;
use super::sqlite_session::SqliteEvalSession;

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredWorkspace {
    pub workspace_id: String,
    pub path: String,
    pub permissions: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub session_id: String,
    pub model_id: Option<String>,
    pub workspace_id: Option<String>,
    pub preview: String,
    pub created_at: i64,
    pub turns: Option<i64>,
    pub total_tokens: Option<i64>,
    pub duration_ms: Option<i64>,
}

// ---------------------------------------------------------------------------
// Manager
// ---------------------------------------------------------------------------

pub struct SqliteEvalSessionManager {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteEvalSessionManager {
    pub fn open(db_path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("failed to open DB at {}: {}", db_path.display(), e))?;
        init_schema(&conn)
            .map_err(|e| format!("failed to init DB schema: {}", e))?;
        // Startup cleanup: drop ghost sessions (rows in `sessions` with no
        // corresponding events). These accumulate from failed first-turn
        // runs — auth errors, 404s, network blips — where the session row
        // was created but no event was ever persisted. Harmless but shows
        // up in the UI as "empty" entries. One-shot at open is fine; doesn't
        // race with live writes.
        let deleted = conn
            .execute(
                "DELETE FROM sessions
                 WHERE NOT EXISTS (
                   SELECT 1 FROM events e WHERE e.session_id = sessions.session_id
                 )",
                [],
            )
            .unwrap_or(0);
        if deleted > 0 {
            tracing::info!(count = deleted, "cleaned up ghost sessions with no events");
        }
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Shared reference to the underlying SQLite connection. Lets callers
    /// construct adapters that need to read/write the same DB — most
    /// importantly `SqliteSessionRegistry`, which mirrors metadata into
    /// the `sessions` table from the SessionRegistry trait side.
    pub fn conn(&self) -> &Arc<Mutex<Connection>> {
        &self.conn
    }

    // -----------------------------------------------------------------------
    // Workspace CRUD
    // -----------------------------------------------------------------------

    pub fn upsert_workspace(&self, ws: &StoredWorkspace) {
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute(
            "INSERT INTO workspaces (workspace_id, path, permissions, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_id) DO UPDATE SET
                 path = excluded.path,
                 permissions = excluded.permissions",
            params![ws.workspace_id, ws.path, ws.permissions, ws.created_at],
        ) {
            tracing::warn!(
                workspace_id = %ws.workspace_id,
                error = %e,
                "upsert_workspace failed; session may not resolve workspace path",
            );
        }
    }

    pub fn get_workspace(&self, workspace_id: &str) -> Option<StoredWorkspace> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT workspace_id, path, permissions, created_at FROM workspaces WHERE workspace_id = ?1",
            params![workspace_id],
            |row| Ok(StoredWorkspace {
                workspace_id: row.get(0)?,
                path: row.get(1)?,
                permissions: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "{}".into()),
                created_at: row.get(3)?,
            }),
        ).ok()
    }

    pub fn find_workspace_by_path(&self, path: &str) -> Option<StoredWorkspace> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT workspace_id, path, permissions, created_at FROM workspaces WHERE path = ?1",
            params![path],
            |row| Ok(StoredWorkspace {
                workspace_id: row.get(0)?,
                path: row.get(1)?,
                permissions: row.get::<_, Option<String>>(2)?.unwrap_or_else(|| "{}".into()),
                created_at: row.get(3)?,
            }),
        ).ok()
    }

    // -----------------------------------------------------------------------
    // Session CRUD
    // -----------------------------------------------------------------------

    pub async fn create_session(&self, preview: &str) -> Arc<SqliteEvalSession> {
        let session = Arc::new(SqliteEvalSession::new(self.conn.clone()));
        let session_id = session.session_id().to_string();
        let preview = truncate_preview(preview);
        let now = chrono::Utc::now().timestamp_millis();
        let conn = self.conn.clone();

        let _ = tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO sessions (session_id, created_at, preview)
                 VALUES (?1, ?2, ?3)",
                params![session_id, now, preview],
            )?;
            Ok(())
        })
        .await;

        session
    }

    pub async fn load_session(&self, session_id: &str) -> Option<Arc<SqliteEvalSession>> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let exists = tokio::task::spawn_blocking(move || -> rusqlite::Result<bool> {
            let conn = conn.lock().unwrap();
            let count: i64 = conn.query_row(
                "SELECT COUNT(*) FROM sessions WHERE session_id = ?1",
                params![sid],
                |r| r.get(0),
            )?;
            Ok(count > 0)
        })
        .await
        .ok()
        .and_then(|r| r.ok())
        .unwrap_or(false);

        if !exists {
            return None;
        }

        let session = Arc::new(SqliteEvalSession::with_id(
            self.conn.clone(),
            session_id.to_string(),
        ));
        if let Err(e) = session.restore().await {
            tracing::warn!(session_id, error = %e, "failed to restore session");
            return None;
        }
        Some(session)
    }

    pub fn list_sessions(&self) -> Vec<SessionSummary> {
        let conn = self.conn.lock().unwrap();
        // Filter out "ghost" sessions — rows in `sessions` that have no events
        // at all. These happen when we create_session() on app start / resume
        // but the first LLM call errors out (auth, 404, network) before any
        // event is persisted. Without this filter, the UI shows them in the
        // list and clicking shows empty history — confusing.
        let mut stmt = match conn.prepare(
            "SELECT s.session_id, s.model_id, s.workspace_id, s.preview,
                    s.created_at, s.turns, s.total_tokens, s.duration_ms
             FROM sessions s
             WHERE EXISTS (SELECT 1 FROM events e WHERE e.session_id = s.session_id)
             ORDER BY s.created_at DESC",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "list_sessions: prepare failed");
                return Vec::new();
            }
        };

        stmt.query_map([], |row| {
            Ok(SessionSummary {
                session_id: row.get(0)?,
                model_id: row.get(1)?,
                workspace_id: row.get(2)?,
                preview: row.get::<_, Option<String>>(3)?.unwrap_or_default(),
                created_at: row.get(4)?,
                turns: row.get(5)?,
                total_tokens: row.get(6)?,
                duration_ms: row.get(7)?,
            })
        })
        .map(|rows| rows.filter_map(|r| r.ok()).collect())
        .unwrap_or_default()
    }

    pub fn delete_session(&self, session_id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute("DELETE FROM sessions WHERE session_id = ?1", params![session_id])
            .map(|n| n > 0)
            .unwrap_or(false)
    }

    pub fn update_preview(&self, session_id: &str, preview: &str) {
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute(
            "UPDATE sessions SET preview = ?2 WHERE session_id = ?1",
            params![session_id, preview],
        ) {
            tracing::warn!(
                session_id = %session_id,
                error = %e,
                "update_preview failed; session list will show stale preview",
            );
        }
    }

    // -- Session ↔ Workspace ------------------------------------------------

    pub fn set_session_workspace(&self, session_id: &str, workspace_id: &str) {
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute(
            "UPDATE sessions SET workspace_id = ?2 WHERE session_id = ?1",
            params![session_id, workspace_id],
        ) {
            tracing::warn!(
                session_id = %session_id,
                workspace_id = %workspace_id,
                error = %e,
                "set_session_workspace failed; session won't be bound to workspace on next load",
            );
        }
    }

    pub fn get_session_workspace_path(&self, session_id: &str) -> Option<String> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT w.path FROM sessions s JOIN workspaces w ON s.workspace_id = w.workspace_id
             WHERE s.session_id = ?1",
            params![session_id],
            |row| row.get(0),
        ).ok()
    }

    // -- Session plugin config ----------------------------------------------

    pub fn get_plugin_config(&self, session_id: &str) -> HashMap<String, bool> {
        let conn = self.conn.lock().unwrap();
        conn.query_row(
            "SELECT plugin_config FROM sessions WHERE session_id = ?1",
            params![session_id],
            |row| row.get::<_, String>(0),
        )
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
    }

    pub fn set_plugin_config(&self, session_id: &str, config: &HashMap<String, bool>) {
        let json = serde_json::to_string(config).unwrap_or_else(|_| "{}".into());
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute(
            "UPDATE sessions SET plugin_config = ?2 WHERE session_id = ?1",
            params![session_id, &json],
        ) {
            tracing::warn!(
                session_id = %session_id,
                config_keys = config.len(),
                error = %e,
                "set_plugin_config failed; plugin overrides will be lost on next load",
            );
        }
    }

    // -- Run metadata -------------------------------------------------------

    #[allow(dead_code)]
    pub fn update_run_metadata(
        &self,
        session_id: &str,
        model_id: &str,
        turns: i64,
        total_tokens: i64,
        duration_ms: i64,
    ) {
        let conn = self.conn.lock().unwrap();
        if let Err(e) = conn.execute(
            "UPDATE sessions SET model_id = ?2, turns = ?3, total_tokens = ?4, duration_ms = ?5
             WHERE session_id = ?1",
            params![session_id, model_id, turns, total_tokens, duration_ms],
        ) {
            tracing::warn!(
                session_id = %session_id,
                model_id = %model_id,
                error = %e,
                "update_run_metadata failed; run stats will not reflect this run",
            );
        }
    }
}

fn truncate_preview(s: &str) -> String {
    let max = 80;
    let truncated: String = s.chars().take(max).collect();
    if truncated.len() < s.len() {
        format!("{}...", truncated)
    } else {
        truncated
    }
}

#[cfg(test)]
mod tests {
    //! Tests for SqliteEvalSessionManager surfaces NOT covered by
    //! SqliteSessionRegistry tests (which exercise the SessionRegistry
    //! trait against the shared sessions table):
    //!
    //!   * Workspace CRUD (upsert / get / find_by_path)
    //!   * Plugin-config JSON roundtrip on a sessions row
    //!   * Session ↔ Workspace join + path lookup
    //!   * list_sessions ghost-row filter (UI never shows empty rows)
    //!   * delete_session / update_preview
    //!   * truncate_preview char-boundary behavior
    //!
    //! Hermetic temp_dir pattern (same as registry.rs:749) — no new
    //! dev-dep.
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    fn unique_db_path(label: &str) -> PathBuf {
        let unique = format!(
            "alva-manager-{label}-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        let p = std::env::temp_dir().join(unique);
        let _ = fs::remove_file(&p);
        p
    }

    fn fresh_manager(label: &str) -> (SqliteEvalSessionManager, PathBuf) {
        let path = unique_db_path(label);
        let mgr = SqliteEvalSessionManager::open(path.clone()).expect("open manager");
        (mgr, path)
    }

    // -- Workspace CRUD ----------------------------------------------------

    #[test]
    fn workspace_upsert_then_get_roundtrips_all_fields() {
        let (mgr, path) = fresh_manager("ws-roundtrip");
        let ws = StoredWorkspace {
            workspace_id: "ws-1".into(),
            path: "/Users/alice/proj".into(),
            permissions: r#"{"can_write":true}"#.into(),
            created_at: 1700000000,
        };
        mgr.upsert_workspace(&ws);

        let got = mgr.get_workspace("ws-1").expect("workspace must exist");
        assert_eq!(got.workspace_id, "ws-1");
        assert_eq!(got.path, "/Users/alice/proj");
        assert_eq!(got.permissions, r#"{"can_write":true}"#);
        assert_eq!(got.created_at, 1700000000);

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn workspace_upsert_on_conflict_updates_path_and_permissions() {
        // Pin the ON CONFLICT DO UPDATE clause — second upsert with the
        // same workspace_id overrides path + permissions but preserves
        // created_at (intentionally, since created_at is not in the SET
        // list).
        let (mgr, path) = fresh_manager("ws-conflict");
        mgr.upsert_workspace(&StoredWorkspace {
            workspace_id: "ws-x".into(),
            path: "/old".into(),
            permissions: "{}".into(),
            created_at: 1,
        });
        mgr.upsert_workspace(&StoredWorkspace {
            workspace_id: "ws-x".into(),
            path: "/new".into(),
            permissions: r#"{"k":1}"#.into(),
            created_at: 99,
        });

        let got = mgr.get_workspace("ws-x").expect("ws-x");
        assert_eq!(got.path, "/new", "path must update on conflict");
        assert_eq!(got.permissions, r#"{"k":1}"#);
        // created_at is intentionally preserved (DO UPDATE SET doesn't list it).
        assert_eq!(got.created_at, 1, "created_at must NOT be overridden");

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    #[test]
    fn workspace_find_by_path_returns_matching_row() {
        let (mgr, path) = fresh_manager("ws-by-path");
        mgr.upsert_workspace(&StoredWorkspace {
            workspace_id: "id-a".into(),
            path: "/abs/path/one".into(),
            permissions: "{}".into(),
            created_at: 1,
        });
        mgr.upsert_workspace(&StoredWorkspace {
            workspace_id: "id-b".into(),
            path: "/abs/path/two".into(),
            permissions: "{}".into(),
            created_at: 2,
        });

        let by_path = mgr
            .find_workspace_by_path("/abs/path/two")
            .expect("find by path");
        assert_eq!(by_path.workspace_id, "id-b");

        let missing = mgr.find_workspace_by_path("/never/created");
        assert!(missing.is_none());

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    // -- Plugin config JSON roundtrip --------------------------------------

    #[tokio::test]
    async fn plugin_config_roundtrips_through_sessions_row() {
        // Pin: HashMap<String, bool> persists as JSON in plugin_config
        // and decodes back lossless. UI relies on this to preserve a
        // user's per-session plugin overrides across restarts.
        let (mgr, path) = fresh_manager("plugin-cfg");
        let session = mgr.create_session("preview").await;
        let sid = session.session_id().to_string();

        let mut cfg = std::collections::HashMap::new();
        cfg.insert("read_url".to_string(), true);
        cfg.insert("internet_search".to_string(), false);
        mgr.set_plugin_config(&sid, &cfg);

        let got = mgr.get_plugin_config(&sid);
        assert_eq!(got.get("read_url"), Some(&true));
        assert_eq!(got.get("internet_search"), Some(&false));
        assert_eq!(got.len(), 2);

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn plugin_config_unknown_session_returns_empty_map() {
        // Missing session_id → empty default, NOT error. UI shows
        // "no overrides" gracefully.
        let (mgr, path) = fresh_manager("plugin-missing");
        let got = mgr.get_plugin_config("does-not-exist");
        assert!(got.is_empty());

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    // -- Session ↔ Workspace -----------------------------------------------

    #[tokio::test]
    async fn set_session_workspace_then_get_path_resolves_via_join() {
        let (mgr, path) = fresh_manager("ws-join");
        mgr.upsert_workspace(&StoredWorkspace {
            workspace_id: "w-joined".into(),
            path: "/tmp/workspace".into(),
            permissions: "{}".into(),
            created_at: 1,
        });
        let session = mgr.create_session("hi").await;
        let sid = session.session_id().to_string();

        mgr.set_session_workspace(&sid, "w-joined");
        let resolved = mgr.get_session_workspace_path(&sid);
        assert_eq!(resolved.as_deref(), Some("/tmp/workspace"));

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    #[tokio::test]
    async fn get_session_workspace_path_returns_none_when_unset() {
        let (mgr, path) = fresh_manager("ws-unset");
        let session = mgr.create_session("hi").await;
        let sid = session.session_id().to_string();

        // No set_session_workspace call yet — JOIN finds no row.
        assert!(mgr.get_session_workspace_path(&sid).is_none());

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    // -- list_sessions ghost filter ----------------------------------------

    #[tokio::test]
    async fn list_sessions_excludes_ghost_rows_with_no_events() {
        // The UI never shows ghost sessions: created via create_session
        // but the first turn errored before any event persisted (auth
        // failure / 404 / network). This filter is what prevents the
        // user from seeing a sea of empty chats — pin it.
        let (mgr, path) = fresh_manager("ghost-filter");
        let _ghost = mgr.create_session("never sent").await; // no flush, no events
        let real = mgr.create_session("real chat").await;
        // Persist one event so `real` survives the EXISTS filter.
        use alva_kernel_abi::agent_session::{AgentSession, SessionEvent};
        real.append(SessionEvent::user_message(serde_json::json!("hi"))).await;
        real.flush().await.expect("flush real session");

        let list = mgr.list_sessions();
        let real_sid = real.session_id();
        assert_eq!(list.len(), 1, "ghost session must be filtered out");
        assert_eq!(list[0].session_id, real_sid);

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    // -- delete_session + update_preview -----------------------------------

    #[tokio::test]
    async fn delete_session_returns_true_for_existing_false_for_missing() {
        let (mgr, path) = fresh_manager("delete");
        let session = mgr.create_session("delete me").await;
        let sid = session.session_id().to_string();

        assert!(mgr.delete_session(&sid), "delete of existing → true");
        assert!(!mgr.delete_session(&sid), "second delete → false");
        assert!(!mgr.delete_session("never-existed"), "missing id → false");

        drop(mgr);
        let _ = fs::remove_file(&path);
    }

    // -- truncate_preview unicode safety -----------------------------------

    #[test]
    fn truncate_preview_below_80_chars_passes_through_verbatim() {
        let s = "short preview".to_string();
        assert_eq!(truncate_preview(&s), s);
    }

    #[test]
    fn truncate_preview_above_80_chars_appends_ellipsis() {
        let s: String = "x".repeat(120);
        let out = truncate_preview(&s);
        assert!(out.ends_with("..."));
        // 80 'x' + "..." = 83 bytes
        assert_eq!(out.len(), 83);
    }

    #[test]
    fn truncate_preview_handles_multibyte_chars_at_boundary() {
        // chars().take(80) is char-boundary safe. A string of 100
        // Chinese chars (3 bytes each in UTF-8) must NOT panic and must
        // truncate to 80 chars + ellipsis.
        let s: String = "中".repeat(100);
        let out = truncate_preview(&s);
        assert!(out.ends_with("..."));
        // 80 Chinese chars = 240 bytes + 3 bytes ellipsis = 243.
        assert_eq!(out.chars().filter(|c| *c == '中').count(), 80);
    }
}
