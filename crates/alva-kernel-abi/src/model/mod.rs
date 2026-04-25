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
    /// Reasoning effort for models that support it (Claude extended
    /// thinking, OpenAI gpt-5 / o-series, Gemini 2.5+). `None` = use
    /// the provider's default (no effort override).
    ///
    /// Scope: per-request. Anthropic additionally constrains that a
    /// single assistant turn (including all tool-use iterations) must
    /// use the **same** mode — don't toggle mid-turn during tool loops.
    /// See `ReasoningEffort` for value meanings.
    pub reasoning_effort: Option<ReasoningEffort>,
    /// Free-form provider-specific options merged into the request body
    /// after every other field has been assembled. Last-write-wins:
    /// keys here override anything the provider built. Use cases:
    ///   - Doubao `thinking: { type: "disabled" }` to turn off reasoning
    ///   - OpenAI-compatible proxies that need `extra_body` style
    ///     pass-through (Ollama options, LiteLLM, vLLM)
    ///   - Anthropic beta headers' equivalents (e.g.
    ///     `anthropic-version` is sent as a header so it's NOT here, but
    ///     fields like custom `metadata` go through this)
    ///
    /// `None` = no overrides. Stored as a JSON object map so providers
    /// can copy into their own `serde_json::Value` without re-parsing.
    pub extra_body: Option<serde_json::Map<String, serde_json::Value>>,
    /// When `true`, the kernel sends an **empty** tool list to the LLM
    /// even when `state.tools` is populated. Use this for models that
    /// don't support function calling (e.g. some local / older
    /// chat-only deployments). The provider's `tools` request field is
    /// then omitted entirely (matches AMP / pi-mono behavior — they
    /// drop the field rather than sending `tools: []`).
    ///
    /// Set per-turn via `Agent::set_disable_tools`. Mirrors the
    /// `supports_tools=false` model override that flows from
    /// Settings → backend.
    pub disable_tools: bool,
}

/// Cross-provider reasoning effort level.
///
/// Each variant maps to different wire-level representations per provider:
/// - **Anthropic** `thinking: {type:"enabled", budget_tokens: N}` (int)
/// - **OpenAI (Chat / Responses)** `reasoning_effort: "<enum>"` (string)
/// - **Gemini** `thinkingConfig.thinkingBudget: N` (int) or `.thinkingLevel`
///
/// Not all providers accept all levels. Adapters perform best-effort
/// translation — e.g. `XHigh` only valid on `gpt-5.1-codex-max`, clamped to
/// `High` on other OpenAI models; `Minimal` clamped to `Low` on gpt-5.1+.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    /// Explicit "no reasoning" — gpt-5.1+ supports this as `"none"`,
    /// Anthropic maps to `thinking: {type:"disabled"}`, Gemini Flash
    /// maps to `thinkingBudget: 0`.
    None,
    /// Original gpt-5's `"minimal"`. Fastest but no tool-heavy plans;
    /// not supported on gpt-5.1+. Other providers map to `Low`.
    Minimal,
    /// Low reasoning. Broadly supported.
    Low,
    /// Medium reasoning (default on most reasoning models).
    Medium,
    /// High reasoning.
    High,
    /// Extra-high reasoning — only valid on `gpt-5.1-codex-max`. Other
    /// providers clamp to `High`.
    XHigh,
}

impl ReasoningEffort {
    /// Parse from the case-insensitive string values callers pass in
    /// (API requests, config files). Unknown strings return `None`.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "none" => Some(Self::None),
            "minimal" => Some(Self::Minimal),
            "low" => Some(Self::Low),
            "medium" => Some(Self::Medium),
            "high" => Some(Self::High),
            "xhigh" => Some(Self::XHigh),
            _ => None,
        }
    }

    /// Canonical lowercase string (e.g. `"medium"`). Use for logs + UI +
    /// passing to providers that want the enum directly.
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::XHigh => "xhigh",
        }
    }

    /// Suggested Anthropic / Gemini token-budget translation for this
    /// effort level. Adapters may override per model family (e.g. Anthropic
    /// Opus 4.7 uses `adaptive` regardless). Returns `None` for `None`.
    pub fn suggested_token_budget(&self) -> Option<u32> {
        match self {
            Self::None => None,
            Self::Minimal => Some(1024),
            Self::Low => Some(2048),
            Self::Medium => Some(8192),
            Self::High => Some(16384),
            Self::XHigh => Some(24576),
        }
    }
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
