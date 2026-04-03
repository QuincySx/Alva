// INPUT:  std::collections, tokio::sync::oneshot
// OUTPUT: PermissionDecision, PermissionManager
// POS:    Session-level HITL permission manager with cached always-allow/deny decisions and async approval flow.
use std::collections::HashSet;

use serde_json::Value;
use tokio::sync::oneshot;

/// Decision from the human operator in an HITL (Human-In-The-Loop) review.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionDecision {
    AllowOnce,
    AllowAlways,
    RejectOnce,
    RejectAlways,
}

/// Tracks session-level permission decisions and manages pending approval
/// requests.
///
/// Lifecycle:
///   1. `check()` — returns cached decision or `None` (need human).
///   2. `request_approval()` — registers a pending request, returns a
///      `oneshot::Receiver` that the caller awaits.
///   3. External code (UI layer) calls `resolve()` with the human's decision.
///   4. The decision is forwarded through the channel and, for *always*
///      variants, cached for future calls.
pub struct PermissionManager {
    always_allowed: HashSet<String>,
    always_denied: HashSet<String>,
    /// Pending approval channels keyed by request_id.
    pending: std::collections::HashMap<String, PendingApproval>,
}

struct PendingApproval {
    scope_key: String,
    tx: oneshot::Sender<PermissionDecision>,
}

impl PermissionManager {
    pub fn new() -> Self {
        Self {
            always_allowed: HashSet::new(),
            always_denied: HashSet::new(),
            pending: std::collections::HashMap::new(),
        }
    }

    /// Check if a tool has a cached always-allow/always-deny decision.
    ///
    /// Returns:
    ///   - `Some(true)`  — always allowed (skip HITL)
    ///   - `Some(false)` — always denied  (block immediately)
    ///   - `None`        — no cached decision, need human approval
    pub fn check(&self, tool_name: &str, args: &Value) -> Option<bool> {
        let scope_key = Self::scope_key(tool_name, args);

        if self.always_allowed.contains(&scope_key) {
            return Some(true);
        }
        if self.always_denied.contains(&scope_key) {
            return Some(false);
        }
        None
    }

    /// Register a pending approval request. Returns a receiver that the engine
    /// awaits; the UI layer calls `resolve(request_id, decision)` when the
    /// human responds.
    pub fn request_approval(
        &mut self,
        request_id: String,
        tool_name: &str,
        args: &Value,
    ) -> oneshot::Receiver<PermissionDecision> {
        let (tx, rx) = oneshot::channel();
        self.pending.insert(
            request_id,
            PendingApproval {
                scope_key: Self::scope_key(tool_name, args),
                tx,
            },
        );
        rx
    }

    /// Called by the UI layer when the human makes a decision.
    ///
    /// Forwards the decision through the pending channel and caches *always*
    /// variants. Returns `false` if the request_id was not found (already
    /// resolved or timed out).
    pub fn resolve(
        &mut self,
        request_id: &str,
        _tool_name: &str,
        decision: PermissionDecision,
    ) -> bool {
        if let Some(pending) = self.pending.remove(request_id) {
            match decision {
                PermissionDecision::AllowAlways => {
                    self.always_allowed.insert(pending.scope_key.clone());
                    self.always_denied.remove(&pending.scope_key);
                }
                PermissionDecision::RejectAlways => {
                    self.always_denied.insert(pending.scope_key.clone());
                    self.always_allowed.remove(&pending.scope_key);
                }
                _ => {}
            }

            let _ = pending.tx.send(decision);
            true
        } else {
            false
        }
    }

    /// Cancel a pending request (e.g., on timeout). The awaiting receiver will
    /// get a `RecvError`, which callers should interpret as rejection.
    pub fn cancel(&mut self, request_id: &str) {
        self.pending.remove(request_id);
        // Dropping the sender causes RecvError on the receiver side
    }

    /// Reset all cached decisions (useful for session reset).
    pub fn reset(&mut self) {
        self.always_allowed.clear();
        self.always_denied.clear();
        self.pending.clear();
    }

    /// Returns whether a specific tool is in the always-allowed set.
    pub fn is_always_allowed(&self, tool_name: &str) -> bool {
        self.always_allowed.contains(tool_name)
    }

    /// Returns whether a specific tool is in the always-denied set.
    pub fn is_always_denied(&self, tool_name: &str) -> bool {
        self.always_denied.contains(tool_name)
    }

    fn scope_key(tool_name: &str, args: &Value) -> String {
        format!("{tool_name}:{}", Self::canonicalize_json(args))
    }

    fn canonicalize_json(value: &Value) -> Value {
        match value {
            Value::Array(values) => Value::Array(
                values
                    .iter()
                    .map(Self::canonicalize_json)
                    .collect(),
            ),
            Value::Object(map) => {
                let mut keys: Vec<_> = map.keys().cloned().collect();
                keys.sort();

                let mut normalized = serde_json::Map::new();
                for key in keys {
                    if let Some(value) = map.get(&key) {
                        normalized.insert(key, Self::canonicalize_json(value));
                    }
                }
                Value::Object(normalized)
            }
            _ => value.clone(),
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
    use serde_json::json;

    #[test]
    fn no_cached_decision_by_default() {
        let pm = PermissionManager::new();
        assert_eq!(pm.check("shell", &json!({"command": "ls"})), None);
    }

    #[test]
    fn always_allow_caches() {
        let mut pm = PermissionManager::new();
        let args = json!({"command": "ls"});
        let _rx = pm.request_approval("req-1".into(), "shell", &args);
        pm.resolve("req-1", "shell", PermissionDecision::AllowAlways);
        assert_eq!(pm.check("shell", &args), Some(true));
    }

    #[test]
    fn always_deny_caches() {
        let mut pm = PermissionManager::new();
        let args = json!({"command": "ls"});
        let _rx = pm.request_approval("req-1".into(), "shell", &args);
        pm.resolve("req-1", "shell", PermissionDecision::RejectAlways);
        assert_eq!(pm.check("shell", &args), Some(false));
    }

    #[test]
    fn allow_once_does_not_cache() {
        let mut pm = PermissionManager::new();
        let args = json!({"command": "ls"});
        let _rx = pm.request_approval("req-1".into(), "shell", &args);
        pm.resolve("req-1", "shell", PermissionDecision::AllowOnce);
        assert_eq!(pm.check("shell", &args), None);
    }

    #[test]
    fn reject_once_does_not_cache() {
        let mut pm = PermissionManager::new();
        let args = json!({"command": "ls"});
        let _rx = pm.request_approval("req-1".into(), "shell", &args);
        pm.resolve("req-1", "shell", PermissionDecision::RejectOnce);
        assert_eq!(pm.check("shell", &args), None);
    }

    #[tokio::test]
    async fn approval_flow_end_to_end() {
        let mut pm = PermissionManager::new();
        let rx = pm.request_approval("req-1".into(), "shell", &json!({"command": "ls"}));
        pm.resolve("req-1", "shell", PermissionDecision::AllowOnce);
        let decision = rx.await.unwrap();
        assert_eq!(decision, PermissionDecision::AllowOnce);
    }

    #[test]
    fn reset_clears_everything() {
        let mut pm = PermissionManager::new();
        let args = json!({"command": "ls"});
        let _rx = pm.request_approval("req-1".into(), "shell", &args);
        pm.resolve("req-1", "shell", PermissionDecision::AllowAlways);
        pm.reset();
        assert_eq!(pm.check("shell", &args), None);
    }

    #[test]
    fn cancel_drops_pending() {
        let mut pm = PermissionManager::new();
        let mut rx = pm.request_approval("req-1".into(), "shell", &json!({"command": "ls"}));
        pm.cancel("req-1");
        // Receiver should get an error since sender was dropped
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn always_allow_is_scoped_to_arguments() {
        let mut pm = PermissionManager::new();
        let args_a = json!({"command": "ls /workspace"});
        let args_b = json!({"command": "ls /workspace/src"});

        let _rx = pm.request_approval("req-1".into(), "shell", &args_a);
        pm.resolve("req-1", "shell", PermissionDecision::AllowAlways);

        assert_eq!(pm.check("shell", &args_a), Some(true));
        assert_eq!(pm.check("shell", &args_b), None);
    }
}
