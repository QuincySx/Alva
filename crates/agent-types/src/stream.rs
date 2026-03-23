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
