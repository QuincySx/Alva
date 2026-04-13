//! Events that extensions can subscribe to via HostAPI::on().

use serde_json::Value;
use alva_kernel_abi::tool::execution::ToolOutput;

/// Events that extensions can subscribe to via HostAPI::on().
#[derive(Debug, Clone)]
pub enum ExtensionEvent {
    AgentStart,
    AgentEnd { error: Option<String> },
    BeforeToolCall { tool_name: String, tool_call_id: String, arguments: Value },
    AfterToolCall { tool_name: String, tool_call_id: String, result: ToolOutput },
    Input { text: String },
}

impl ExtensionEvent {
    pub fn event_type(&self) -> &'static str {
        match self {
            Self::AgentStart => "agent_start",
            Self::AgentEnd { .. } => "agent_end",
            Self::BeforeToolCall { .. } => "before_tool_call",
            Self::AfterToolCall { .. } => "after_tool_call",
            Self::Input { .. } => "input",
        }
    }
}

/// Result from an event handler.
#[derive(Debug)]
pub enum EventResult {
    /// Continue processing (no-op).
    Continue,
    /// Block the operation (for before_tool_call).
    Block { reason: String },
    /// Mark input as fully handled (short-circuit).
    Handled,
}
