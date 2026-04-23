// INPUT:  serde, crate::base::message::UsageMetadata
// OUTPUT: pub enum StreamEvent
// POS:    Streaming event enum representing incremental deltas from a language model response.
use serde::{Deserialize, Serialize};

use crate::base::message::UsageMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Start,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    /// A new tool call is about to stream. Fires once per tool call, before
    /// any `ToolCallDelta`. UI layers can use this to render a "tool X
    /// starting" indicator and to allocate per-tool state keyed by `id`.
    ToolCallStart {
        id: String,
        name: String,
    },
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_delta: String,
    },
    /// The tool call with this `id` has emitted its last argument delta —
    /// callers holding per-tool buffers can finalize / parse them now.
    /// Fires once per tool call, after all `ToolCallDelta`s for that id.
    ToolCallEnd {
        id: String,
    },
    Usage(UsageMetadata),
    Done,
    Error(String),
}
