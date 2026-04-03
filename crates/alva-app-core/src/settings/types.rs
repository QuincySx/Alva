use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Complete settings structure matching Claude Code's settings schema
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Permission rules
    pub permissions: PermissionSettings,
    /// Environment variables to inject
    pub env: HashMap<String, String>,
    /// Custom instructions appended to system prompt
    pub system_prompt: Option<String>,
    /// Model override
    pub model: Option<String>,
    /// Whether to show verbose output
    pub verbose: bool,
    /// Theme name
    pub theme: Option<String>,
    /// Hook configurations
    pub hooks: HooksSettings,
    /// Sandbox configuration
    pub sandbox: Option<SandboxSettings>,
    /// Maximum thinking tokens
    pub max_thinking_tokens: Option<usize>,
    /// Custom API base URL
    pub api_base_url: Option<String>,
    /// Expanded view mode
    pub expand_output: bool,
    /// Trusted directories (no permission prompts)
    pub trusted_directories: Vec<PathBuf>,
    /// MCP server configurations
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PermissionSettings {
    /// Tool patterns that are always allowed
    pub allow: Vec<String>,
    /// Tool patterns that are always denied
    pub deny: Vec<String>,
    /// Tool patterns that always require asking
    pub ask: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct HooksSettings {
    /// Hooks that run before tool execution
    #[serde(rename = "PreToolUse")]
    pub pre_tool_use: Vec<HookConfig>,
    /// Hooks that run after tool execution
    #[serde(rename = "PostToolUse")]
    pub post_tool_use: Vec<HookConfig>,
    /// Hooks that run at session start
    #[serde(rename = "SessionStart")]
    pub session_start: Vec<HookConfig>,
    /// Hooks that run at session end
    #[serde(rename = "SessionEnd")]
    pub session_end: Vec<HookConfig>,
    /// Hooks that run on notification events
    #[serde(rename = "Notification")]
    pub notification: Vec<HookConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookConfig {
    /// Matcher pattern for when this hook should fire
    pub matcher: Option<String>,
    /// Hook implementations
    pub hooks: Vec<HookEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookEntry {
    /// Hook type
    #[serde(rename = "type")]
    pub hook_type: String,
    /// Command to execute
    pub command: String,
    /// Timeout in milliseconds
    pub timeout: Option<u64>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct SandboxSettings {
    pub enabled: bool,
    pub read_paths: Vec<PathBuf>,
    pub write_paths: Vec<PathBuf>,
    pub allow_network: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Server command
    pub command: String,
    /// Command arguments
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    pub env: HashMap<String, String>,
    /// Whether the server is enabled
    #[serde(default = "default_true")]
    pub enabled: bool,
}

fn default_true() -> bool {
    true
}

/// Source of a settings value
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSource {
    /// User-level: ~/.claude/settings.json
    User,
    /// Project-level: .claude/settings.json
    Project,
    /// Local (git-ignored): .claude/settings.local.json
    Local,
    /// Feature flags (remote)
    Flag,
    /// Policy (organization)
    Policy,
}
