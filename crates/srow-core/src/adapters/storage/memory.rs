use agent_types::Message;
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use crate::ports::storage::SessionStorage;
use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::RwLock;

/// In-memory storage backed by HashMap + RwLock
pub struct MemoryStorage {
    sessions: RwLock<HashMap<String, Session>>,
    messages: RwLock<HashMap<String, Vec<Message>>>,
}

impl MemoryStorage {
    pub fn new() -> Self {
        Self {
            sessions: RwLock::new(HashMap::new()),
            messages: RwLock::new(HashMap::new()),
        }
    }
}

impl Default for MemoryStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl SessionStorage for MemoryStorage {
    async fn create_session(&self, session: &Session) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        sessions.insert(session.id.clone(), session.clone());
        Ok(())
    }

    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError> {
        let sessions = self.sessions.read().await;
        Ok(sessions.get(id).cloned())
    }

    async fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        if let Some(session) = sessions.get_mut(id) {
            session.status = status;
            Ok(())
        } else {
            Err(EngineError::SessionNotFound(id.to_string()))
        }
    }

    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError> {
        let sessions = self.sessions.read().await;
        Ok(sessions
            .values()
            .filter(|s| s.workspace == workspace)
            .cloned()
            .collect())
    }

    async fn delete_session(&self, id: &str) -> Result<(), EngineError> {
        let mut sessions = self.sessions.write().await;
        sessions.remove(id);
        let mut messages = self.messages.write().await;
        messages.remove(id);
        Ok(())
    }

    async fn append_message(&self, session_id: &str, msg: &Message) -> Result<(), EngineError> {
        let mut messages = self.messages.write().await;
        messages
            .entry(session_id.to_string())
            .or_default()
            .push(msg.clone());
        Ok(())
    }

    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, EngineError> {
        let messages = self.messages.read().await;
        Ok(messages.get(session_id).cloned().unwrap_or_default())
    }
}
