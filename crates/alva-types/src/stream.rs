// INPUT:  serde, crate::message::UsageMetadata
// OUTPUT: pub enum StreamEvent
// POS:    Streaming event enum representing incremental deltas from a language model response.
use serde::{Deserialize, Serialize};

use crate::message::UsageMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Start,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage(UsageMetadata),
    Done,
    Error(String),
}
