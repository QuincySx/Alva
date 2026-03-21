// INPUT:  crate::domain::message, crate::domain::tool, crate::error, async_trait, tokio::sync::mpsc
// OUTPUT: LLMResponse, StopReason, TokenUsage, StreamChunk, LLMRequest, LLMProvider (trait)
// POS:    Defines the abstract LLM provider interface with sync and streaming completion methods.
use crate::domain::message::{LLMContent, LLMMessage};
use crate::domain::tool::ToolDefinition;
use crate::error::EngineError;
use async_trait::async_trait;
use tokio::sync::mpsc;

/// Non-streaming LLM response
#[derive(Debug, Clone)]
pub struct LLMResponse {
    pub content: Vec<LLMContent>,
    pub stop_reason: StopReason,
    pub usage: TokenUsage,
}

/// Why the LLM stopped generating
#[derive(Debug, Clone, PartialEq)]
pub enum StopReason {
    /// Normal completion, no tool calls
    EndTurn,
    /// LLM wants to execute tools
    ToolUse,
    /// Hit max_tokens limit
    MaxTokens,
    /// Stop sequence triggered
    StopSequence,
}

/// Token usage statistics (aligned with AI SDK + rig-core)
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    /// Total tokens (may differ from input+output due to caching)
    pub total_tokens: Option<u32>,
    /// Cached input tokens (prompt caching optimization)
    pub cached_input_tokens: Option<u32>,
}

/// Streaming chunk events
#[derive(Debug, Clone)]
pub enum StreamChunk {
    /// Incremental text from the model
    TextDelta(String),
    /// Incremental thinking / reasoning text (from models like DeepSeek R1)
    ThinkingDelta(String),
    /// Incremental tool call data
    ToolCallDelta {
        id: String,
        name: String,
        input_delta: String,
    },
    /// Stream finished, final aggregated response
    Done(LLMResponse),
}

/// Tool choice strategy (matches AI SDK's ToolChoice)
#[derive(Debug, Clone, PartialEq)]
pub enum ToolChoice {
    /// Model decides whether to use tools
    Auto,
    /// Model must not use tools
    None,
    /// Model must use at least one tool
    Required,
    /// Model must use the specified tool
    Tool(String),
}

/// Response format specification (matches AI SDK's responseFormat)
#[derive(Debug, Clone)]
pub enum ResponseFormat {
    /// Plain text response (default)
    Text,
    /// JSON response, optionally constrained by a schema
    Json {
        schema: Option<serde_json::Value>,
        name: Option<String>,
        description: Option<String>,
    },
}

/// Parameters for an LLM request
/// Aligned with AI SDK's LanguageModelV4CallOptions
#[derive(Debug, Clone)]
pub struct LLMRequest {
    pub messages: Vec<LLMMessage>,
    pub tools: Vec<ToolDefinition>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
    // --- Fields aligned with AI SDK ---
    /// Tool choice strategy: auto (default), none, required, or specific tool
    pub tool_choice: Option<ToolChoice>,
    /// Response format: text (default) or json with optional schema
    pub response_format: Option<ResponseFormat>,
    /// Top-P nucleus sampling
    pub top_p: Option<f32>,
    /// Top-K sampling
    pub top_k: Option<u32>,
    /// Presence penalty (-2.0 to 2.0)
    pub presence_penalty: Option<f32>,
    /// Frequency penalty (-2.0 to 2.0)
    pub frequency_penalty: Option<f32>,
    /// Custom stop sequences
    pub stop_sequences: Option<Vec<String>>,
    /// Seed for deterministic sampling
    pub seed: Option<u64>,
    /// Provider-specific options (escape hatch for vendor extensions)
    pub provider_options: Option<serde_json::Value>,
}

/// Abstract LLM provider interface
#[async_trait]
pub trait LLMProvider: Send + Sync {
    /// Model identifier string
    fn model_id(&self) -> &str;

    /// Non-streaming completion
    async fn complete(&self, request: LLMRequest) -> Result<LLMResponse, EngineError>;

    /// Streaming completion, sends chunks through the channel
    async fn complete_stream(
        &self,
        request: LLMRequest,
        tx: mpsc::Sender<StreamChunk>,
    ) -> Result<(), EngineError>;

    /// Fast local token estimate (rough, for compaction decisions)
    fn estimate_tokens(&self, messages: &[LLMMessage]) -> u32 {
        messages
            .iter()
            .flat_map(|m| &m.content)
            .map(|c| match c {
                LLMContent::Text { text } => text.len() as u32 / 4,
                LLMContent::ToolUse { input, .. } => input.to_string().len() as u32 / 4,
                LLMContent::ToolResult { content, .. } => content.len() as u32 / 4,
                LLMContent::Image { .. } => 500, // Fixed estimate for images
                LLMContent::Reasoning { text } => text.len() as u32 / 4,
            })
            .sum()
    }
}
