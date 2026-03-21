// INPUT:  gpui (Context, EventEmitter), serde, serde_json, std::path, std::fs, dirs, tracing
// OUTPUT: pub struct LlmSettings, pub struct ProxySettings, pub enum ThemeMode, pub struct AppSettings, pub struct SettingsModel, pub enum SettingsModelEvent
// POS:    Persistent application settings (LLM, proxy, theme) with JSON file I/O and a GPUI reactive model wrapper.
//! Application settings — persisted to ~/.srow/settings.json

use gpui::{Context, EventEmitter};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LlmSettings {
    pub api_key: String,
    pub base_url: String,
    pub model: String,
}

impl Default for LlmSettings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProxySettings {
    pub enabled: bool,
    /// e.g. "http://127.0.0.1:7890" or "socks5://127.0.0.1:1080"
    pub url: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    Light,
    Dark,
    System,
}

impl Default for ThemeMode {
    fn default() -> Self {
        Self::System
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub llm: LlmSettings,
    pub proxy: ProxySettings,
    pub theme: ThemeMode,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            llm: LlmSettings::default(),
            proxy: ProxySettings::default(),
            theme: ThemeMode::default(),
        }
    }
}

impl AppSettings {
    fn config_dir() -> PathBuf {
        let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
        home.join(".srow")
    }

    fn config_path() -> PathBuf {
        Self::config_dir().join("settings.json")
    }

    pub fn load() -> Self {
        let path = Self::config_path();
        if path.exists() {
            match std::fs::read_to_string(&path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(settings) => return settings,
                    Err(e) => {
                        tracing::warn!("Failed to parse settings.json: {}", e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read settings.json: {}", e);
                }
            }
        }
        Self::default()
    }

    pub fn save(&self) -> Result<(), std::io::Error> {
        let dir = Self::config_dir();
        std::fs::create_dir_all(&dir)?;
        let path = Self::config_path();
        let content = serde_json::to_string_pretty(self)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;
        std::fs::write(&path, content)?;
        tracing::info!("Settings saved to {:?}", path);
        Ok(())
    }

    pub fn has_api_key(&self) -> bool {
        !self.llm.api_key.trim().is_empty()
    }
}

// -- GPUI model wrapper --

pub struct SettingsModel {
    pub settings: AppSettings,
}

pub enum SettingsModelEvent {
    SettingsChanged,
}

impl EventEmitter<SettingsModelEvent> for SettingsModel {}

impl SettingsModel {
    pub fn load() -> Self {
        Self {
            settings: AppSettings::load(),
        }
    }

    pub fn update_settings(&mut self, settings: AppSettings, cx: &mut Context<Self>) {
        if let Err(e) = settings.save() {
            tracing::error!("Failed to save settings: {}", e);
        }
        self.settings = settings;
        cx.emit(SettingsModelEvent::SettingsChanged);
        cx.notify();
    }

    pub fn has_api_key(&self) -> bool {
        self.settings.has_api_key()
    }
}

impl Default for SettingsModel {
    fn default() -> Self {
        Self::load()
    }
}
