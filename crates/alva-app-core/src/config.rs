// INPUT:  std::fs, std::path, serde, dirs
// OUTPUT: AlvaConfig, ProviderEntry, config_path, load, save, lookup_provider, active_provider
// POS:    Multi-provider configuration shared between alva-app-cli and
//         alva-app-tauri. Lives at `~/.alva/config.json` so both apps see
//         the same provider keys + base URLs + active selection.

//! Multi-provider config persisted at `~/.alva/config.json`.
//!
//! Schema:
//!
//! ```json
//! {
//!   "providers": {
//!     "anthropic":        { "api_key": "sk-ant-...", "model": "claude-opus-4-7", "base_url": null },
//!     "openai-chat":      { "api_key": "sk-...",     "model": "gpt-5.4",         "base_url": null },
//!     "openai-responses": { "api_key": "sk-...",     "model": "gpt-5.4",         "base_url": null },
//!     "gemini":           { "api_key": "...",        "model": "gemini-2.5-pro"                    }
//!   },
//!   "active": "anthropic"
//! }
//! ```
//!
//! Both alva-app-cli (`alva settings ...` + as a fallback in
//! `ProviderConfig::load`) and alva-app-tauri (Rust-side IPC `lookup_provider`
//! fallback when the UI didn't carry api_key/base_url) read this file. The
//! `active` field marks which provider CLI should use by default; the Tauri
//! UI tracks its own active selection in localStorage but can choose to
//! consume this field too.

use std::collections::HashMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Top-level config structure persisted at `~/.alva/config.json`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlvaConfig {
    /// Provider configurations keyed by kind: `"anthropic"`, `"openai-chat"`,
    /// `"openai-responses"`, `"gemini"`.
    #[serde(default)]
    pub providers: HashMap<String, ProviderEntry>,
    /// Which provider kind is "active" — what `alva` (no flags) uses, and
    /// what the Tauri frontend defaults to on first launch. `None` = pick
    /// the first available provider on read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active: Option<String>,
    /// CLI UI mode. `"inline"` (default) renders ratatui in a 20-row inline
    /// viewport — preserves shell scrollback, pipes-friendly, matches
    /// claude-code/aider style. `"fullscreen"` takes over the whole terminal
    /// via alternate screen (classic TUI). Override at runtime with the
    /// `--ui-mode <inline|fullscreen>` flag or `ALVA_UI_MODE` env var.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_mode: Option<String>,
    /// Inline-mode viewport height in rows. Ignored when `ui_mode` is
    /// `"fullscreen"`. Default 20 if unset.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ui_inline_rows: Option<u16>,
    /// Per-component on/off overrides, keyed by `ComponentMeta::id` (see
    /// `crate::components`). Maps directly to `ComponentToggles`. A missing
    /// id falls back to the component's `default_on`. Shared by CLI + Tauri
    /// so both apps assemble the same agent.
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub components: HashMap<String, bool>,
}

/// A single provider's auth + endpoint + default model.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProviderEntry {
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub api_key: String,
    /// Default model id (e.g. `claude-opus-4-7`). Used as fallback when
    /// the caller didn't pick one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
}

/// `~/.alva/config.json`. Returns `None` when `home_dir()` is undefined.
pub fn config_path() -> Option<PathBuf> {
    Some(dirs::home_dir()?.join(".alva").join("config.json"))
}

/// Load the config, or `None` if the file is missing / unreadable / invalid.
/// Failures log a warning but never propagate — config is best-effort.
pub fn load() -> Option<AlvaConfig> {
    let path = config_path()?;
    let body = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return None,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "alva config: read failed");
            return None;
        }
    };
    match serde_json::from_str::<AlvaConfig>(&body) {
        Ok(c) => Some(c),
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "alva config: parse failed");
            None
        }
    }
}

/// Persist the config to `~/.alva/config.json`. Pretty-printed; `~/.alva/`
/// is created if missing. Returns the canonical path written to.
pub fn save(cfg: &AlvaConfig) -> Result<PathBuf, std::io::Error> {
    let path = config_path()
        .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::NotFound, "no home dir"))?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let body = serde_json::to_string_pretty(cfg)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
    std::fs::write(&path, body)?;
    Ok(path)
}

/// Look up the entry for `provider_kind` (`"anthropic"` / `"openai-chat"` /
/// `"openai-responses"` / `"gemini"`). Returns `None` when the kind isn't
/// configured.
pub fn lookup_provider<'a>(cfg: &'a AlvaConfig, provider_kind: &str) -> Option<&'a ProviderEntry> {
    cfg.providers.get(provider_kind)
}

impl AlvaConfig {
    /// Return the (kind, entry) tuple for the active provider. Resolution:
    /// 1. `self.active` if it points at a real provider, else
    /// 2. arbitrary first provider with a non-empty api_key, else
    /// 3. arbitrary first provider, else
    /// 4. `None`.
    pub fn active_provider(&self) -> Option<(&str, &ProviderEntry)> {
        if let Some(active_kind) = self.active.as_deref() {
            if let Some(entry) = self.providers.get(active_kind) {
                return Some((active_kind, entry));
            }
        }
        if let Some((kind, entry)) = self
            .providers
            .iter()
            .find(|(_, e)| !e.api_key.is_empty())
        {
            return Some((kind.as_str(), entry));
        }
        self.providers
            .iter()
            .next()
            .map(|(k, v)| (k.as_str(), v))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_multi_provider_schema() {
        let json = r#"{
            "providers": {
                "anthropic": { "api_key": "sk-ant-XXX" },
                "openai-chat": { "api_key": "sk-YYY", "base_url": "https://example/v1" }
            },
            "active": "openai-chat"
        }"#;
        let cfg: AlvaConfig = serde_json::from_str(json).unwrap();
        assert_eq!(cfg.active.as_deref(), Some("openai-chat"));
        let (k, _) = cfg.active_provider().unwrap();
        assert_eq!(k, "openai-chat");
    }

    #[test]
    fn empty_object_parses_to_default() {
        let cfg: AlvaConfig = serde_json::from_str("{}").unwrap();
        assert!(cfg.providers.is_empty());
        assert!(cfg.active.is_none());
        assert!(cfg.active_provider().is_none());
    }

    #[test]
    fn active_falls_back_to_first_with_key() {
        let mut cfg = AlvaConfig::default();
        cfg.providers.insert("a".into(), ProviderEntry { api_key: String::new(), ..Default::default() });
        cfg.providers.insert("b".into(), ProviderEntry { api_key: "k".into(), ..Default::default() });
        let (k, _) = cfg.active_provider().unwrap();
        assert_eq!(k, "b");
    }
}
