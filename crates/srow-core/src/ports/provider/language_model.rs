// INPUT:  std::pin, async_trait, futures, super::types, super::errors, super::content, super::prompt, super::tool_types
// OUTPUT: LanguageModel (trait), LanguageModelCallOptions, ResponseFormat, ReasoningLevel,
//         LanguageModelGenerateResult, FinishReason, UnifiedFinishReason, LanguageModelUsage,
//         UsageInputTokens, UsageOutputTokens, ResponseMetadata, LanguageModelStreamPart,
//         LanguageModelStreamResult
// POS:    Core language model trait and all associated request/response types for Provider V4.
use std::pin::Pin;
use async_trait::async_trait;
use futures::Stream;
use super::types::*;
use super::errors::ProviderError;
use super::content::*;
use super::prompt::LanguageModelMessage;
use super::tool_types::*;

// ---------------------------------------------------------------------------
// LanguageModel trait
// ---------------------------------------------------------------------------

/// Abstract language model interface (Provider V4 specification).
#[async_trait]
pub trait LanguageModel: Send + Sync {
    /// The specification version this model implements.
    fn specification_version(&self) -> &str {
        "v4"
    }

    /// The provider identifier (e.g. "openai", "anthropic").
    fn provider(&self) -> &str;

    /// The model identifier (e.g. "gpt-4o", "claude-sonnet-4-20250514").
    fn model_id(&self) -> &str;

    /// Generate a complete response (non-streaming).
    async fn do_generate(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError>;

    /// Generate a streaming response.
    async fn do_stream(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError>;
}

// ---------------------------------------------------------------------------
// Call options
// ---------------------------------------------------------------------------

/// Options for a language model call.
pub struct LanguageModelCallOptions {
    pub prompt: Vec<LanguageModelMessage>,
    pub max_output_tokens: Option<u32>,
    pub temperature: Option<f32>,
    pub stop_sequences: Option<Vec<String>>,
    pub top_p: Option<f32>,
    pub top_k: Option<u32>,
    pub presence_penalty: Option<f32>,
    pub frequency_penalty: Option<f32>,
    pub response_format: Option<ResponseFormat>,
    pub seed: Option<u64>,
    pub tools: Option<Vec<LanguageModelTool>>,
    pub tool_choice: Option<ToolChoice>,
    pub reasoning: Option<ReasoningLevel>,
    pub provider_options: Option<ProviderOptions>,
    pub headers: Option<ProviderHeaders>,
}

/// The desired response format from the model.
#[derive(Debug, Clone)]
pub enum ResponseFormat {
    Text,
    Json {
        schema: Option<serde_json::Value>,
        name: Option<String>,
        description: Option<String>,
    },
}

/// The level of reasoning/thinking to request from the model.
#[derive(Debug, Clone, PartialEq)]
pub enum ReasoningLevel {
    ProviderDefault,
    None,
    Minimal,
    Low,
    Medium,
    High,
    XHigh,
}

// ---------------------------------------------------------------------------
// Generate result
// ---------------------------------------------------------------------------

/// The result of a non-streaming language model call.
pub struct LanguageModelGenerateResult {
    pub content: Vec<LanguageModelContent>,
    pub finish_reason: FinishReason,
    pub usage: LanguageModelUsage,
    pub provider_metadata: Option<ProviderMetadata>,
    pub warnings: Vec<ProviderWarning>,
    pub response: Option<ResponseMetadata>,
}

/// Why the model stopped generating.
#[derive(Debug, Clone)]
pub struct FinishReason {
    /// Normalized finish reason.
    pub unified: UnifiedFinishReason,
    /// Raw provider-specific finish reason string.
    pub raw: Option<String>,
}

/// Normalized finish reasons across all providers.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum UnifiedFinishReason {
    Stop,
    Length,
    ContentFilter,
    ToolCalls,
    Error,
    Other,
}

/// Token usage statistics for a language model call.
#[derive(Debug, Clone, Default)]
pub struct LanguageModelUsage {
    pub input_tokens: UsageInputTokens,
    pub output_tokens: UsageOutputTokens,
    /// Raw provider-specific usage data.
    pub raw: Option<serde_json::Value>,
}

/// Breakdown of input token usage.
#[derive(Debug, Clone, Default)]
pub struct UsageInputTokens {
    pub total: Option<u32>,
    pub no_cache: Option<u32>,
    pub cache_read: Option<u32>,
    pub cache_write: Option<u32>,
}

/// Breakdown of output token usage.
#[derive(Debug, Clone, Default)]
pub struct UsageOutputTokens {
    pub total: Option<u32>,
    pub text: Option<u32>,
    pub reasoning: Option<u32>,
}

/// Metadata about the provider's response.
#[derive(Debug, Clone)]
pub struct ResponseMetadata {
    pub id: Option<String>,
    pub timestamp: Option<chrono::DateTime<chrono::Utc>>,
    pub model_id: Option<String>,
    pub headers: Option<ProviderHeaders>,
}

// ---------------------------------------------------------------------------
// Stream types
// ---------------------------------------------------------------------------

/// Individual parts emitted during a streaming language model response.
#[derive(Debug, Clone)]
pub enum LanguageModelStreamPart {
    // Text generation
    TextStart { id: String },
    TextDelta { id: String, delta: String },
    TextEnd { id: String },

    // Reasoning/thinking
    ReasoningStart { id: String },
    ReasoningDelta { id: String, delta: String },
    ReasoningEnd { id: String },

    // Tool input streaming
    ToolInputStart {
        id: String,
        tool_name: String,
        title: Option<String>,
    },
    ToolInputDelta { id: String, delta: String },
    ToolInputEnd { id: String },

    // Complete objects
    ToolCall { content: LanguageModelContent },
    ToolResult { content: LanguageModelContent },
    ToolApprovalRequest { content: LanguageModelContent },
    File { content: LanguageModelContent },
    ReasoningFile { content: LanguageModelContent },
    Source { source: LanguageModelSource },
    Custom { content: LanguageModelContent },

    // Control events
    StreamStart { warnings: Vec<ProviderWarning> },
    Metadata(ResponseMetadata),
    Finish {
        usage: LanguageModelUsage,
        finish_reason: FinishReason,
        provider_metadata: Option<ProviderMetadata>,
    },

    // Raw data and errors
    Raw { value: serde_json::Value },
    Error { error: String },
}

/// The result of initiating a streaming language model call.
pub struct LanguageModelStreamResult {
    pub stream: Pin<Box<dyn Stream<Item = LanguageModelStreamPart> + Send>>,
    pub response: Option<ResponseMetadata>,
}
