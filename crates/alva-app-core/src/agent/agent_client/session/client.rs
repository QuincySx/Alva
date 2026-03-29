// INPUT:  std::sync, tokio::sync, alva_protocol_acp, crate::agent::agent_client::AcpError
// OUTPUT: AcpSessionState, AcpSession
// POS:    App-level ACP session — thin wrapper over protocol-level AcpSession with app error types.

use crate::agent::agent_client::AcpError;

/// Re-export protocol session state as the app-level type.
pub use alva_protocol_acp::AcpSessionState;

/// App-level ACP session wrapping the protocol-level session.
///
/// Provides the same API but converts protocol errors to app-level AcpError.
pub struct AcpSession {
    inner: alva_protocol_acp::AcpSession,
}

impl AcpSession {
    pub fn new(session_id: String, process_id: String) -> Self {
        Self {
            inner: alva_protocol_acp::AcpSession::new(session_id, process_id),
        }
    }

    pub fn session_id(&self) -> &str {
        &self.inner.session_id
    }

    pub fn process_id(&self) -> &str {
        &self.inner.process_id
    }

    pub async fn state(&self) -> AcpSessionState {
        self.inner.state.lock().await.clone()
    }

    pub async fn send_prompt(&self, prompt: String, resume: bool) -> Result<(), AcpError> {
        self.inner
            .send_prompt(prompt, resume)
            .await
            .map_err(Into::into)
    }

    pub async fn cancel(&self) -> Result<(), AcpError> {
        self.inner.cancel().await.map_err(Into::into)
    }

    pub async fn take_pending_outbound(
        &self,
    ) -> Option<alva_protocol_acp::AcpOutboundMessage> {
        self.inner.take_pending_outbound().await
    }

    pub async fn handle_inbound(&self, msg: &alva_protocol_acp::AcpInboundMessage) {
        self.inner.handle_inbound(msg).await
    }
}
