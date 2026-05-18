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

#[cfg(test)]
mod tests {
    //! Tests for model-layer value types: ModelConfig Default,
    //! ReasoningEffort parse/as_str/budget invariants, CompletionResponse
    //! constructors, HeuristicTokenCounter chars/4 fallback.
    //!
    //! ReasoningEffort.parse() ingests user-typed strings (API requests,
    //! config files) — missing case-insensitivity or trim defense
    //! silently rejects "MEDIUM " or " high\n" from real configs.
    //! `suggested_token_budget` is the Anthropic thinking-mode mapping;
    //! a monotonicity flip would mean higher effort gets a smaller
    //! budget — opposite of intent, no compile-time signal.
    use super::*;
    use serde_json::json;

    // -- ModelConfig Default -----------------------------------------------

    #[test]
    fn model_config_default_is_all_none_empty_false() {
        let c = ModelConfig::default();
        assert!(c.temperature.is_none());
        assert!(c.max_tokens.is_none());
        assert!(c.stop_sequences.is_empty());
        assert!(c.top_p.is_none());
        assert!(c.reasoning_effort.is_none());
        assert!(c.extra_body.is_none());
        assert!(!c.disable_tools, "Default disable_tools must be false");
    }

    // -- ReasoningEffort::parse / as_str -----------------------------------

    #[test]
    fn parse_all_six_lowercase_variants() {
        assert_eq!(ReasoningEffort::parse("none"), Some(ReasoningEffort::None));
        assert_eq!(ReasoningEffort::parse("minimal"), Some(ReasoningEffort::Minimal));
        assert_eq!(ReasoningEffort::parse("low"), Some(ReasoningEffort::Low));
        assert_eq!(ReasoningEffort::parse("medium"), Some(ReasoningEffort::Medium));
        assert_eq!(ReasoningEffort::parse("high"), Some(ReasoningEffort::High));
        assert_eq!(ReasoningEffort::parse("xhigh"), Some(ReasoningEffort::XHigh));
    }

    #[test]
    fn parse_is_case_insensitive() {
        assert_eq!(ReasoningEffort::parse("MEDIUM"), Some(ReasoningEffort::Medium));
        assert_eq!(ReasoningEffort::parse("High"), Some(ReasoningEffort::High));
        assert_eq!(ReasoningEffort::parse("xHIGH"), Some(ReasoningEffort::XHigh));
    }

    #[test]
    fn parse_trims_surrounding_whitespace() {
        assert_eq!(ReasoningEffort::parse("  low  "), Some(ReasoningEffort::Low));
        assert_eq!(ReasoningEffort::parse("\tmedium\n"), Some(ReasoningEffort::Medium));
    }

    #[test]
    fn parse_unknown_returns_none() {
        assert_eq!(ReasoningEffort::parse(""), None);
        assert_eq!(ReasoningEffort::parse("ultra"), None);
        assert_eq!(ReasoningEffort::parse("highish"), None);
    }

    #[test]
    fn as_str_roundtrips_through_parse_for_all_variants() {
        // Pin: parse(as_str(v)) == Some(v) for every variant — the
        // canonical lowercase form must always be parse-able. Without
        // this, a future rename of as_str's output silently breaks
        // serialize-then-deserialize loops.
        for v in [
            ReasoningEffort::None,
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
        ] {
            assert_eq!(
                ReasoningEffort::parse(v.as_str()),
                Some(v),
                "round-trip failed for variant {v:?} via as_str() = {:?}",
                v.as_str(),
            );
        }
    }

    // -- ReasoningEffort serde --------------------------------------------

    #[test]
    fn serde_uses_lowercase_string() {
        // #[serde(rename_all = "lowercase")] pin — Anthropic / OpenAI
        // wire formats expect "low" / "medium" / etc.
        assert_eq!(serde_json::to_value(ReasoningEffort::Medium).unwrap(), json!("medium"));
        assert_eq!(serde_json::to_value(ReasoningEffort::XHigh).unwrap(), json!("xhigh"));
    }

    #[test]
    fn serde_deserializes_lowercase() {
        let m: ReasoningEffort = serde_json::from_value(json!("medium")).unwrap();
        assert_eq!(m, ReasoningEffort::Medium);
    }

    // -- ReasoningEffort::suggested_token_budget --------------------------

    #[test]
    fn suggested_budget_none_variant_returns_none() {
        // ReasoningEffort::None = "explicit no reasoning"; budget is
        // intentionally None, NOT Some(0). Anthropic uses
        // `thinking: {type:"disabled"}` rather than `budget_tokens: 0`.
        assert_eq!(ReasoningEffort::None.suggested_token_budget(), None);
    }

    #[test]
    fn suggested_budget_is_monotonically_increasing_for_active_levels() {
        // Pin the SEMANTIC contract: higher reasoning effort →
        // higher token budget. A refactor that scrambled the
        // budgets would silently flip user intent (Asked "High",
        // got fewer tokens than "Low").
        let order = [
            ReasoningEffort::Minimal,
            ReasoningEffort::Low,
            ReasoningEffort::Medium,
            ReasoningEffort::High,
            ReasoningEffort::XHigh,
        ];
        let budgets: Vec<u32> = order
            .iter()
            .map(|e| e.suggested_token_budget().expect("active level must have Some budget"))
            .collect();
        for w in budgets.windows(2) {
            assert!(
                w[0] < w[1],
                "monotonicity broken: {} >= {} in {budgets:?}",
                w[0], w[1]
            );
        }
    }

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
