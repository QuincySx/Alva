// INPUT:  async_trait, futures_core::Stream, std::pin::Pin, crate::base::error::AgentError, crate::base::message::Message, crate::base::stream::StreamEvent, crate::tool::Tool
// OUTPUT: ModelConfig, ReasoningEffort (via config), LanguageModel (trait), TokenCounter (trait), HeuristicTokenCounter, CompletionResponse
// POS:    Language model trait + TokenCounter capability for accurate token budgeting via bus registration.
//         ModelConfig/ReasoningEffort are pure-serde value types living in model::config.
mod config;
pub use config::{ModelConfig, ReasoningEffort};

use async_trait::async_trait;
use futures_core::Stream;
use std::pin::Pin;

use crate::base::error::AgentError;
use crate::base::message::Message;
use crate::base::stream::StreamEvent;
use crate::tool::Tool;

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

    /// Identifier for the upstream provider this model came from
    /// (`"anthropic"`, `"openai-chat"`, `"gemini"`, …).
    ///
    /// Default `"unknown"` keeps the trait additive for older impls.
    /// Concrete `Provider` impls in `alva-llm-provider` override this so
    /// telemetry can attribute LLM calls to the right backend without
    /// parsing model name conventions.
    fn provider_id(&self) -> &str {
        "unknown"
    }
}

/// Bus Capability: Token counting + context-window lookup.
///
/// **Provider**: `BaseAgentBuilder::build` (default `HeuristicTokenCounter`);
/// `AgentRuntimeBuilder::build` standard-stack path. Replaceable by
/// publishing a provider-specific counter via a custom Extension (e.g.
/// a real tokenizer for the configured model).
/// **Consumers**: `alva-kernel-core::run` for budget reporting,
/// `CompactionMiddleware` (`alva-agent-context`) for token accounting,
/// `ContextHandleImpl` for context assembly size checks.
/// **Why bus**: The tokenizer lives with the provider crate (because each
/// model family has its own encoding). Context management lives in a
/// separate crate with no compile-time dependency on any provider —
/// the bus bridges those layers without a build-time edge.
#[crate::bus_cap]
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

#[cfg(test)]
mod tests {
    //! Tests for model-layer types remaining in mod.rs: CompletionResponse
    //! constructors and HeuristicTokenCounter chars/4 fallback.
    //! ModelConfig / ReasoningEffort tests live in model::config.
    use super::*;

    // -- CompletionResponse -----------------------------------------------

    #[test]
    fn from_message_ctor_leaves_raw_none() {
        let m = Message::user("hi");
        let r = CompletionResponse::from_message(m.clone());
        assert!(r.raw.is_none(), "from_message must NOT synthesize a raw blob");
        assert_eq!(r.message.id, m.id);
    }

    #[test]
    fn from_message_trait_impl_delegates_to_ctor() {
        let m = Message::user("hi");
        let r: CompletionResponse = m.clone().into();
        assert!(r.raw.is_none());
        assert_eq!(r.message.id, m.id);
    }

    // -- HeuristicTokenCounter --------------------------------------------

    #[test]
    fn heuristic_counter_counts_text_len_over_four() {
        let c = HeuristicTokenCounter::new(8192);
        assert_eq!(c.count_tokens(""), 0);
        // "abc" len=3, 3/4 = 0 (integer division — pinned current behavior)
        assert_eq!(c.count_tokens("abc"), 0);
        // 4 chars → 1 token
        assert_eq!(c.count_tokens("abcd"), 1);
        // 12 chars → 3 tokens
        assert_eq!(c.count_tokens("abcdefghijkl"), 3);
    }

    #[test]
    fn heuristic_counter_returns_configured_context_window() {
        let c = HeuristicTokenCounter::new(123_456);
        assert_eq!(c.context_window(), 123_456);
    }
}
