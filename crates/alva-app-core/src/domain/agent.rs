// INPUT:  serde, std::path, uuid
// OUTPUT: LLMProviderKind, LLMConfig, AgentConfig
// POS:    Defines Agent and LLM configuration entities with sensible defaults.
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// LLM provider identifier
#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LLMProviderKind {
    OpenAI,
    Anthropic,
    Gemini,
    DeepSeek,
}

/// LLM connection configuration
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LLMConfig {
    pub provider: LLMProviderKind,
    pub model: String,
    pub api_key: String,
    /// Override default base_url (for proxies / DeepSeek / Qwen)
    pub base_url: Option<String>,
    pub max_tokens: u32,
    pub temperature: Option<f32>,
}

/// Agent instance configuration
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentConfig {
    pub id: String,
    pub name: String,
    pub system_prompt: String,
    pub llm: LLMConfig,
    pub workspace: PathBuf,
    /// Tool name allowlist (None = all registered tools)
    pub allowed_tools: Option<Vec<String>>,
    /// Max loop iterations to prevent infinite loops
    pub max_iterations: u32,
    /// Token threshold to trigger context compaction (0 = disabled)
    pub compaction_threshold: u32,
}

impl Default for AgentConfig {
    fn default() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            name: "Agent".to_string(),
            system_prompt: String::new(),
            llm: LLMConfig {
                provider: LLMProviderKind::OpenAI,
                model: "gpt-4o".to_string(),
                api_key: String::new(),
                base_url: None,
                max_tokens: 8192,
                temperature: None,
            },
            workspace: PathBuf::from("."),
            allowed_tools: None,
            max_iterations: 50,
            compaction_threshold: 0,
        }
    }
}
