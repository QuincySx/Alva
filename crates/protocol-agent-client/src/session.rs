// ACP session management: session state machine and permission cache.

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
}

impl AcpSession {
    pub fn new(session_id: String, process_id: String) -> Self {
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
