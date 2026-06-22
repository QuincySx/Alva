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

#[cfg(test)]
mod tests {
    //! Tests for ClaudeAdapterConfig + PermissionMode + BridgeConfig.
    //!
    //! Two load-bearing contract families:
    //!
    //! 1. **PermissionMode wire-string consistency + spec values** —
    //!    there are TWO parallel code paths that produce SDK wire
    //!    values:
    //!    - `as_sdk_str()` (explicit match arms)
    //!    - serde `#[serde(rename_all = "camelCase")]` (derived)
    //!
    //!    Both reach the same Claude Agent SDK on the wire. The
    //!    parametric test below pins THREE things in one pass per
    //!    variant: (a) `as_sdk_str()` returns the SDK-spec literal
    //!    (e.g. `acceptEdits` NOT snake_case `accept_edits` nor
    //!    PascalCase `AcceptEdits`); (b) serde emits the same literal;
    //!    (c) both paths agree (so a refactor adding a new variant
    //!    that updates only one path is caught). CRITICAL: the
    //!    camelCase casing is mandated by the SDK protocol — a
    //!    regression to snake_case would make Claude reject the
    //!    option and silently fall back to default permission mode.
    //!
    //! 2. **BridgeConfig `skip_serializing_if` behaviors** — the
    //!    bridge protocol sends fields as JSON args; absent fields
    //!    are interpreted as "let SDK use its default". A refactor
    //!    that dropped a `skip_serializing_if` attr would start
    //!    sending `"field": null` and break SDK consumers that
    //!    distinguish "unset" from "explicitly null".
    use super::*;

    // -- PermissionMode wire pins (literal value + serde/as_sdk_str
    //    consistency) ---------------------------------------------------

    #[test]
    fn permission_mode_each_variant_emits_spec_camel_case_on_both_wire_paths() {
        // Pins three things per variant in one pass:
        //   (a) as_sdk_str() returns the SDK-spec literal
        //   (b) serde camelCase emits the same literal
        //   (c) both paths agree (catches refactors that update only
        //       one path when adding a new variant)
        //
        // Spec literals: Default→"default", AcceptEdits→"acceptEdits",
        // BypassPermissions→"bypassPermissions", Plan→"plan",
        // DontAsk→"dontAsk". A regression to snake_case (e.g.
        // "accept_edits") or PascalCase would make Claude silently
        // reject the option and fall back to default permission mode.
        let cases = [
            (PermissionMode::Default, "default"),
            (PermissionMode::AcceptEdits, "acceptEdits"),
            (PermissionMode::BypassPermissions, "bypassPermissions"),
            (PermissionMode::Plan, "plan"),
            (PermissionMode::DontAsk, "dontAsk"),
        ];
        for (variant, expected_wire) in cases {
            assert_eq!(
                variant.as_sdk_str(),
                expected_wire,
                "as_sdk_str spec value mismatch for {variant:?}"
            );
            let serde_token = serde_json::to_value(&variant).unwrap();
            let serde_str = serde_token
                .as_str()
                .expect("PermissionMode serialises as string");
            assert_eq!(
                serde_str, expected_wire,
                "serde emit mismatch for {variant:?}: got {serde_str:?}"
            );
        }
    }

    // -- PermissionMode Default impl pin --------------------------------

    #[test]
    fn permission_mode_default_impl_yields_default_variant() {
        // Pin: #[default] is on `Default`. A refactor that moved the
        // attribute to e.g. `AcceptEdits` would silently grant edit
        // permission to every Claude session that didn't opt out.
        let m: PermissionMode = Default::default();
        assert!(matches!(m, PermissionMode::Default));
    }

    // -- ClaudeAdapterConfig::default smoke ------------------------------

    #[test]
    fn claude_adapter_config_default_has_none_paths_and_default_permission() {
        // Pin the all-empty/None default state — every Option is None,
        // every collection is empty, every bool is false, permission
        // is the safe Default variant. A regression e.g. setting
        // use_bedrock=true by default would silently route every
        // session through Bedrock with no opt-in.
        let c = ClaudeAdapterConfig::default();
        assert!(c.node_path.is_none());
        assert!(c.sdk_package_path.is_none());
        assert!(c.api_key.is_none());
        assert!(c.api_base_url.is_none());
        assert!(c.model.is_none());
        assert!(c.max_budget_usd.is_none());
        assert!(c.effort.is_none());
        assert!(matches!(c.permission_mode, PermissionMode::Default));
        assert!(c.allowed_tools.is_empty());
        assert!(c.disallowed_tools.is_empty());
        assert!(c.sandbox.is_none());
        assert!(c.mcp_servers.is_empty());
        assert!(!c.use_bedrock, "use_bedrock must NOT default to true");
        assert!(!c.use_vertex, "use_vertex must NOT default to true");
        assert!(!c.use_azure, "use_azure must NOT default to true");
        assert!(c.agents.is_empty());
        assert!(c.setting_sources.is_empty());
        assert!(c.persist_session.is_none());
        assert!(c.env.is_empty());
    }

    // -- BridgeConfig skip_serializing_if pins ---------------------------

    fn empty_bridge_config() -> BridgeConfig {
        BridgeConfig {
            prompt: "".into(),
            cwd: None,
            system_prompt: None,
            streaming: false,
            max_turns: None,
            resume_session: None,
            api_key: None,
            api_base_url: None,
            model: None,
            effort: None,
            max_budget_usd: None,
            permission_mode: "default".into(),
            allowed_tools: vec![],
            disallowed_tools: vec![],
            sandbox: None,
            mcp_servers: HashMap::new(),
            agents: HashMap::new(),
            setting_sources: vec![],
            persist_session: None,
            sdk_executable_path: None,
            env: HashMap::new(),
        }
    }

    #[test]
    fn bridge_config_none_max_turns_omitted_from_json() {
        // Pin: max_turns=None must NOT appear in the JSON sent to the
        // bridge — the SDK distinguishes absent (use default) from
        // explicit null. Dropping the skip_serializing_if attr would
        // start sending `"max_turns": null` and trip SDK validation.
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("max_turns").is_none(),
            "max_turns=None must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_none_resume_session_omitted_from_json() {
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("resume_session").is_none(),
            "resume_session=None must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_none_sandbox_omitted_from_json() {
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("sandbox").is_none(),
            "sandbox=None must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_empty_agents_omitted_from_json() {
        // Pin: agents={} (empty HashMap) must be omitted via
        // skip_serializing_if = "HashMap::is_empty". Sending `"agents": {}`
        // would be interpreted by the SDK as "explicitly clear agents"
        // rather than "use default agent set".
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("agents").is_none(),
            "empty agents must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_empty_setting_sources_omitted_from_json() {
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("setting_sources").is_none(),
            "empty setting_sources must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_none_persist_session_omitted_from_json() {
        let cfg = empty_bridge_config();
        let json = serde_json::to_value(&cfg).unwrap();
        assert!(
            json.get("persist_session").is_none(),
            "persist_session=None must be omitted: {json}"
        );
    }

    #[test]
    fn bridge_config_present_values_serialize_with_field_present() {
        // Positive pin: when fields ARE set, they DO appear. Guards
        // against a refactor that accidentally moved skip_serializing_if
        // from `is_none` / `is_empty` to a stricter predicate.
        let mut cfg = empty_bridge_config();
        cfg.max_turns = Some(7);
        cfg.resume_session = Some("sess-1".into());
        cfg.agents
            .insert("worker".into(), serde_json::json!({"k": "v"}));
        cfg.setting_sources.push("user".into());
        cfg.persist_session = Some(true);
        let json = serde_json::to_value(&cfg).unwrap();
        assert_eq!(json["max_turns"], serde_json::json!(7));
        assert_eq!(json["resume_session"], serde_json::json!("sess-1"));
        assert!(json.get("agents").is_some());
        assert_eq!(json["setting_sources"], serde_json::json!(["user"]));
        assert_eq!(json["persist_session"], serde_json::json!(true));
    }

    // -- SandboxConfig serde round-trip ---------------------------------

    #[test]
    fn sandbox_config_roundtrip_preserves_enabled_flag() {
        // SandboxConfig is sent inside BridgeConfig and back; the
        // enabled flag is binary (no other fields yet). Round-trip
        // ensures derive Serialize/Deserialize stays paired.
        let cfg = SandboxConfig { enabled: true };
        let v = serde_json::to_value(&cfg).unwrap();
        assert_eq!(v, serde_json::json!({"enabled": true}));
        let back: SandboxConfig = serde_json::from_value(v).unwrap();
        assert!(back.enabled);
    }
}
