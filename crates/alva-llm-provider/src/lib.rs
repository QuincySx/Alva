//! LLM provider implementations for alva agent framework.
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

pub use config::{normalize_base_url, ProviderConfig};
pub use provider::anthropic::AnthropicProvider;
pub use provider::openai_chat::OpenAIChatProvider;
pub use provider::openai_responses::OpenAIResponsesProvider;
pub use rate_limit::{RateLimitCheck, RateLimitState, RateLimitType};
