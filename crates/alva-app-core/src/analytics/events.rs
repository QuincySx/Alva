// INPUT:  serde, std::collections::HashMap
// OUTPUT: AnalyticsEvent, event_names
// POS:    Analytics event definitions — structured telemetry events with builder pattern.

//! Analytics event definitions — structured telemetry events with builder pattern.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Analytics event matching Claude Code's event system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnalyticsEvent {
    /// Event name (e.g., "tool_use", "query_start", "permission_decision")
    pub name: String,
    /// Event timestamp (unix milliseconds)
    pub timestamp: u64,
    /// Session ID
    pub session_id: String,
    /// Event properties (no code or file paths!)
    pub properties: HashMap<String, serde_json::Value>,
}

impl AnalyticsEvent {
    pub fn new(name: impl Into<String>, session_id: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
            session_id: session_id.into(),
            properties: HashMap::new(),
        }
    }

    pub fn with_property(
        mut self,
        key: impl Into<String>,
        value: impl Into<serde_json::Value>,
    ) -> Self {
        self.properties.insert(key.into(), value.into());
        self
    }
}

/// Pre-defined event names
pub mod event_names {
    pub const TOOL_USE: &str = "tool_use";
    pub const TOOL_RESULT: &str = "tool_result";
    pub const QUERY_START: &str = "query_start";
    pub const QUERY_END: &str = "query_end";
    pub const PERMISSION_DECISION: &str = "permission_decision";
    pub const SESSION_START: &str = "session_start";
    pub const SESSION_END: &str = "session_end";
    pub const COMPACTION: &str = "compaction";
    pub const ERROR: &str = "error";
    pub const COMMAND_USE: &str = "command_use";
    pub const AGENT_SPAWN: &str = "agent_spawn";
    pub const AGENT_END: &str = "agent_end";
    pub const MEMORY_EXTRACTION: &str = "memory_extraction";
    pub const PLUGIN_EVENT: &str = "plugin_event";
    pub const MCP_CONNECTION: &str = "mcp_connection";
    pub const MODEL_SWITCH: &str = "model_switch";
}
