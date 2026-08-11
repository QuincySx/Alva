// INPUT:  std collections/path
// OUTPUT: PiAdapterConfig, PiProviderConfig, PiApiProtocol, PiToolPolicy
// POS:    Configuration for spawning Pi print-mode JSON sessions.

use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiToolPolicy {
    ReadOnly,
    Unrestricted,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PiApiProtocol {
    AnthropicMessages,
    OpenAiCompletions,
    OpenAiResponses,
    GoogleGenerativeAi,
}

impl PiApiProtocol {
    pub const fn as_pi_str(self) -> &'static str {
        match self {
            Self::AnthropicMessages => "anthropic-messages",
            Self::OpenAiCompletions => "openai-completions",
            Self::OpenAiResponses => "openai-responses",
            Self::GoogleGenerativeAi => "google-generative-ai",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PiProviderConfig {
    pub provider_id: String,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub protocol: PiApiProtocol,
    pub model_id: String,
    pub model_name: String,
    pub context_window: u32,
    pub max_output_tokens: u32,
    pub images: bool,
    pub reasoning: bool,
}

#[derive(Debug, Clone)]
pub struct PiAdapterConfig {
    pub pi_path: String,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub thinking: Option<String>,
    pub default_cwd: Option<PathBuf>,
    pub session_dir: PathBuf,
    pub tool_policy: PiToolPolicy,
    pub custom_provider: Option<PiProviderConfig>,
    pub env: HashMap<String, String>,
}

impl Default for PiAdapterConfig {
    fn default() -> Self {
        Self {
            pi_path: "pi".into(),
            provider: None,
            model: None,
            thinking: None,
            default_cwd: None,
            session_dir: std::env::temp_dir().join("alva-hub-pi-sessions"),
            tool_policy: PiToolPolicy::ReadOnly,
            custom_provider: None,
            env: HashMap::new(),
        }
    }
}
