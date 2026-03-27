// INPUT:  tokio::sync, crate::error, crate::protocol::permission
// OUTPUT: AcpSession, AcpSessionState, PermissionManager
// POS:    Session state machine and permission approval cache for ACP interaction cycles

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

use crate::error::AcpError;
use crate::protocol::permission::{PermissionData, PermissionOption};

// ---------------------------------------------------------------------------
// Session state
// ---------------------------------------------------------------------------

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
/// Body todo!()'d during migration -- depends on event types being rebuilt.
pub struct AcpSession {
    pub session_id: String,
    pub process_id: String,
    pub state: Arc<Mutex<AcpSessionState>>,
    /// Pending outbound message (set by send_prompt/cancel, consumed by caller).
    pending_outbound: Mutex<Option<crate::protocol::message::AcpOutboundMessage>>,
}

impl AcpSession {
    pub fn new(session_id: String, process_id: String) -> Self {
        Self {
            session_id,
            process_id,
            state: Arc::new(Mutex::new(AcpSessionState::Ready)),
            pending_outbound: Mutex::new(None),
        }
    }

    pub async fn send_prompt(&self, prompt: String, resume: bool) -> Result<(), AcpError> {
        let mut state = self.state.lock().await;
        match *state {
            AcpSessionState::Ready | AcpSessionState::Completed => {}
            ref s => {
                return Err(AcpError::Protocol(format!(
                    "cannot send prompt in state {:?}",
                    s
                )));
            }
        }
        *state = AcpSessionState::Running;
        drop(state);

        // The actual send is done by the caller via AcpProcessManager::send().
        // This method validates state transitions and stores the outbound message
        // shape so callers can use it.
        self.pending_outbound
            .lock()
            .await
            .replace(crate::protocol::message::AcpOutboundMessage::Prompt {
                content: prompt,
                resume: if resume { Some(true) } else { None },
            });

        Ok(())
    }

    pub async fn cancel(&self) -> Result<(), AcpError> {
        let mut state = self.state.lock().await;
        match *state {
            AcpSessionState::Running | AcpSessionState::WaitingForPermission { .. } => {
                *state = AcpSessionState::Cancelled;
            }
            _ => {}
        }
        drop(state);

        self.pending_outbound
            .lock()
            .await
            .replace(crate::protocol::message::AcpOutboundMessage::Cancel);

        Ok(())
    }

    /// Take the pending outbound message (if any) for the caller to send
    /// via AcpProcessManager.
    pub async fn take_pending_outbound(
        &self,
    ) -> Option<crate::protocol::message::AcpOutboundMessage> {
        self.pending_outbound.lock().await.take()
    }

    /// Update session state based on an inbound message.
    pub async fn handle_inbound(
        &self,
        msg: &crate::protocol::message::AcpInboundMessage,
    ) {
        use crate::protocol::message::AcpInboundMessage;
        let mut state = self.state.lock().await;
        match msg {
            AcpInboundMessage::RequestPermission { request_id, .. } => {
                *state = AcpSessionState::WaitingForPermission {
                    request_id: request_id.clone(),
                };
            }
            AcpInboundMessage::TaskComplete { .. }
            | AcpInboundMessage::FinishData { .. } => {
                *state = AcpSessionState::Completed;
            }
            AcpInboundMessage::ErrorData { data } => {
                *state = AcpSessionState::Error {
                    message: data.message.clone(),
                };
            }
            _ => {
                // SessionUpdate, MessageUpdate, ToolCallData, etc. keep Running state
                if *state == AcpSessionState::Ready {
                    *state = AcpSessionState::Running;
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Permission manager
// ---------------------------------------------------------------------------

/// Permission approval cache (session-level, lifetime = process lifetime).
/// allow_always / reject_always records are valid across prompts until restart.
pub struct PermissionManager {
    /// tool_name -> PermissionOption (only stores AllowAlways / RejectAlways)
    cache: RwLock<HashMap<String, PermissionOption>>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            cache: RwLock::new(HashMap::new()),
        }
    }

    /// Check if there's a cached always-policy
    pub async fn check_cached(&self, tool_name: &str) -> Option<PermissionData> {
        let cache = self.cache.read().await;
        match cache.get(tool_name)? {
            PermissionOption::AllowAlways => Some(PermissionData {
                option: PermissionOption::AllowAlways,
                reason: None,
            }),
            PermissionOption::RejectAlways => Some(PermissionData {
                option: PermissionOption::RejectAlways,
                reason: Some("previously rejected always".to_string()),
            }),
            // AllowOnce / RejectOnce are not cached
            _ => None,
        }
    }

    /// Record user choice (only AllowAlways / RejectAlways are persisted)
    pub async fn record(&self, tool_name: &str, data: &PermissionData) {
        match data.option {
            PermissionOption::AllowAlways | PermissionOption::RejectAlways => {
                self.cache
                    .write()
                    .await
                    .insert(tool_name.to_string(), data.option.clone());
            }
            _ => {}
        }
    }
}

impl Default for PermissionManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_cached_empty() {
        let pm = PermissionManager::new();
        assert!(pm.check_cached("some_tool").await.is_none());
    }

    #[tokio::test]
    async fn test_record_allow_always() {
        let pm = PermissionManager::new();
        pm.record(
            "read_file",
            &PermissionData {
                option: PermissionOption::AllowAlways,
                reason: None,
            },
        )
        .await;

        let cached = pm.check_cached("read_file").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().option, PermissionOption::AllowAlways);
    }

    #[tokio::test]
    async fn test_record_reject_always() {
        let pm = PermissionManager::new();
        pm.record(
            "execute_shell",
            &PermissionData {
                option: PermissionOption::RejectAlways,
                reason: Some("dangerous".to_string()),
            },
        )
        .await;

        let cached = pm.check_cached("execute_shell").await;
        assert!(cached.is_some());
        assert_eq!(cached.unwrap().option, PermissionOption::RejectAlways);
    }

    #[tokio::test]
    async fn test_record_allow_once_not_cached() {
        let pm = PermissionManager::new();
        pm.record(
            "read_file",
            &PermissionData {
                option: PermissionOption::AllowOnce,
                reason: None,
            },
        )
        .await;

        // AllowOnce should not be cached
        assert!(pm.check_cached("read_file").await.is_none());
    }

    #[tokio::test]
    async fn test_record_reject_once_not_cached() {
        let pm = PermissionManager::new();
        pm.record(
            "read_file",
            &PermissionData {
                option: PermissionOption::RejectOnce,
                reason: None,
            },
        )
        .await;

        assert!(pm.check_cached("read_file").await.is_none());
    }
}
