// INPUT:  std::path, async_trait, tokio_rusqlite, crate::domain::{message, session}, crate::error, crate::ports::storage, super::migrations
// OUTPUT: SqliteStorage
// POS:    SQLite-backed SessionStorage implementation with WAL mode and migration support.
//! SQLite-backed implementation of [`SessionStorage`].

use std::path::Path;

use async_trait::async_trait;
use tokio_rusqlite::Connection;

use crate::domain::message::{LLMContent, LLMMessage, Role};
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use crate::ports::storage::SessionStorage;

use super::migrations;

/// Persistent session storage backed by SQLite (WAL mode).
pub struct SqliteStorage {
    conn: Connection,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at `path` and run migrations.
    pub async fn open(path: impl AsRef<Path>) -> Result<Self, EngineError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)
            .await
            .map_err(|e| EngineError::Storage(format!("sqlite open: {e}")))?;

        // Enable WAL mode + run migrations inside a single blocking call.
        conn.call(|conn| {
            conn.pragma_update(None, "journal_mode", "wal")?;
            conn.pragma_update(None, "foreign_keys", "on")?;
            migrations::run_migrations(conn)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::Storage(format!("sqlite init: {e}")))?;

        Ok(Self { conn })
    }

    /// Open an in-memory database (useful for tests).
    pub async fn open_in_memory() -> Result<Self, EngineError> {
        let conn = Connection::open_in_memory()
            .await
            .map_err(|e| EngineError::Storage(format!("sqlite open memory: {e}")))?;

        conn.call(|conn| {
            conn.pragma_update(None, "foreign_keys", "on")?;
            migrations::run_migrations(conn)?;
            Ok(())
        })
        .await
        .map_err(|e| EngineError::Storage(format!("sqlite init: {e}")))?;

        Ok(Self { conn })
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn status_to_str(s: &SessionStatus) -> &'static str {
    match s {
        SessionStatus::Idle => "idle",
        SessionStatus::Running => "running",
        SessionStatus::WaitingForHuman => "waiting_for_human",
        SessionStatus::Completed => "completed",
        SessionStatus::Cancelled => "cancelled",
        SessionStatus::Error => "error",
    }
}

fn str_to_status(s: &str) -> SessionStatus {
    match s {
        "running" => SessionStatus::Running,
        "waiting_for_human" => SessionStatus::WaitingForHuman,
        "completed" => SessionStatus::Completed,
        "cancelled" => SessionStatus::Cancelled,
        "error" => SessionStatus::Error,
        _ => SessionStatus::Idle,
    }
}

fn role_to_str(r: &Role) -> &'static str {
    match r {
        Role::System => "system",
        Role::User => "user",
        Role::Assistant => "assistant",
        Role::Tool => "tool",
    }
}

fn str_to_role(s: &str) -> Role {
    match s {
        "system" => Role::System,
        "user" => Role::User,
        "assistant" => Role::Assistant,
        "tool" => Role::Tool,
        _ => Role::User,
    }
}

/// Extract a tool_call_id from the first ToolResult block, if any.
fn extract_tool_call_id(content: &[LLMContent]) -> Option<String> {
    content.iter().find_map(|c| match c {
        LLMContent::ToolResult { tool_use_id, .. } => Some(tool_use_id.clone()),
        _ => None,
    })
}

// ---------------------------------------------------------------------------
// SessionStorage implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl SessionStorage for SqliteStorage {
    async fn create_session(&self, session: &Session) -> Result<(), EngineError> {
        let id = session.id.clone();
        let status = status_to_str(&session.status).to_string();
        let workspace = session.workspace.clone();
        let config_snapshot = serde_json::to_string(&session.agent_config_snapshot)
            .map_err(|e| EngineError::Serialization(e.to_string()))?;
        let total_tokens = session.total_tokens as i64;
        let iteration_count = session.iteration_count as i64;

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO sessions (id, status, workspace_path, config_snapshot, total_tokens, iteration_count)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    rusqlite::params![id, status, workspace, config_snapshot, total_tokens, iteration_count],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| EngineError::Storage(format!("create_session: {e}")))?;

        Ok(())
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, status, workspace_path, config_snapshot, total_tokens, iteration_count
                     FROM sessions WHERE id = ?1",
                )?;
                let mut rows = stmt.query(rusqlite::params![id])?;
                if let Some(row) = rows.next()? {
                    let status_str: String = row.get(1)?;
                    let config_str: String = row.get(3)?;
                    let total_tokens: i64 = row.get(4)?;
                    let iteration_count: i64 = row.get(5)?;
                    Ok(Some(Session {
                        id: row.get(0)?,
                        status: str_to_status(&status_str),
                        workspace: row.get(2)?,
                        agent_config_snapshot: serde_json::from_str(&config_str)
                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                        total_tokens: total_tokens as u32,
                        iteration_count: iteration_count as u32,
                    }))
                } else {
                    Ok(None)
                }
            })
            .await
            .map_err(|e| EngineError::Storage(format!("get_session: {e}")))
    }

    async fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), EngineError> {
        let id = id.to_string();
        let id_for_err = id.clone();
        let status_str = status_to_str(&status).to_string();

        let rows_affected = self
            .conn
            .call(move |conn| {
                let n = conn.execute(
                    "UPDATE sessions SET status = ?1, last_active_at = datetime('now') WHERE id = ?2",
                    rusqlite::params![status_str, id],
                )?;
                Ok(n)
            })
            .await
            .map_err(|e| EngineError::Storage(format!("update_session_status: {e}")))?;

        if rows_affected == 0 {
            return Err(EngineError::SessionNotFound(id_for_err));
        }
        Ok(())
    }

    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError> {
        let workspace = workspace.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT id, status, workspace_path, config_snapshot, total_tokens, iteration_count
                     FROM sessions WHERE workspace_path = ?1 ORDER BY last_active_at DESC",
                )?;
                let mut rows = stmt.query(rusqlite::params![workspace])?;
                let mut result = Vec::new();
                while let Some(row) = rows.next()? {
                    let status_str: String = row.get(1)?;
                    let config_str: String = row.get(3)?;
                    let total_tokens: i64 = row.get(4)?;
                    let iteration_count: i64 = row.get(5)?;
                    result.push(Session {
                        id: row.get(0)?,
                        status: str_to_status(&status_str),
                        workspace: row.get(2)?,
                        agent_config_snapshot: serde_json::from_str(&config_str)
                            .unwrap_or(serde_json::Value::Object(serde_json::Map::new())),
                        total_tokens: total_tokens as u32,
                        iteration_count: iteration_count as u32,
                    });
                }
                Ok(result)
            })
            .await
            .map_err(|e| EngineError::Storage(format!("list_sessions: {e}")))
    }

    async fn delete_session(&self, id: &str) -> Result<(), EngineError> {
        let id = id.to_string();
        self.conn
            .call(move |conn| {
                // CASCADE will remove messages too.
                conn.execute("DELETE FROM sessions WHERE id = ?1", rusqlite::params![id])?;
                Ok(())
            })
            .await
            .map_err(|e| EngineError::Storage(format!("delete_session: {e}")))?;
        Ok(())
    }

    async fn append_message(
        &self,
        session_id: &str,
        msg: &LLMMessage,
    ) -> Result<(), EngineError> {
        let session_id = session_id.to_string();
        let msg_id = msg.id.clone();
        let role = role_to_str(&msg.role).to_string();
        let content_json = serde_json::to_string(&msg.content)
            .map_err(|e| EngineError::Serialization(e.to_string()))?;
        let turn_index = msg.turn_index as i64;
        let token_count = msg.token_count.map(|t| t as i64);
        let tool_call_id = extract_tool_call_id(&msg.content);

        self.conn
            .call(move |conn| {
                conn.execute(
                    "INSERT INTO messages (session_id, msg_id, role, content_json, turn_index, token_count, tool_call_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    rusqlite::params![session_id, msg_id, role, content_json, turn_index, token_count, tool_call_id],
                )?;
                Ok(())
            })
            .await
            .map_err(|e| EngineError::Storage(format!("append_message: {e}")))?;
        Ok(())
    }

    async fn get_messages(&self, session_id: &str) -> Result<Vec<LLMMessage>, EngineError> {
        let session_id = session_id.to_string();
        self.conn
            .call(move |conn| {
                let mut stmt = conn.prepare(
                    "SELECT msg_id, role, content_json, turn_index, token_count
                     FROM messages WHERE session_id = ?1 ORDER BY id ASC",
                )?;
                let mut rows = stmt.query(rusqlite::params![session_id])?;
                let mut result = Vec::new();
                while let Some(row) = rows.next()? {
                    let role_str: String = row.get(1)?;
                    let content_str: String = row.get(2)?;
                    let turn_index: i64 = row.get(3)?;
                    let token_count: Option<i64> = row.get(4)?;
                    let content: Vec<LLMContent> = serde_json::from_str(&content_str)
                        .unwrap_or_default();
                    result.push(LLMMessage {
                        id: row.get(0)?,
                        role: str_to_role(&role_str),
                        content,
                        turn_index: turn_index as u32,
                        token_count: token_count.map(|t| t as u32),
                    });
                }
                Ok(result)
            })
            .await
            .map_err(|e| EngineError::Storage(format!("get_messages: {e}")))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::message::LLMMessage;
    use crate::domain::session::{Session, SessionStatus};

    fn sample_session() -> Session {
        Session {
            id: "sess-001".into(),
            workspace: "/tmp/test".into(),
            agent_config_snapshot: serde_json::json!({"model": "test"}),
            status: SessionStatus::Idle,
            total_tokens: 0,
            iteration_count: 0,
        }
    }

    #[tokio::test]
    async fn test_session_crud() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();

        // create
        storage.create_session(&session).await.unwrap();

        // get
        let fetched = storage.get_session("sess-001").await.unwrap().unwrap();
        assert_eq!(fetched.id, "sess-001");
        assert_eq!(fetched.status, SessionStatus::Idle);
        assert_eq!(fetched.workspace, "/tmp/test");

        // update status
        storage
            .update_session_status("sess-001", SessionStatus::Running)
            .await
            .unwrap();
        let fetched = storage.get_session("sess-001").await.unwrap().unwrap();
        assert_eq!(fetched.status, SessionStatus::Running);

        // list
        let list = storage.list_sessions("/tmp/test").await.unwrap();
        assert_eq!(list.len(), 1);

        // delete
        storage.delete_session("sess-001").await.unwrap();
        assert!(storage.get_session("sess-001").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn test_message_append_and_get() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();
        storage.create_session(&session).await.unwrap();

        let msg1 = LLMMessage::user("Hello agent");
        let msg2 = LLMMessage::assistant(vec![crate::domain::message::LLMContent::Text {
            text: "Hi there!".into(),
        }]);

        storage.append_message("sess-001", &msg1).await.unwrap();
        storage.append_message("sess-001", &msg2).await.unwrap();

        let messages = storage.get_messages("sess-001").await.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].text(), "Hello agent");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].text(), "Hi there!");
    }

    #[tokio::test]
    async fn test_cascade_delete() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let session = sample_session();
        storage.create_session(&session).await.unwrap();

        let msg = LLMMessage::user("test");
        storage.append_message("sess-001", &msg).await.unwrap();

        // deleting session should cascade to messages
        storage.delete_session("sess-001").await.unwrap();
        let messages = storage.get_messages("sess-001").await.unwrap();
        assert!(messages.is_empty());
    }

    #[tokio::test]
    async fn test_update_nonexistent_session() {
        let storage = SqliteStorage::open_in_memory().await.unwrap();
        let result = storage
            .update_session_status("nonexistent", SessionStatus::Running)
            .await;
        assert!(result.is_err());
    }
}
