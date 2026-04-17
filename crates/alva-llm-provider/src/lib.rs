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

pub use config::ProviderConfig;
pub use provider::anthropic::AnthropicProvider;
pub use provider::openai_chat::OpenAIChatProvider;
pub use provider::openai_responses::OpenAIResponsesProvider;
pub use rate_limit::{RateLimitCheck, RateLimitState, RateLimitType};
