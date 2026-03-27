//! OpenAI-compatible LLM provider.
//!
//! Supports any service with an OpenAI Chat Completions API:
//! OpenAI, Anthropic (via proxy), Ollama, vLLM, DeepSeek, etc.
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
//! use alva_provider::{OpenAIProvider, ProviderConfig};
//!
//! let config = ProviderConfig::from_env()?;
//! let provider = OpenAIProvider::new(config);
//! // provider implements LanguageModel
//! ```

mod config;
mod openai;

pub use config::ProviderConfig;
pub use openai::OpenAIProvider;
