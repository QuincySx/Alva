// INPUT:  serde, serde_json, super::types, super::tool_types
// OUTPUT: LanguageModelMessage, UserContentPart, AssistantContentPart, ToolContentPart, DataContent
// POS:    Prompt/message types for Provider V4, representing the full conversation structure sent to models.
use serde::{Deserialize, Serialize};
use super::types::ProviderOptions;
use super::tool_types::ToolResultOutput;

/// A message in the conversation prompt sent to the language model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "role", rename_all = "kebab-case")]
pub enum LanguageModelMessage {
    System {
        content: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    User {
        content: Vec<UserContentPart>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    Assistant {
        content: Vec<AssistantContentPart>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    Tool {
        content: Vec<ToolContentPart>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

/// Content parts that can appear in a user message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum UserContentPart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    File {
        data: DataContent,
        media_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        filename: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

/// Content parts that can appear in an assistant message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AssistantContentPart {
    Text {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    File {
        data: DataContent,
        media_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    Reasoning {
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    ReasoningFile {
        data: DataContent,
        media_type: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    ToolCall {
        tool_call_id: String,
        tool_name: String,
        input: serde_json::Value,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        output: ToolResultOutput,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    Custom {
        kind: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

/// Content parts that can appear in a tool message.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolContentPart {
    ToolResult {
        tool_call_id: String,
        tool_name: String,
        output: ToolResultOutput,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
    ToolApprovalResponse {
        approval_id: String,
        approved: bool,
        #[serde(skip_serializing_if = "Option::is_none")]
        reason: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        provider_options: Option<ProviderOptions>,
    },
}

/// Raw data content, supporting multiple encoding formats.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum DataContent {
    Bytes { data: Vec<u8> },
    Base64 { data: String },
    Url { url: String },
}
