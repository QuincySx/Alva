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

    // -----------------------------------------------------------------------
    // Workspace CRUD
    // -----------------------------------------------------------------------

    pub fn upsert_workspace(&self, ws: &StoredWorkspace) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "INSERT INTO workspaces (workspace_id, path, permissions, created_at)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(workspace_id) DO UPDATE SET
                 path = excluded.path,
                 permissions = excluded.permissions",
            params![ws.workspace_id, ws.path, ws.permissions, ws.created_at],
        );
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
        let _ = conn.execute(
            "UPDATE sessions SET preview = ?2 WHERE session_id = ?1",
            params![session_id, preview],
        );
    }

    // -- Session ↔ Workspace ------------------------------------------------

    pub fn set_session_workspace(&self, session_id: &str, workspace_id: &str) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE sessions SET workspace_id = ?2 WHERE session_id = ?1",
            params![session_id, workspace_id],
        );
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
        let _ = conn.execute(
            "UPDATE sessions SET plugin_config = ?2 WHERE session_id = ?1",
            params![session_id, json],
        );
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
        let _ = conn.execute(
            "UPDATE sessions SET model_id = ?2, turns = ?3, total_tokens = ?4, duration_ms = ?5
             WHERE session_id = ?1",
            params![session_id, model_id, turns, total_tokens, duration_ms],
        );
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
