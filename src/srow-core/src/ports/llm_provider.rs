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

/// Token usage statistics
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
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

/// Parameters for an LLM request
#[derive(Debug, Clone)]
pub struct LLMRequest {
    pub messages: Vec<LLMMessage>,
    pub tools: Vec<ToolDefinition>,
    pub system: Option<String>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
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
            })
            .sum()
    }
}
