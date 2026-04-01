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

#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError>;

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
