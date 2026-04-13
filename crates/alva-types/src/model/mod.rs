// INPUT:  async_trait, futures_core::Stream, std::pin::Pin, crate::base::error::AgentError, crate::base::message::Message, crate::base::stream::StreamEvent, crate::tool::Tool
// OUTPUT: ModelConfig, LanguageModel (trait), TokenCounter (trait), HeuristicTokenCounter
// POS:    Language model trait + TokenCounter capability for accurate token budgeting via bus registration.
use async_trait::async_trait;
use futures_core::Stream;
use std::pin::Pin;

use crate::base::error::AgentError;
use crate::base::message::Message;
use crate::base::stream::StreamEvent;
use crate::tool::Tool;

#[derive(Debug, Clone, Default)]
pub struct ModelConfig {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop_sequences: Vec<String>,
    pub top_p: Option<f32>,
}

/// Result of a `LanguageModel::complete` call.
///
/// Carries both the framework-normalized `Message` and the raw provider
/// response JSON. Most callers only need `.message`; the `raw` field is
/// for consumers that want provider-specific fields the normalization
/// layer doesn't model (Anthropic's full `stop_reason` enum, OpenAI's
/// `logprobs`, `system_fingerprint`, per-token timing, etc.).
///
/// `raw` is `Option<Value>` so mock and test providers that don't have
/// a "real" wire response can return `None` instead of a synthetic blob.
#[derive(Debug, Clone)]
pub struct CompletionResponse {
    /// Framework-normalized message.
    pub message: Message,
    /// The provider's raw JSON response, if it has one. Populated by
    /// real HTTP providers, left `None` by mocks and synthetic sources.
    pub raw: Option<serde_json::Value>,
}

impl CompletionResponse {
    /// Convenience constructor for mocks / tests where there's no raw JSON.
    pub fn from_message(message: Message) -> Self {
        Self { message, raw: None }
    }
}

impl From<Message> for CompletionResponse {
    fn from(message: Message) -> Self {
        Self::from_message(message)
    }
}

#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError>;

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

    fn model_id(&self) -> &str;
}

/// Token counting capability.
///
/// Registered on the bus by the provider layer. Context management
/// and compression systems use this for accurate token budgeting
/// instead of heuristic estimation (chars/4).
pub trait TokenCounter: Send + Sync {
    /// Count tokens in a text string using the model's actual tokenizer.
    fn count_tokens(&self, text: &str) -> usize;

    /// Estimate the context window size (max tokens) for this model.
    fn context_window(&self) -> usize;
}

/// Fallback token counter using chars/4 heuristic.
/// Used when no model-specific tokenizer is available.
pub struct HeuristicTokenCounter {
    pub context_window_size: usize,
}

impl HeuristicTokenCounter {
    pub fn new(context_window_size: usize) -> Self {
        Self { context_window_size }
    }
}

impl TokenCounter for HeuristicTokenCounter {
    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }

    fn context_window(&self) -> usize {
        self.context_window_size
    }
}
