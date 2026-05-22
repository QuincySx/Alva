// INPUT:  serde, serde_json
// OUTPUT: ToolDefinition
// POS:    Pure-serde struct describing a tool for LLM function calling.
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// ToolDefinition — JSON Schema description for LLM function calling
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolDefinition {
    pub name: String,
    pub description: String,
    /// JSON Schema object describing parameters
    pub parameters: serde_json::Value,
}
