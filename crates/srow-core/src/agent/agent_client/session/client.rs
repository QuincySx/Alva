// INPUT:  std::sync, tokio::sync, crate::agent::agent_client::{protocol, connection, session::permission_manager, AcpError}
// OUTPUT: AcpSessionState, AcpSession
// POS:    ACP session state machine — drives inbound message handling, content forwarding, and HITL permission flow.
//         COMMENTED OUT during migration: depends on deleted UIMessageChunk/FinishReason.
//         TODO: Rebuild using agent-core event types.

use std::sync::Arc;
use tokio::sync::Mutex;

use crate::agent::agent_client::AcpError;

#[derive(Debug, Clone, PartialEq)]
pub enum AcpSessionState {
    Ready,
    Running,
    WaitingForPermission { request_id: String },
    Completed,
    Cancelled,
    Error { message: String },
    Crashed,
}

/// A single ACP interaction session (corresponds to one prompt -> response cycle).
///
/// Body commented out during migration — UIMessageChunk dependency removed.
pub struct AcpSession {
    pub session_id: String,
    pub process_id: String,
    pub state: Arc<Mutex<AcpSessionState>>,
}

impl AcpSession {
    pub fn new(
        session_id: String,
        process_id: String,
    ) -> Self {
        Self {
            session_id,
            process_id,
            state: Arc::new(Mutex::new(AcpSessionState::Ready)),
        }
    }

    pub async fn send_prompt(&self, _prompt: String, _resume: bool) -> Result<(), AcpError> {
        todo!("Rebuild AcpSession on agent-core event types")
    }

    pub async fn cancel(&self) -> Result<(), AcpError> {
        todo!("Rebuild AcpSession on agent-core event types")
    }
}
