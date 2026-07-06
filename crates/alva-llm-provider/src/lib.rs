#![cfg(not(target_family = "wasm"))]

//! LLM provider implementations for alva agent framework.
//!
//! **Native-only by design.** This crate uses `reqwest` directly, whose
//! wasm32 futures are not `Send` — at odds with the `Send + Sync` bounds
//! of `alva_kernel_abi::LanguageModel`. Wasm consumers should implement
//! their own `LanguageModel` over `gloo-net::http` or `web_sys::fetch`
//! with the spawn_local + oneshot bridging pattern (see
//! `alva-host-wasm::sleeper::WasmSleeper` for the template) and bypass
//! this crate entirely.
//!
//! The entire crate body is gated on `cfg(not(target_family = "wasm"))`
//! so it compiles to an empty library on wasm32 targets, keeping
//! `cargo check --target wasm32 --workspace` green without forcing
//! every consumer to feature-flag this crate.
//!
//! Supports multiple provider backends:
//! - **OpenAI Chat Completions**: Any service with an OpenAI-compatible `/chat/completions` API
//!   (OpenAI, Ollama, vLLM, DeepSeek, etc.)
//! - **OpenAI Responses**: OpenAI Responses API (`/v1/responses`) with typed
//!   input items and named SSE events.
//! - **Anthropic Messages**: Direct Anthropic Messages API with native tool use,
//!   thinking blocks, and streaming support.
//!
//! Includes rate limiting support for tracking API usage and overage state.
//!
//! Configuration priority: environment variables > config file > defaults.
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_llm_provider::{OpenAIChatProvider, AnthropicProvider, ProviderConfig};
//!
//! let config = ProviderConfig::from_env()?;
//! let oai = OpenAIChatProvider::new(config.clone());
//! let anthropic = AnthropicProvider::new(config);
//! ```

pub mod auth;
mod config;
mod provider;
pub mod rate_limit;
pub mod registry;

/// UTF-8 safe slice helper for `tracing` macros' body/data preview
/// fields, shared by all 4 provider implementations.
pub(crate) mod util;

pub use config::ProviderConfig;
pub use provider::anthropic::AnthropicProvider;
pub use provider::gemini::GeminiProvider;
pub use provider::openai_chat::OpenAIChatProvider;
pub use provider::openai_responses::OpenAIResponsesProvider;
pub use rate_limit::{RateLimitCheck, RateLimitState, RateLimitType};
pub use registry::{build_provider_registry, AliasRouter, ConfigProviderAdapter};

/// THE single kind→provider switch. Every call site must go through here —
/// this match existed as five drifting copies (CLI, Tauri ×2, app-core
/// template spawn, registry) before PR-10 collapsed them.
///
/// `None` / `"openai-chat"` / unknown → OpenAI Chat (broadest compat path).
pub fn build_language_model(
    kind: Option<&str>,
    config: ProviderConfig,
) -> std::sync::Arc<dyn alva_kernel_abi::LanguageModel> {
    use std::sync::Arc;
    match kind {
        Some("anthropic") => Arc::new(AnthropicProvider::new(config)),
        Some("openai-responses") => Arc::new(OpenAIResponsesProvider::new(config)),
        Some("gemini") => Arc::new(GeminiProvider::new(config)),
        _ => Arc::new(OpenAIChatProvider::new(config)),
    }
}

/// THE default endpoint per provider kind. The /v1 asymmetry is load-
/// bearing: OpenAIResponsesProvider appends `/v1/responses` itself (base
/// WITHOUT /v1), OpenAIChatProvider appends `/chat/completions` (base WITH
/// /v1). One of the four former copies had responses falling through to
/// the /v1 base — producing `/v1/v1/responses`.
pub fn default_base_url(kind: Option<&str>) -> &'static str {
    match kind {
        Some("anthropic") => "https://api.anthropic.com",
        Some("gemini") => "https://generativelanguage.googleapis.com",
        Some("openai-responses") => "https://api.openai.com",
        _ => "https://api.openai.com/v1",
    }
}

#[cfg(test)]
mod factory_tests {
    use super::*;

    #[test]
    fn default_base_url_pins_the_v1_asymmetry() {
        // responses appends /v1/responses itself; chat appends
        // /chat/completions. Getting these crossed yields /v1/v1/… or a
        // missing /v1 — both 404 in production.
        assert_eq!(
            default_base_url(Some("openai-responses")),
            "https://api.openai.com"
        );
        assert_eq!(
            default_base_url(Some("openai-chat")),
            "https://api.openai.com/v1"
        );
        assert_eq!(default_base_url(None), "https://api.openai.com/v1");
        assert_eq!(
            default_base_url(Some("anthropic")),
            "https://api.anthropic.com"
        );
        assert_eq!(
            default_base_url(Some("gemini")),
            "https://generativelanguage.googleapis.com"
        );
    }

    #[test]
    fn build_language_model_dispatches_by_kind_with_chat_fallback() {
        let cfg = || ProviderConfig {
            api_key: "k".into(),
            model: "m".into(),
            base_url: "http://x".into(),
            max_tokens: 16,
            custom_headers: Default::default(),
            kind: None,
        };
        assert_eq!(
            build_language_model(Some("anthropic"), cfg()).model_id(),
            "m"
        );
        assert_eq!(build_language_model(None, cfg()).model_id(), "m");
        assert_eq!(
            build_language_model(Some("unknown-kind"), cfg()).model_id(),
            "m"
        );
    }
}
