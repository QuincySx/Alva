use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UIMessagePart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<TextPartState>,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        state: Option<TextPartState>,
    },
    Tool {
        id: String,
        tool_name: String,
        input: serde_json::Value,
        state: ToolState,
        #[serde(skip_serializing_if = "Option::is_none")]
        output: Option<serde_json::Value>,
        #[serde(skip_serializing_if = "Option::is_none")]
        error: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    File {
        media_type: String,
        data: String,
    },
    SourceUrl {
        url: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        title: Option<String>,
    },
    SourceDocument {
        id: String,
        title: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        source_type: Option<String>,
    },
    StepStart,
    Custom {
        id: String,
        data: serde_json::Value,
    },
    Data {
        name: String,
        data: serde_json::Value,
    },
}

/// Text/reasoning part streaming state
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TextPartState {
    Streaming,
    Done,
}

/// Tool call 7-state lifecycle
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ToolState {
    InputStreaming,
    InputAvailable,
    ApprovalRequested,
    ApprovalResponded,
    OutputAvailable,
    OutputError,
    OutputDenied,
}
