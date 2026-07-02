// INPUT:  serde, crate::extension::skills::skill_domain::{mcp, skill_config}
// OUTPUT: AgentTemplate, SkillSet, McpSet, GlobalSkillConfig
// POS:    Defines Agent template structure for capability-set and behavior specification.
use serde::{Deserialize, Serialize};

use crate::extension::skills::skill_domain::mcp::McpServerConfig;
use crate::extension::skills::skill_domain::skill_config::{InjectionPolicy, SkillRef};

/// Agent template: defines the capability set and behavior spec for a class of Agents
/// Corresponds to the "Agent Template Library" concept in Wukong
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTemplate {
    /// Template ID, kebab-case, e.g. "browser-agent". Defaults to the
    /// `name` when omitted in config.
    #[serde(default)]
    pub id: String,
    /// Human-readable name
    pub name: String,
    /// Template description (referenced by decision Agent for selection)
    pub description: String,

    /// Base system prompt (Skill injection appended after this)
    pub system_prompt_base: String,

    /// Skill set used by this template
    #[serde(default)]
    pub skills: SkillSet,

    /// MCP Server set used by this template
    #[serde(default)]
    pub mcp_servers: McpSet,

    /// Allowed tool name list (None = use all registered tools)
    /// Note: MCP tool names use format "mcp:<server_id>:<tool_name>"
    #[serde(default)]
    pub allowed_tools: Option<Vec<String>>,

    /// Max loop iterations (overrides engine default)
    #[serde(default)]
    pub max_iterations: Option<u32>,

    /// Optional dedicated model/endpoint for this sub-agent. `None` → the
    /// sub-agent inherits the parent agent's model.
    #[serde(default)]
    pub model: Option<AgentModelConfig>,
}

/// Per-template model + endpoint, so a sub-agent can run on its own provider.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AgentModelConfig {
    /// Model id, e.g. "qwen3.5", "claude-haiku-4.5", "gpt-4o".
    pub model: String,
    /// API endpoint / node. Empty → the provider impl's default.
    #[serde(default)]
    pub base_url: Option<String>,
    /// Provider impl: "anthropic" | "openai-chat" | "openai-responses" |
    /// "gemini". Omitted / unknown → OpenAI Chat Completions (broadest
    /// OpenAI-compatible path).
    #[serde(default)]
    pub provider_kind: Option<String>,
    /// Literal API key. Prefer `api_key_env` so secrets stay out of config.
    #[serde(default)]
    pub api_key: Option<String>,
    /// Name of an environment variable to read the API key from.
    #[serde(default)]
    pub api_key_env: Option<String>,
    /// Max output tokens (default 4096).
    #[serde(default)]
    pub max_tokens: Option<u32>,
}

/// Agent template's Skill declaration set
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SkillSet {
    /// Inherit global Skill config (enabled skills from skills.toml)
    pub inherit_global: bool,
    /// Additional Skills on top of global
    pub include: Vec<SkillRef>,
    /// Skills to exclude from global set (by name)
    pub exclude: Vec<String>,
    /// Default injection policy (individual SkillRef can override)
    pub default_injection: InjectionPolicy,
}

/// Agent template's MCP Server declaration set
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct McpSet {
    /// Inherit global MCP Server config
    pub inherit_global: bool,
    /// Additional MCP Servers (full config)
    pub include: Vec<McpServerConfig>,
    /// Servers to exclude from global set (by id)
    pub exclude: Vec<String>,
}

/// Global Skill and MCP baseline config file format
/// Corresponds to ~/.srow/skills.toml or workspace/.srow/skills.toml
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct GlobalSkillConfig {
    /// Globally enabled Skills (by name)
    pub enabled_skills: Vec<SkillRef>,
    /// Global MCP Server configs
    pub mcp_servers: Vec<McpServerConfig>,
}
