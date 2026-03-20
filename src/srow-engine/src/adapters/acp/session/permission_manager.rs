use std::collections::HashMap;
use tokio::sync::RwLock;

use crate::adapters::acp::protocol::permission::{PermissionData, PermissionOption};

/// Permission approval cache (session-level, lifetime = process lifetime).
/// allow_always / reject_always records are valid across prompts until Srow restarts.
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
