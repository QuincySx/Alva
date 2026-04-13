// INPUT:  alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolResult}, serde::{Deserialize, Serialize}, serde_json::Value
// OUTPUT: pub enum RuntimeEvent, pub struct RuntimeUsage, pub struct RuntimeCapabilities, pub enum PermissionDecision
// POS:    Defines the unified event, usage, capability, and permission types emitted by all engine adapters.

use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolOutput};
use serde::{Deserialize, Serialize};

/// Unified event type emitted by all engine adapters.
///
/// **Termination semantics:** `Completed` is the only terminal event.
/// On errors, adapters emit `Error { recoverable: false }` followed by
/// `Completed { result: None }`. Consumers should wait for `Completed`
/// to finalize cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum RuntimeEvent {
    /// Session has started.
    SessionStarted {
        session_id: String,
        model: Option<String>,
        tools: Vec<String>,
    },

    /// Complete assistant message.
    ///
    /// `content` does NOT contain ToolUse/ToolResult blocks — those are
    /// extracted into separate ToolStart/ToolEnd events.
    Message {
        id: String,
        role: MessageRole,
        content: Vec<ContentBlock>,
    },

    /// Streaming delta (reuses alva_kernel_abi::StreamEvent).
    MessageDelta {
        id: String,
        delta: StreamEvent,
    },

    /// Tool call started.
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool call ended.
    ///
    /// Adapters must maintain a `HashMap<tool_use_id, tool_name>` to
    /// populate `name` since SDK tool_result only carries tool_use_id.
    ToolEnd {
        id: String,
        name: String,
        result: ToolOutput,
        duration_ms: Option<u64>,
    },

    /// Permission approval required from the user.
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: Option<String>,
    },

    /// Session completed (always the final event).
    Completed {
        session_id: String,
        result: Option<String>,
        usage: Option<RuntimeUsage>,
    },

    /// Error during execution.
    Error {
        message: String,
        recoverable: bool,
    },
}

/// Engine-level usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub num_turns: u32,
}

/// Declares what an engine supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub streaming: bool,
    pub tool_control: bool,
    pub permission_callback: bool,
    pub resume: bool,
    pub cancel: bool,
}

/// Permission decision sent back to the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionDecision {
    Allow {
        updated_input: Option<serde_json::Value>,
    },
    Deny {
        message: String,
    },
}
