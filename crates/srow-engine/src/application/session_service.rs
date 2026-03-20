use crate::domain::agent::AgentConfig;
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use crate::ports::storage::SessionStorage;
use std::sync::Arc;

/// Service for Session CRUD operations
pub struct SessionService {
    storage: Arc<dyn SessionStorage>,
}

impl SessionService {
    pub fn new(storage: Arc<dyn SessionStorage>) -> Self {
        Self { storage }
    }

    /// Create a new session from an agent config
    pub async fn create(&self, config: &AgentConfig) -> Result<Session, EngineError> {
        let session = Session {
            id: uuid::Uuid::new_v4().to_string(),
            workspace: config.workspace.to_string_lossy().to_string(),
            agent_config_snapshot: serde_json::to_value(config)
                .map_err(|e| EngineError::Serialization(e.to_string()))?,
            status: SessionStatus::Idle,
            total_tokens: 0,
            iteration_count: 0,
        };
        self.storage.create_session(&session).await?;
        Ok(session)
    }

    /// Resume an existing session (validates it's not already running)
    pub async fn resume(&self, session_id: &str) -> Result<Session, EngineError> {
        let session = self
            .storage
            .get_session(session_id)
            .await?
            .ok_or_else(|| EngineError::SessionNotFound(session_id.to_string()))?;

        match session.status {
            SessionStatus::Running => Err(EngineError::SessionAlreadyRunning),
            _ => Ok(session),
        }
    }

    pub async fn list(&self, workspace: &str) -> Result<Vec<Session>, EngineError> {
        self.storage.list_sessions(workspace).await
    }
}
