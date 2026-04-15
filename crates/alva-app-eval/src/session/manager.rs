// INPUT:  rusqlite, SqliteEvalSession, schema
// OUTPUT: SqliteEvalSessionManager, SessionSummary, StoredRunSummary
// POS:    Manages the eval SQLite DB — creates/loads/lists/deletes sessions.

use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use rusqlite::{params, Connection};
use serde::Serialize;

use alva_kernel_abi::agent_session::AgentSession;

use super::schema::init_schema;
use super::sqlite_session::SqliteEvalSession;

/// Summary returned by list() — mirrors the legacy StoredRunSummary shape
/// so the frontend JSON stays the same.
#[derive(Debug, Clone, Serialize)]
pub struct StoredRunSummary {
    pub run_id: String,
    pub model_id: String,
    pub turns: usize,
    pub total_tokens: u64,
    pub duration_ms: u64,
    pub created_at: String,
    pub preview: String,
}

/// Manages the eval session database.
///
/// Owns a single shared `rusqlite::Connection` (WAL mode) behind a
/// `Mutex`. All SQL runs inside `tokio::task::spawn_blocking`.
pub struct SqliteEvalSessionManager {
    conn: Arc<Mutex<Connection>>,
    #[allow(dead_code)]
    db_path: PathBuf,
}

impl SqliteEvalSessionManager {
    /// Open (or create) the eval DB at `db_path` and run the schema migration.
    pub fn open(db_path: PathBuf) -> Result<Self, String> {
        let conn = Connection::open(&db_path)
            .map_err(|e| format!("failed to open eval DB at {}: {}", db_path.display(), e))?;
        init_schema(&conn)
            .map_err(|e| format!("failed to init eval DB schema: {}", e))?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Create a new session row in the DB and return a handle.
    ///
    /// `preview` is a short text excerpt stored in the sessions table for
    /// display in the run list. Typically the first ~80 chars of the user
    /// prompt.
    pub async fn create_session(&self, preview: &str) -> Arc<SqliteEvalSession> {
        let session = Arc::new(SqliteEvalSession::new(self.conn.clone()));
        let session_id = session.session_id().to_string();
        let preview = truncate_preview(preview);
        let now = chrono::Utc::now().timestamp_millis();
        let conn = self.conn.clone();

        let _ = tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO sessions (session_id, parent_session_id, created_at, preview)
                 VALUES (?1, NULL, ?2, ?3)",
                params![session_id, now, preview],
            )?;
            Ok(())
        })
        .await;

        session
    }

    /// Load an existing session by id, restoring its events into memory.
    /// Returns `None` if the session doesn't exist in the DB.
    pub async fn load_session(&self, session_id: &str) -> Option<Arc<SqliteEvalSession>> {
        // Check existence first.
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
            tracing::warn!(
                session_id,
                error = %e,
                "SqliteEvalSessionManager: failed to restore session"
            );
            return None;
        }
        Some(session)
    }

    /// List all sessions, most recently created first.
    ///
    /// The summary data (model_id, turns, total_tokens, duration_ms) is
    /// stored in the sessions table as metadata and updated by the eval
    /// app after each run via `update_run_metadata`.
    pub fn list_runs(&self) -> Vec<StoredRunSummary> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = match conn.prepare(
            "SELECT session_id, model_id, turns, total_tokens, duration_ms,
                    created_at, preview
             FROM sessions
             ORDER BY created_at DESC",
        ) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(error = %e, "list_runs: failed to prepare statement");
                return Vec::new();
            }
        };

        let rows = match stmt.query_map([], |row| {
            let ms = row.get::<_, Option<i64>>(5)?.unwrap_or(0);
            Ok(StoredRunSummary {
                run_id: row.get(0)?,
                model_id: row.get::<_, Option<String>>(1)?.unwrap_or_default(),
                turns: row.get::<_, Option<i64>>(2)?.unwrap_or(0) as usize,
                total_tokens: row.get::<_, Option<i64>>(3)?.unwrap_or(0) as u64,
                duration_ms: row.get::<_, Option<i64>>(4)?.unwrap_or(0) as u64,
                created_at: {
                    // Format as ISO-8601 for frontend compatibility.
                    chrono::DateTime::from_timestamp_millis(ms)
                        .map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.3fZ").to_string())
                        .unwrap_or_else(|| ms.to_string())
                },
                preview: row.get::<_, Option<String>>(6)?.unwrap_or_default(),
            })
        }) {
            Ok(r) => r,
            Err(e) => {
                tracing::warn!(error = %e, "list_runs: query failed");
                return Vec::new();
            }
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Delete a session and all its events/snapshot (CASCADE).
    pub fn delete_session(&self, session_id: &str) -> bool {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM sessions WHERE session_id = ?1",
            params![session_id],
        )
        .map(|n| n > 0)
        .unwrap_or(false)
    }

    /// Update the run metadata columns in the sessions table after a run
    /// completes.  Called by `create_run` after projection so the run list
    /// shows real numbers.
    pub fn update_run_metadata(
        &self,
        session_id: &str,
        model_id: &str,
        turns: usize,
        total_tokens: u64,
        duration_ms: u64,
    ) {
        let conn = self.conn.lock().unwrap();
        let _ = conn.execute(
            "UPDATE sessions
             SET model_id = ?2, turns = ?3, total_tokens = ?4, duration_ms = ?5
             WHERE session_id = ?1",
            params![
                session_id,
                model_id,
                turns as i64,
                total_tokens as i64,
                duration_ms as i64,
            ],
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
