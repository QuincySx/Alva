//! Provider configuration — env vars > config file > defaults.

use serde::{Deserialize, Serialize};

/// Configuration for the OpenAI-compatible provider.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u32,
}

impl ProviderConfig {
    /// Load from environment variables. Falls back to defaults for optional fields.
    ///
    /// Returns Err if `ALVA_API_KEY` is not set.
    pub fn from_env() -> Result<Self, String> {
        let api_key = std::env::var("ALVA_API_KEY")
            .map_err(|_| "ALVA_API_KEY not set. Export it or add to .env file.".to_string())?;

        Ok(Self {
            api_key,
            model: std::env::var("ALVA_MODEL").unwrap_or_else(|_| "gpt-4o".to_string()),
            base_url: std::env::var("ALVA_BASE_URL")
                .unwrap_or_else(|_| "https://api.openai.com/v1".to_string()),
            max_tokens: std::env::var("ALVA_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(8192),
        })
    }

    /// Load from a JSON config file, then override with any env vars that are set.
    pub fn from_file_with_env(path: &str) -> Result<Self, String> {
        let mut config: Self = if std::path::Path::new(path).exists() {
            let content =
                std::fs::read_to_string(path).map_err(|e| format!("read {}: {}", path, e))?;
            serde_json::from_str(&content).map_err(|e| format!("parse {}: {}", path, e))?
        } else {
            return Self::from_env();
        };

        // Env vars override file values
        if let Ok(v) = std::env::var("ALVA_API_KEY") {
            config.api_key = v;
        }
        if let Ok(v) = std::env::var("ALVA_MODEL") {
            config.model = v;
        }
        if let Ok(v) = std::env::var("ALVA_BASE_URL") {
            config.base_url = v;
        }
        if let Ok(v) = std::env::var("ALVA_MAX_TOKENS") {
            if let Ok(n) = v.parse() {
                config.max_tokens = n;
            }
        }

        if config.api_key.is_empty() {
            return Err("api_key is empty. Set ALVA_API_KEY or add to config file.".to_string());
        }

        Ok(config)
    }
}
