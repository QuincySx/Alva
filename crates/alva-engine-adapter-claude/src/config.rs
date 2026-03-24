use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for the Claude Agent SDK bridge adapter.
#[derive(Debug, Clone, Default)]
pub struct ClaudeAdapterConfig {
    /// Node.js executable path (default: "node").
    pub node_path: Option<String>,

    /// Path to @anthropic-ai/claude-agent-sdk package.
    /// Default: resolved from npm global or project node_modules.
    pub sdk_package_path: Option<String>,

    /// API key. Falls back to ANTHROPIC_API_KEY env var if unset.
    pub api_key: Option<String>,

    /// Model name (e.g., "claude-sonnet-4-6").
    pub model: Option<String>,

    /// Permission mode for tool execution.
    pub permission_mode: PermissionMode,

    /// Tools to auto-approve without prompting.
    pub allowed_tools: Vec<String>,

    /// Tools to always deny.
    pub disallowed_tools: Vec<String>,

    /// Maximum budget in USD.
    pub max_budget_usd: Option<f64>,

    /// MCP server configurations.
    pub mcp_servers: HashMap<String, serde_json::Value>,

    /// Additional environment variables for the subprocess.
    pub env: HashMap<String, String>,
}

/// Permission mode for the Claude engine session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    BypassPermissions,
    Plan,
    DontAsk,
}

impl PermissionMode {
    /// Returns the SDK wire value.
    pub fn as_sdk_str(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::BypassPermissions => "bypassPermissions",
            Self::Plan => "plan",
            Self::DontAsk => "dontAsk",
        }
    }
}

/// Serializable config sent to the bridge script as JSON via process arg.
#[derive(Debug, Serialize)]
pub(crate) struct BridgeConfig {
    pub prompt: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub permission_mode: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_budget_usd: Option<f64>,
    pub mcp_servers: HashMap<String, serde_json::Value>,
    pub env: HashMap<String, String>,
    pub api_key: Option<String>,
    pub sdk_executable_path: Option<String>,
    pub system_prompt: Option<String>,
    pub streaming: bool,
}
