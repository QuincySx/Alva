pub mod state;
pub mod processor;
pub mod sse;

pub use processor::{process_ui_message_stream, UIMessageStreamUpdate};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UIMessageChunk {
    // Lifecycle
    Start {
        #[serde(skip_serializing_if = "Option::is_none")]
        message_id: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        message_metadata: Option<serde_json::Value>,
    },
    Finish {
        finish_reason: FinishReason,
        #[serde(skip_serializing_if = "Option::is_none")]
        usage: Option<TokenUsage>,
    },

    // Text
    TextStart { id: String },
    TextDelta { id: String, delta: String },
    TextEnd { id: String },

    // Reasoning
    ReasoningStart { id: String },
    ReasoningDelta { id: String, delta: String },
    ReasoningEnd { id: String },

    // Tool calls
    ToolInputStart {
        id: String,
        tool_name: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    ToolInputDelta { id: String, delta: String },
    ToolInputAvailable { id: String, input: serde_json::Value },
    ToolInputError { id: String, error: String },
    ToolApprovalRequest { id: String },
    ToolOutputAvailable { id: String, output: serde_json::Value },
    ToolOutputError { id: String, error: String },
    ToolOutputDenied { id: String },

    // Files
    File { id: String, media_type: String, data: String },
    ReasoningFile { id: String, media_type: String, data: String },

    // Sources
    SourceUrl { id: String, url: String, title: Option<String> },
    SourceDocument { id: String, title: String, source_type: Option<String> },

    // Custom
    Custom { id: String, data: serde_json::Value },

    // Data
    Data { name: String, data: serde_json::Value },

    // Steps
    StartStep,
    FinishStep,

    // Metadata
    MessageMetadata { metadata: serde_json::Value },

    // Token usage
    TokenUsage { usage: TokenUsage },

    // Error / abort
    Error { error: String },
    Abort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FinishReason {
    Stop,
    ToolCalls,
    MaxTokens,
    Error,
    Abort,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ChatStatus {
    Ready,
    Submitted,
    Streaming,
    Error,
}
