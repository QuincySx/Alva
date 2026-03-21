// INPUT:  serde, serde_json, super::types
// OUTPUT: LanguageModelTool, FunctionTool, ProviderTool, ToolChoice, ToolResultOutput, ToolResultContentItem
// POS:    Tool-related types for Provider V4, defining tool definitions, choice strategies, and result formats.
use serde::{Deserialize, Serialize};
use super::types::ProviderOptions;

/// A tool available to the language model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LanguageModelTool {
    Function(FunctionTool),
    Provider(ProviderTool),
}

/// A function-based tool with a JSON Schema for inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionTool {
    pub name: String,
    pub description: Option<String>,
    pub input_schema: serde_json::Value,
    pub strict: Option<bool>,
    pub provider_options: Option<ProviderOptions>,
}

/// A provider-defined tool (e.g. web search, code interpreter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderTool {
    pub id: String,
    pub name: String,
    pub args: serde_json::Value,
}

/// How the model should choose which tool to use.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolChoice {
    Auto,
    None,
    Required,
    Tool { tool_name: String },
}

/// The output produced by a tool execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultOutput {
    Text { value: String },
    Json { value: serde_json::Value },
    ExecutionDenied { reason: Option<String> },
    ErrorText { value: String },
    ErrorJson { value: serde_json::Value },
    Content { value: Vec<ToolResultContentItem> },
}

/// An individual content item within a tool result.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum ToolResultContentItem {
    Text { text: String },
    FileData {
        data: String,
        media_type: String,
        filename: Option<String>,
    },
    FileUrl { url: String },
    ImageData { data: String, media_type: String },
    ImageUrl { url: String },
    Custom {
        provider_options: Option<ProviderOptions>,
    },
}
