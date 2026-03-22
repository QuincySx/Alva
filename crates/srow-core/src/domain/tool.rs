// INPUT:  serde
// OUTPUT: ToolCall, ToolResult, ToolDefinition
// POS:    Defines tool call/result/definition types used across the tool pipeline.
//         Kept during migration because runtime/tools/ depends on these.
//         TODO: Migrate to agent-base equivalents when tool implementations are ported.
use serde::{Deserialize, Serialize};

/// A tool call parsed from an LLM response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub input: serde_json::Value,
}

/// Result of executing a tool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    pub is_error: bool,
    pub duration_ms: u64,
}

/// Tool definition for LLM function calling (JSON Schema)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}
