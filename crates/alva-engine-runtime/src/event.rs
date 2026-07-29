// INPUT:  alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolResult}, serde::{Deserialize, Serialize}, serde_json::Value
// OUTPUT: pub enum RuntimeEvent, pub struct RuntimeUsage, pub struct RuntimeCapabilities, pub enum PermissionDecision
// POS:    Defines the unified event, usage, capability, and permission types emitted by all engine adapters.

use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolOutput};
use serde::{Deserialize, Serialize};

/// Unified event type emitted by all engine adapters.
///
/// Lifecycle and pairing rules live in [`crate::stream_contract`] — that
/// module is the source of truth; the notes below are a quick reference.
///
/// **Termination:** `Completed` is the only terminal event and must be last.
/// On fatal errors adapters emit `Error { recoverable: false }` then
/// `Completed { result: None }`. Consumers wait for `Completed` before cleanup.
///
/// **Tools:** announced via `ToolStart` / finished via `ToolEnd` (same `id`).
/// Never embed ToolUse/ToolResult inside `Message.content`.
///
/// **Streaming:** `MessageDelta` carries only consumer-visible deltas
/// (`TextDelta` / `ReasoningDelta`). Control frames must be filtered.
///
/// **Completed.result:** last assistant text when the run succeeded and a
/// final answer exists; `None` on failure or empty output. Never a
/// placeholder like `"completed"`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum RuntimeEvent {
    /// Session has started (at most once per execute stream).
    SessionStarted {
        session_id: String,
        model: Option<String>,
        tools: Vec<String>,
    },

    /// Complete assistant message (non-tool content only).
    ///
    /// `content` does NOT contain ToolUse/ToolResult blocks — those are
    /// extracted into separate ToolStart/ToolEnd events.
    Message {
        id: String,
        role: MessageRole,
        content: Vec<ContentBlock>,
    },

    /// Streaming delta for the current assistant message.
    ///
    /// Only [`StreamEvent::TextDelta`] and [`StreamEvent::ReasoningDelta`].
    MessageDelta { id: String, delta: StreamEvent },

    /// Tool call announced / started (opens id for a later ToolEnd).
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool call finished.
    ///
    /// Adapters must maintain a `HashMap<tool_use_id, tool_name>` to
    /// populate `name` when the backend only returns tool_use_id.
    ToolEnd {
        id: String,
        name: String,
        result: ToolOutput,
        duration_ms: Option<u64>,
    },

    /// Permission approval required from the user (capability-gated).
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: Option<String>,
    },

    /// Session completed — always the final event, exactly once.
    Completed {
        session_id: String,
        /// Final assistant text when available; never a status placeholder.
        result: Option<String>,
        usage: Option<RuntimeUsage>,
    },

    /// Error during execution.
    ///
    /// Maps to cindy's terminal / retry split:
    /// - `recoverable: false` ≈ `isTerminal: true` — turn is dead; adapter
    ///   **must** follow with `Completed { result: None }` and nothing else.
    /// - `recoverable: true` ≈ `willRetry: true` / non-terminal banner —
    ///   consumer **must not** free running/keepalive state; more events may
    ///   follow until a real `Completed`.
    ///
    /// Consumers always wait for [`RuntimeEvent::Completed`] before cleanup
    /// (stricter than cindy, where a terminal error alone can end a turn).
    Error { message: String, recoverable: bool },
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
