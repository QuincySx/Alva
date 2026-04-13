// INPUT:  uuid, serde, thiserror, std::time::Duration
// OUTPUT: ScopeId, ChildScopeConfig, ScopeError, ScopeSnapshot
// POS:    Shared types for SpawnScope — the unified execution context managing agent lifecycle.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::time::Duration;

/// Unique identifier for a scope, wrapping a UUID v4 string.
#[derive(Clone, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct ScopeId(String);

impl ScopeId {
    pub fn new() -> Self {
        Self(uuid::Uuid::new_v4().to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ScopeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Builder for creating a child scope with specific configuration.
///
/// Tool inheritance is **no longer** a scope-level config: the parent
/// agent's LLM picks which tools to grant per-spawn by passing their
/// names in the `agent` tool call (see `AgentSpawnTool`). The whitelist
/// is filtered against the parent's own tool set, so a child can never
/// get a tool the parent doesn't already have.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChildScopeConfig {
    pub role: String,
    pub system_prompt: String,
    #[serde(with = "optional_duration_millis")]
    pub timeout: Option<Duration>,
    pub max_iterations: Option<u32>,
}

impl ChildScopeConfig {
    pub fn new(role: impl Into<String>) -> Self {
        Self {
            role: role.into(),
            system_prompt: String::new(),
            timeout: None,
            max_iterations: None,
        }
    }

    pub fn with_system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = Some(timeout);
        self
    }

    pub fn with_max_iterations(mut self, max: u32) -> Self {
        self.max_iterations = Some(max);
        self
    }
}

/// Serde helper for `Option<Duration>` as milliseconds.
mod optional_duration_millis {
    use serde::{self, Deserialize, Deserializer, Serializer};
    use std::time::Duration;

    pub fn serialize<S>(value: &Option<Duration>, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match value {
            Some(d) => serializer.serialize_some(&d.as_millis()),
            None => serializer.serialize_none(),
        }
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<Option<Duration>, D::Error>
    where
        D: Deserializer<'de>,
    {
        let opt: Option<u64> = Option::deserialize(deserializer)?;
        Ok(opt.map(Duration::from_millis))
    }
}

/// Errors that can occur during scope operations.
#[derive(Debug, thiserror::Error)]
pub enum ScopeError {
    #[error("Scope depth exceeded: current depth {current} >= max {max}")]
    DepthExceeded { current: u32, max: u32 },

    #[error("Budget exceeded: {reason}")]
    BudgetExceeded { reason: String },

    #[error("{0}")]
    Other(String),
}

/// A snapshot of scope state for debugging and logging.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScopeSnapshot {
    pub id: String,
    pub parent_id: Option<String>,
    pub depth: u32,
    pub role: String,
    pub session_id: String,
    pub children_count: usize,
    pub completed: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_id_display() {
        let id = ScopeId::new();
        assert!(!id.to_string().is_empty());
    }

    #[test]
    fn scope_id_equality() {
        let id = ScopeId::new();
        let id2 = id.clone();
        assert_eq!(id, id2);
        assert_ne!(id, ScopeId::new());
    }

    #[test]
    fn child_config_builder() {
        let config = ChildScopeConfig::new("planner")
            .with_system_prompt("You plan.")
            .with_timeout(std::time::Duration::from_secs(120))
            .with_max_iterations(30);
        assert_eq!(config.role, "planner");
        assert_eq!(config.max_iterations, Some(30));
    }

    #[test]
    fn child_config_defaults() {
        let config = ChildScopeConfig::new("worker");
        assert!(config.timeout.is_none());
        assert!(config.max_iterations.is_none());
    }

    #[test]
    fn scope_error_display() {
        let err = ScopeError::DepthExceeded { current: 3, max: 3 };
        assert!(err.to_string().contains("depth"));

        let err2 = ScopeError::BudgetExceeded {
            reason: "tokens".into(),
        };
        assert!(err2.to_string().contains("tokens"));
    }

    #[test]
    fn scope_snapshot_serializes() {
        let snap = ScopeSnapshot {
            id: "abc".into(),
            parent_id: None,
            depth: 0,
            role: "root".into(),
            session_id: "sess-1".into(),
            children_count: 2,
            completed: false,
        };
        let json = serde_json::to_string(&snap).unwrap();
        assert!(json.contains("root"));
    }
}
