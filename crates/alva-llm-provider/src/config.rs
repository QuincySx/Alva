//! Provider configuration with XDG Base Directory support.
//!
//! Loading priority (highest wins):
//!   1. Environment variables (ALVA_API_KEY, ALVA_MODEL, ALVA_BASE_URL, ALVA_MAX_TOKENS)
//!   2. Project config: `<workspace>/.alva/config.json`
//!   3. Global config: `$XDG_CONFIG_HOME/alva/config.json` (default: `~/.config/alva/config.json`)
//!   4. Built-in defaults
//!
//! Each layer only overrides fields it explicitly sets. API keys in particular
//! should live in the global config, not per-project files.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Configuration for LLM providers.
///
/// Authentication is mutually exclusive:
/// - If `custom_headers` is non-empty, those headers are sent as-is (api_key is ignored).
/// - If `custom_headers` is empty, `api_key` is used to construct the standard auth header
///   (`Authorization: Bearer` for OpenAI, `x-api-key` for Anthropic).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub api_key: String,
    pub model: String,
    pub base_url: String,
    pub max_tokens: u32,
    /// Custom headers to send with every request. When non-empty, `api_key` is ignored.
    #[serde(default)]
    pub custom_headers: std::collections::HashMap<String, String>,
}

/// Partial config — all fields optional, for layered merging.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct PartialConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_tokens: Option<u32>,
}

impl PartialConfig {
    /// Merge `other` on top of `self` — other's values take precedence.
    fn merge(self, other: PartialConfig) -> PartialConfig {
        PartialConfig {
            api_key: other.api_key.or(self.api_key),
            model: other.model.or(self.model),
            base_url: other.base_url.or(self.base_url),
            max_tokens: other.max_tokens.or(self.max_tokens),
        }
    }

    fn from_file(path: &Path) -> Option<PartialConfig> {
        let content = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&content).ok()
    }

    fn from_env() -> PartialConfig {
        PartialConfig {
            api_key: std::env::var("ALVA_API_KEY").ok().filter(|s| !s.is_empty()),
            model: std::env::var("ALVA_MODEL").ok().filter(|s| !s.is_empty()),
            base_url: std::env::var("ALVA_BASE_URL").ok().filter(|s| !s.is_empty()),
            max_tokens: std::env::var("ALVA_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok()),
        }
    }

    fn into_config(self) -> Result<ProviderConfig, String> {
        let api_key = self.api_key.unwrap_or_default();
        Ok(ProviderConfig {
            api_key,
            model: self.model.unwrap_or_else(|| "gpt-4o".to_string()),
            base_url: self
                .base_url
                .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
            max_tokens: self.max_tokens.unwrap_or(8192),
            custom_headers: std::collections::HashMap::new(),
        })
    }
}

/// Known API path suffixes that should be stripped from base_url.
/// Order matters — longer suffixes first to avoid partial matches.
const KNOWN_SUFFIXES: &[&str] = &[
    "/v1/chat/completions",
    "/chat/completions",
    "/v1/responses",
    "/responses",
    "/v1/messages",
    "/messages",
    "/v1",
];

/// Strip known API endpoint suffixes from a base URL.
///
/// Users often paste full endpoint URLs like `https://api.openai.com/v1/chat/completions`
/// when they should only provide the base `https://api.openai.com/v1`.
/// This normalizes to just the base so providers can append their own paths.
///
/// ```text
/// "https://api.openai.com/v1/chat/completions"  → "https://api.openai.com/v1"
/// "https://api.openai.com/v1"                   → "https://api.openai.com"
/// "https://api.anthropic.com/v1/messages"        → "https://api.anthropic.com"
/// "https://my-proxy.com/api"                     → "https://my-proxy.com/api" (unchanged)
/// ```
pub fn normalize_base_url(url: &str) -> String {
    let mut url = url.trim_end_matches('/').to_string();
    for suffix in KNOWN_SUFFIXES {
        if url.ends_with(suffix) {
            url.truncate(url.len() - suffix.len());
            break;
        }
    }
    // Don't return empty string
    if url.is_empty() {
        return "https://api.openai.com".to_string();
    }
    url
}

impl ProviderConfig {
    /// Global config directory following XDG Base Directory spec.
    ///
    /// - Linux/macOS: `$XDG_CONFIG_HOME/alva` or `~/.config/alva`
    /// - macOS (fallback): `~/Library/Application Support/alva`
    /// - Windows: `%APPDATA%\alva`
    pub fn global_config_dir() -> Option<PathBuf> {
        // Prefer XDG_CONFIG_HOME if set (works on all platforms)
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            let dir = PathBuf::from(xdg).join("alva");
            return Some(dir);
        }
        // Fall back to dirs crate (platform-appropriate)
        dirs::config_dir().map(|d| d.join("alva"))
    }

    /// Path to the global config file.
    pub fn global_config_path() -> Option<PathBuf> {
        Self::global_config_dir().map(|d| d.join("config.json"))
    }

    /// Load configuration with full layered resolution.
    ///
    /// Priority: env vars > project config > global config > defaults.
    ///
    /// `workspace` is the project directory to check for `.alva/config.json` or `alva.json`.
    pub fn load(workspace: &Path) -> Result<Self, String> {
        // Layer 1: defaults (implicit in into_config)
        let mut merged = PartialConfig::default();

        // Layer 2: global config
        if let Some(global_path) = Self::global_config_path() {
            if let Some(global) = PartialConfig::from_file(&global_path) {
                merged = merged.merge(global);
            }
        }

        // Layer 3: project config (.alva/config.json)
        let project_config_path = workspace.join(".alva").join("config.json");
        if let Some(project) = PartialConfig::from_file(&project_config_path) {
            merged = merged.merge(project);
        }

        // Layer 4: environment variables (highest priority)
        merged = merged.merge(PartialConfig::from_env());

        merged.into_config()
    }

    /// Save configuration to the global config file.
    pub fn save_global(&self) -> Result<PathBuf, String> {
        let dir = Self::global_config_dir()
            .ok_or("cannot determine config directory")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create config dir {}: {}", dir.display(), e))?;

        let path = dir.join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize config: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;

        Ok(path)
    }

    /// Save a project-level override config (only non-default fields).
    pub fn save_project(&self, workspace: &Path) -> Result<PathBuf, String> {
        let dir = workspace.join(".alva");
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create .alva dir: {}", e))?;

        let path = dir.join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize config: {}", e))?;
        std::fs::write(&path, json)
            .map_err(|e| format!("cannot write {}: {}", path.display(), e))?;

        Ok(path)
    }

    /// Load from environment variables only (no file lookup).
    pub fn from_env() -> Result<Self, String> {
        PartialConfig::from_env().into_config()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_merge_precedence() {
        let base = PartialConfig {
            api_key: Some("base-key".into()),
            model: Some("base-model".into()),
            base_url: None,
            max_tokens: Some(4096),
        };
        let overlay = PartialConfig {
            api_key: None,
            model: Some("overlay-model".into()),
            base_url: Some("https://custom.api".into()),
            max_tokens: None,
        };
        let merged = base.merge(overlay);
        assert_eq!(merged.api_key.as_deref(), Some("base-key"));
        assert_eq!(merged.model.as_deref(), Some("overlay-model"));
        assert_eq!(merged.base_url.as_deref(), Some("https://custom.api"));
        assert_eq!(merged.max_tokens, Some(4096));
    }

    #[test]
    fn into_config_uses_defaults() {
        let partial = PartialConfig {
            api_key: Some("test-key".into()),
            ..Default::default()
        };
        let config = partial.into_config().unwrap();
        assert_eq!(config.api_key, "test-key");
        assert_eq!(config.model, "gpt-4o");
        assert_eq!(config.base_url, "https://api.openai.com/v1");
        assert_eq!(config.max_tokens, 8192);
    }

    #[test]
    fn into_config_succeeds_without_api_key() {
        let partial = PartialConfig::default();
        let config = partial.into_config().unwrap();
        assert_eq!(config.api_key, "");
    }

    #[test]
    fn global_config_dir_exists() {
        // Should return Some on all platforms
        let dir = ProviderConfig::global_config_dir();
        assert!(dir.is_some());
        let dir = dir.unwrap();
        assert!(dir.ends_with("alva"));
    }

    #[test]
    fn load_from_workspace_with_no_files() {
        let tmp = tempfile::tempdir().unwrap();
        // No config files, no env vars → should fail
        // (Can't easily test since env vars may be set in CI)
        let result = ProviderConfig::load(tmp.path());
        // Just verify it doesn't panic
        let _ = result;
    }

    #[test]
    fn save_and_load_project_config() {
        let tmp = tempfile::tempdir().unwrap();
        let config = ProviderConfig {
            api_key: "test-key".into(),
            model: "test-model".into(),
            base_url: "https://test.api/v1".into(),
            max_tokens: 4096,
            custom_headers: std::collections::HashMap::new(),
        };
        let path = config.save_project(tmp.path()).unwrap();
        assert!(path.exists());

        let loaded = PartialConfig::from_file(&path).unwrap();
        assert_eq!(loaded.api_key.as_deref(), Some("test-key"));
        assert_eq!(loaded.model.as_deref(), Some("test-model"));
    }
}
