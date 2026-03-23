use alva_types::Message;
use crate::domain::session::{Session, SessionStatus};
use crate::error::EngineError;
use async_trait::async_trait;

/// Abstract session storage interface
#[async_trait]
pub trait SessionStorage: Send + Sync {
    async fn create_session(&self, session: &Session) -> Result<(), EngineError>;
    async fn get_session(&self, id: &str) -> Result<Option<Session>, EngineError>;
    async fn update_session_status(
        &self,
        id: &str,
        status: SessionStatus,
    ) -> Result<(), EngineError>;
    async fn list_sessions(&self, workspace: &str) -> Result<Vec<Session>, EngineError>;
    async fn delete_session(&self, id: &str) -> Result<(), EngineError>;

    async fn append_message(&self, session_id: &str, msg: &Message) -> Result<(), EngineError>;
    async fn get_messages(&self, session_id: &str) -> Result<Vec<Message>, EngineError>;
}
