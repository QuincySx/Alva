//! LLM provider implementations for alva agent framework.
//!
//! Supports multiple provider backends:
//! - **OpenAI-compatible**: Any service with an OpenAI Chat Completions API
//!   (OpenAI, Ollama, vLLM, DeepSeek, etc.)
//! - **Anthropic**: Direct Anthropic Messages API with native tool use,
//!   thinking blocks, and streaming support.
//!
//! Includes rate limiting support for tracking API usage and overage state.
//!
//! Configuration priority: environment variables > config file > defaults.
//!
//! # Environment variables
//!
//! | Variable | Description | Default |
//! |----------|-------------|---------|
//! | `ALVA_API_KEY` | API key | (required) |
//! | `ALVA_MODEL` | Model ID | `gpt-4o` |
//! | `ALVA_BASE_URL` | API base URL | `https://api.openai.com/v1` |
//! | `ALVA_MAX_TOKENS` | Max response tokens | `8192` |
//!
//! # Usage
//!
//! ```rust,ignore
//! use alva_provider::{OpenAIProvider, AnthropicProvider, ProviderConfig};
//!
//! let config = ProviderConfig::from_env()?;
//! // OpenAI-compatible provider
//! let oai = OpenAIProvider::new(config.clone());
//! // Direct Anthropic API provider
//! let anthropic = AnthropicProvider::new(config);
//! ```

mod anthropic;
mod config;
mod openai;
pub mod rate_limit;

pub use anthropic::AnthropicProvider;
pub use config::ProviderConfig;
pub use openai::OpenAIProvider;
pub use rate_limit::{RateLimitCheck, RateLimitState, RateLimitType};
