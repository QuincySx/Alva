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
    /// Which provider impl to use. Values:
    /// `"anthropic"` / `"openai-chat"` / `"openai-responses"` / `"gemini"`.
    /// `None` (or unknown value) defaults to OpenAI Chat Completions, which
    /// is the broadest OpenAI-compatible path (OpenAI / DeepSeek / Ollama / etc.).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub custom_headers: Option<std::collections::HashMap<String, String>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
}

impl PartialConfig {
    /// Merge `other` on top of `self` — other's values take precedence.
    fn merge(self, other: PartialConfig) -> PartialConfig {
        PartialConfig {
            api_key: other.api_key.or(self.api_key),
            model: other.model.or(self.model),
            base_url: other.base_url.or(self.base_url),
            max_tokens: other.max_tokens.or(self.max_tokens),
            custom_headers: other.custom_headers.or(self.custom_headers),
            kind: other.kind.or(self.kind),
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
            base_url: std::env::var("ALVA_BASE_URL")
                .ok()
                .filter(|s| !s.is_empty()),
            max_tokens: std::env::var("ALVA_MAX_TOKENS")
                .ok()
                .and_then(|s| s.parse().ok()),
            custom_headers: None,
            kind: std::env::var("ALVA_PROVIDER_KIND")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }

    fn into_config(self) -> Result<ProviderConfig, String> {
        let api_key = self.api_key.unwrap_or_default();
        let kind = self.kind;
        Ok(ProviderConfig {
            api_key,
            model: self.model.unwrap_or_else(|| "gpt-4o".to_string()),
            base_url: self
                .base_url
                .unwrap_or_else(|| crate::default_base_url(kind.as_deref()).to_string()),
            max_tokens: self.max_tokens.unwrap_or(8192),
            custom_headers: self.custom_headers.unwrap_or_default(),
            kind,
        })
    }
}

/// Write `contents` to `path` with owner-only permissions and an atomic
/// replace. These config files hold API keys, so a plain `fs::write` is
/// wrong twice: it leaves the file world-readable (default umask, typically
/// 0644), and a crash mid-write can leave a torn/truncated config. We write
/// a sibling temp file created 0600 from the start (no window where the key
/// is briefly world-readable) and `rename` it over the target — rename is
/// atomic within a filesystem, and the temp is in the same directory to
/// guarantee that.
fn write_secret_file(path: &Path, contents: &str) -> Result<(), String> {
    let dir = path
        .parent()
        .ok_or_else(|| format!("config path {} has no parent directory", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("config.json");
    let tmp = dir.join(format!(".{}.{}.tmp", file_name, std::process::id()));

    #[cfg(unix)]
    {
        use std::io::Write;
        use std::os::unix::fs::OpenOptionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&tmp)
            .map_err(|e| format!("cannot create {}: {}", tmp.display(), e))?;
        f.write_all(contents.as_bytes())
            .map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
        let _ = f.sync_all();
    }
    #[cfg(not(unix))]
    {
        std::fs::write(&tmp, contents)
            .map_err(|e| format!("cannot write {}: {}", tmp.display(), e))?;
    }

    std::fs::rename(&tmp, path).map_err(|e| {
        // Best-effort cleanup so a failed rename doesn't strand the temp.
        let _ = std::fs::remove_file(&tmp);
        format!("cannot replace {}: {}", path.display(), e)
    })?;
    Ok(())
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
        let dir = Self::global_config_dir().ok_or("cannot determine config directory")?;
        std::fs::create_dir_all(&dir)
            .map_err(|e| format!("cannot create config dir {}: {}", dir.display(), e))?;

        let path = dir.join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize config: {}", e))?;
        write_secret_file(&path, &json)?;

        Ok(path)
    }

    /// Save a project-level override config (only non-default fields).
    pub fn save_project(&self, workspace: &Path) -> Result<PathBuf, String> {
        let dir = workspace.join(".alva");
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create .alva dir: {}", e))?;

        let path = dir.join("config.json");
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| format!("cannot serialize config: {}", e))?;
        write_secret_file(&path, &json)?;

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

    #[cfg(unix)]
    #[test]
    fn write_secret_file_is_owner_only_and_atomic() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");

        write_secret_file(&path, "{\"api_key\":\"sk-secret\"}").unwrap();

        // Content is correct.
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"api_key\":\"sk-secret\"}"
        );

        // Owner-only: a config holding an API key must not be world/group
        // readable.
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "config must be 0600, got {mode:o}");

        // Atomic write leaves no temp file behind.
        let temp_left = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".tmp"));
        assert!(!temp_left, "atomic write must not strand a temp file");

        // Overwriting an existing config still ends up 0600.
        write_secret_file(&path, "{\"api_key\":\"sk-rotated\"}").unwrap();
        let mode2 = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode2, 0o600, "rewrite must stay 0600, got {mode2:o}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{\"api_key\":\"sk-rotated\"}"
        );
    }

    #[test]
    fn partial_merge_precedence() {
        let base = PartialConfig {
            api_key: Some("base-key".into()),
            model: Some("base-model".into()),
            base_url: None,
            max_tokens: Some(4096),
            custom_headers: None,
            kind: None,
        };
        let overlay = PartialConfig {
            api_key: None,
            model: Some("overlay-model".into()),
            base_url: Some("https://custom.api".into()),
            max_tokens: None,
            custom_headers: None,
            kind: None,
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
    fn into_config_uses_provider_specific_default_base_url() {
        for (kind, expected) in [
            ("anthropic", "https://api.anthropic.com"),
            ("gemini", "https://generativelanguage.googleapis.com"),
            ("openai-responses", "https://api.openai.com"),
            ("openai-chat", "https://api.openai.com/v1"),
        ] {
            let config = PartialConfig {
                kind: Some(kind.into()),
                ..Default::default()
            }
            .into_config()
            .unwrap();

            assert_eq!(config.base_url, expected, "wrong default for {kind}");
        }
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
        let custom_headers =
            std::collections::HashMap::from([("x-provider-key".into(), "secret".into())]);
        let config = ProviderConfig {
            api_key: "test-key".into(),
            model: "test-model".into(),
            base_url: "https://test.api/v1".into(),
            max_tokens: 4096,
            custom_headers: custom_headers.clone(),
            kind: None,
        };
        let path = config.save_project(tmp.path()).unwrap();
        assert!(path.exists());

        let loaded = PartialConfig::from_file(&path).unwrap();
        assert_eq!(loaded.api_key.as_deref(), Some("test-key"));
        assert_eq!(loaded.model.as_deref(), Some("test-model"));
        assert_eq!(
            loaded.into_config().unwrap().custom_headers,
            custom_headers,
            "custom headers must survive the save/load round trip"
        );
    }
}
