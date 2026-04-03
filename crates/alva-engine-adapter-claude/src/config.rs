// INPUT:  std::collections::HashMap, serde::{Deserialize, Serialize}, serde_json::Value
// OUTPUT: pub struct ClaudeAdapterConfig, pub struct SandboxConfig, pub enum PermissionMode
// POS:    Defines all configuration types for the Claude Agent SDK bridge adapter.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for the Claude Agent SDK bridge adapter.
#[derive(Debug, Clone, Default)]
pub struct ClaudeAdapterConfig {
    // ── Bridge 运行环境 ──────────────────────────────────────────

    /// Node.js executable path (default: "node").
    pub node_path: Option<String>,

    /// Path to @anthropic-ai/claude-agent-sdk package.
    /// Default: resolved from npm global or project node_modules.
    pub sdk_package_path: Option<String>,

    // ── API 认证 ─────────────────────────────────────────────────

    /// API key. Falls back to ANTHROPIC_API_KEY env var if unset.
    /// 不设置时 SDK 会尝试使用本地 Claude Code 登录态。
    pub api_key: Option<String>,

    /// Custom API base URL (e.g., "https://your-proxy.com/v1").
    /// Maps to ANTHROPIC_BASE_URL env var.
    /// 不设置时使用 Anthropic 官方端点。
    pub api_base_url: Option<String>,

    // ── 模型与能力 ───────────────────────────────────────────────

    /// Model name (e.g., "claude-sonnet-4-6").
    pub model: Option<String>,

    /// Maximum budget in USD.
    pub max_budget_usd: Option<f64>,

    /// Effort level: "low", "medium", "high", "max".
    /// Controls how much reasoning Claude puts into responses.
    pub effort: Option<String>,

    // ── 权限与工具 ───────────────────────────────────────────────

    /// Permission mode for tool execution.
    pub permission_mode: PermissionMode,

    /// Tools to auto-approve without prompting.
    pub allowed_tools: Vec<String>,

    /// Tools to always deny.
    pub disallowed_tools: Vec<String>,

    // ── 执行环境 ─────────────────────────────────────────────────

    /// Enable sandbox execution.
    /// Maps to SDK `sandbox` option.
    pub sandbox: Option<SandboxConfig>,

    /// MCP server configurations.
    pub mcp_servers: HashMap<String, serde_json::Value>,

    // ── Cloud Provider 认证 ──────────────────────────────────────

    /// Use Amazon Bedrock as the API provider.
    /// Sets CLAUDE_CODE_USE_BEDROCK=1. AWS credentials must be configured in env.
    pub use_bedrock: bool,

    /// Use Google Vertex AI as the API provider.
    /// Sets CLAUDE_CODE_USE_VERTEX=1. GCP credentials must be configured in env.
    pub use_vertex: bool,

    /// Use Microsoft Azure AI Foundry as the API provider.
    /// Sets CLAUDE_CODE_USE_FOUNDRY=1. Azure credentials must be configured in env.
    pub use_azure: bool,

    // ── Agent 与 Session ─────────────────────────────────────────

    /// Enable subagent support. Include "Agent" in allowed_tools to use.
    /// Agents are defined via the `agents` field.
    pub agents: HashMap<String, serde_json::Value>,

    /// Which settings sources to load from filesystem.
    /// Options: "user", "project", "local".
    /// Empty = SDK-only mode (no filesystem settings loaded).
    pub setting_sources: Vec<String>,

    /// Persist session to disk for later resume.
    pub persist_session: Option<bool>,

    // ── 透传 ─────────────────────────────────────────────────────

    /// Additional environment variables for the subprocess.
    /// 可用于设置任意 Claude Code 支持的环境变量。
    pub env: HashMap<String, String>,
}

/// Sandbox execution configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Enable sandbox mode.
    pub enabled: bool,
    // Future: sandbox type, allowed paths, etc.
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
    pub system_prompt: Option<String>,
    pub streaming: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub max_turns: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resume_session: Option<String>,

    // API
    pub api_key: Option<String>,
    pub api_base_url: Option<String>,
    pub model: Option<String>,
    pub effort: Option<String>,
    pub max_budget_usd: Option<f64>,

    // Permissions & tools
    pub permission_mode: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,

    // Execution
    #[serde(skip_serializing_if = "Option::is_none")]
    pub sandbox: Option<SandboxConfig>,
    pub mcp_servers: HashMap<String, serde_json::Value>,

    // Agents
    #[serde(skip_serializing_if = "HashMap::is_empty")]
    pub agents: HashMap<String, serde_json::Value>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub setting_sources: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub persist_session: Option<bool>,

    // Bridge internals
    pub sdk_executable_path: Option<String>,
    pub env: HashMap<String, String>,
}
