// INPUT:  tauri (State, AppHandle, Emitter), alva_app_core (BaseAgent + extensions),
//         alva_llm_provider, alva_kernel_abi (InMemoryAgentSession, AgentMessage, ContentBlock),
//         tokio
// OUTPUT: Tauri commands for chat sessions (send/cancel, list/create/switch/delete),
//         provider discovery, and an `agent_event` emit stream tagged with the session id.
// POS:    The bridge between the Tauri shell and `alva-app-core::BaseAgent`. One
//         BaseAgent is built lazily on first `send_message`; N in-memory sessions
//         are managed in `AppState` and swapped into the agent per turn via
//         `BaseAgent::swap_session`.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use tokio::runtime::Handle;
use tokio::sync::RwLock;

use alva_app_core::extension::{
    ApprovalExtension, BrowserExtension, CheckpointExtension, CompactionExtension, CoreExtension,
    DanglingToolCallExtension, HooksExtension, InteractionExtension, LoopDetectionExtension,
    McpExtension, PlanModeExtension, PlanningExtension, ShellExtension, SkillsExtension,
    SubAgentExtension, TaskExtension, TeamExtension, ToolTimeoutExtension, UtilityExtension,
    WebExtension,
};
use alva_app_core::{AlvaPaths, BaseAgent};
use alva_kernel_abi::agent_session::{AgentSession, EventQuery};
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::message::{AgentMessage, Message, MessageRole};
use alva_kernel_abi::LanguageModel;
use alva_llm_provider::{
    AnthropicProvider, OpenAIChatProvider, OpenAIResponsesProvider, ProviderConfig,
};

use crate::sqlite_session::{SessionSummary, SqliteEvalSession, SqliteEvalSessionManager};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

struct SessionEntry {
    info: SessionInfo,
    session: Arc<SqliteEvalSession>,
    /// Set to `true` once we've appended an `eval_config_snapshot` event
    /// to this session. The session_projection layer reads it as the source
    /// of truth for the run's configuration, so we want exactly one per
    /// session lifecycle.
    config_snapshot_appended: bool,
}

pub struct AppState {
    pub tokio: Handle,
    pub agent: RwLock<Option<Arc<BaseAgent>>>,
    /// Cache key: "provider:model:base_url|plugin_hash"
    current_agent_key: RwLock<Option<String>>,
    session_manager: Arc<SqliteEvalSessionManager>,
    /// In-memory cache of loaded session entries. The db is the source of
    /// truth for `list_sessions`; this cache keeps the Arcs alive while
    /// they're in active use (BaseAgent.swap_session needs the Arc to
    /// outlive the turn).
    sessions: RwLock<Vec<SessionEntry>>,
    active_session_id: RwLock<Option<String>>,
}

impl AppState {
    pub fn new(tokio: Handle) -> Result<Self, String> {
        let home = workspace_home()?;
        let alva_dir = home.join(".alva");
        std::fs::create_dir_all(&alva_dir)
            .map_err(|e| format!("create ~/.alva: {e}"))?;
        let db_path = alva_dir.join("sessions.db");
        let manager = SqliteEvalSessionManager::open(db_path)?;
        Ok(Self {
            tokio,
            agent: RwLock::new(None),
            current_agent_key: RwLock::new(None),
            session_manager: Arc::new(manager),
            sessions: RwLock::new(Vec::new()),
            active_session_id: RwLock::new(None),
        })
    }
}

fn summary_to_session_info(s: SessionSummary, manager: &SqliteEvalSessionManager) -> SessionInfo {
    let title = if s.preview.is_empty() {
        "New chat".to_string()
    } else {
        s.preview
    };
    let workspace_path = s.workspace_id.as_deref()
        .and_then(|wid| manager.get_workspace(wid))
        .map(|ws| ws.path);
    SessionInfo {
        id: s.session_id,
        title,
        created_at_ms: s.created_at as u64,
        updated_at_ms: s.created_at as u64,
        workspace_path,
    }
}

/// Compute the default per-session workspace path:
/// `~/.alva/workspaces/{session_id}` and create the directory if needed.
fn default_workspace_for(session_id: &str) -> Result<std::path::PathBuf, String> {
    let home = workspace_home()?;
    let path = home.join(".alva").join("workspaces").join(session_id);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create workspace dir {}: {e}", path.display()))?;
    Ok(path)
}

/// Create or find a workspace record for a path, and link it to the session.
fn link_workspace(manager: &SqliteEvalSessionManager, session_id: &str, path: &str) {
    let workspace_id = if let Some(existing) = manager.find_workspace_by_path(path) {
        existing.workspace_id
    } else {
        let id = format!("ws-{:x}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_nanos());
        manager.upsert_workspace(&crate::sqlite_session::StoredWorkspace {
            workspace_id: id.clone(),
            path: path.to_string(),
            permissions: "{}".into(),
            created_at: chrono::Utc::now().timestamp_millis(),
        });
        id
    };
    manager.set_session_workspace(session_id, &workspace_id);
}

/// Fast check: does this session already have an `eval_config_snapshot`
/// system event? Used when loading a session from disk to avoid appending
/// a duplicate snapshot on the next send.
async fn session_has_config_snapshot(session: &Arc<SqliteEvalSession>) -> bool {
    let events = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    events.iter().any(|m| {
        m.event.event_type == "system"
            && m.event
                .data
                .as_ref()
                .and_then(|d| d.get("type"))
                .and_then(|v| v.as_str())
                == Some("eval_config_snapshot")
    })
}

/// Ensure a session Arc is in the in-memory cache. Loads from db if it's
/// not already there. Returns the cached (or freshly-loaded) Arc plus a
/// best-effort `SessionInfo`.
async fn ensure_session_loaded(
    state: &State<'_, AppState>,
    id: &str,
) -> Result<Arc<SqliteEvalSession>, String> {
    {
        let sessions = state.sessions.read().await;
        if let Some(entry) = sessions.iter().find(|e| e.info.id == id) {
            return Ok(entry.session.clone());
        }
    }

    let loaded = state
        .session_manager
        .load_session(id)
        .await
        .ok_or_else(|| format!("session not found: {id}"))?;
    let snapshot_done = session_has_config_snapshot(&loaded).await;

    // Look up info from the db's sessions table so the title survives.
    let info = {
        let manager = state.session_manager.clone();
        let target = id.to_string();
        tokio::task::spawn_blocking(move || {
            let found = manager
                .list_sessions()
                .into_iter()
                .find(|s| s.session_id == target);
            found.map(|s| summary_to_session_info(s, &manager))
        })
        .await
        .ok()
        .flatten()
        .unwrap_or_else(|| SessionInfo {
            id: id.to_string(),
            title: "New chat".to_string(),
            created_at_ms: now_ms(),
            updated_at_ms: now_ms(),
            workspace_path: None,
        })
    };

    let mut sessions = state.sessions.write().await;
    // Guard against a concurrent insert racing us.
    if !sessions.iter().any(|e| e.info.id == id) {
        sessions.insert(
            0,
            SessionEntry {
                info,
                session: loaded.clone(),
                config_snapshot_appended: snapshot_done,
            },
        );
    }
    Ok(loaded)
}

// ---------------------------------------------------------------------------
// API types
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct SendMessageRequest {
    pub provider: String,
    pub model: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub system_prompt: Option<String>,
    #[serde(default)]
    pub workspace: Option<String>,
    #[serde(default)]
    pub session_id: Option<String>,
    /// Skill names the user selected for this turn. Currently logged but not
    /// wired into the agent (next batch: rebuild BaseAgent with SkillsExtension
    /// targeting these skills).
    #[serde(default)]
    pub skill_names: Option<Vec<String>>,
    /// Manual tool allow-list. `None` means "auto mode" (every tool the
    /// agent knows about is exposed to the LLM). Currently just logged —
    /// per-turn tool filtering is a future kernel enhancement.
    #[serde(default)]
    pub tool_names: Option<Vec<String>>,
    /// Deprecated — SubAgentExtension is now always registered and the
    /// `agent` tool appears in the ToolPicker like any other tool. Field
    /// kept for a release or two so older frontend builds don't 400.
    #[allow(dead_code)]
    #[serde(default)]
    pub enable_sub_agent: Option<bool>,
    pub text: String,
}

#[derive(Serialize, Clone)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub default_model: &'static str,
}

#[derive(Serialize, Clone)]
pub struct SessionInfo {
    pub id: String,
    pub title: String,
    pub created_at_ms: u64,
    pub updated_at_ms: u64,
    /// Absolute path to this session's sandbox folder. `None` only during
    /// the very brief window before `create_session` finishes seeding the
    /// default path.
    pub workspace_path: Option<String>,
}

/// A single rendered chat bubble in the Home chat area. Discriminated
/// union — the frontend renders each variant differently:
///
/// - `User` / `Assistant` / `System` → a plain bubble with text.
/// - `Thinking` → collapsible "思考" block.
/// - `ToolCall` → collapsible block with the tool name, arguments, and
///    (once the paired `tool_result` block arrives) its output + error flag.
/// - `Error` → red bubble for surfaced errors.
///
/// Projection from `Vec<AgentMessage>` happens in `messages_to_chat_entries`,
/// which walks blocks in order and merges `ToolUse` + `ToolResult` pairs
/// by their `tool_use_id`.
#[allow(dead_code)] // the Error variant is emitted from the frontend only
#[derive(Serialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatEntry {
    User {
        text: String,
    },
    Assistant {
        text: String,
    },
    System {
        text: String,
    },
    Thinking {
        text: String,
    },
    ToolCall {
        id: String,
        name: String,
        arguments: serde_json::Value,
        /// Flat text rendering of the tool output (from ToolResult). `None`
        /// means the tool is still running or the result hasn't landed.
        result: Option<String>,
        is_error: bool,
    },
    Error {
        text: String,
    },
}

// ---------------------------------------------------------------------------
// Commands — provider discovery
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_providers() -> Vec<ProviderInfo> {
    vec![
        ProviderInfo {
            id: "anthropic",
            label: "Anthropic",
            default_model: "claude-sonnet-4-6",
        },
        ProviderInfo {
            id: "openai",
            label: "OpenAI (Chat)",
            default_model: "gpt-4o",
        },
        ProviderInfo {
            id: "openai-responses",
            label: "OpenAI (Responses)",
            default_model: "gpt-4o",
        },
    ]
}

// ---------------------------------------------------------------------------
// Commands — skills & MCP discovery
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_skill_sources() -> Vec<crate::skills::SkillSourceInfo> {
    crate::skills::discover_skill_sources()
}

#[tauri::command]
pub async fn scan_skills(path: String) -> Vec<crate::skills::SkillInfo> {
    crate::skills::scan_skills(std::path::Path::new(&path)).await
}

/// Convenience: walks all standard skill sources and flattens the scan
/// results into one list. Returns empty if no source dirs exist.
#[tauri::command]
pub async fn list_all_skills() -> Vec<crate::skills::SkillInfo> {
    let sources = crate::skills::discover_skill_sources();
    let mut out = Vec::new();
    for src in sources {
        if !src.exists {
            continue;
        }
        let scanned = crate::skills::scan_skills(std::path::Path::new(&src.path)).await;
        out.extend(scanned);
    }
    out
}

#[tauri::command]
pub async fn list_mcp_servers() -> Vec<crate::mcp::McpServerInfo> {
    crate::mcp::load_mcp_servers()
}

// ---------------------------------------------------------------------------
// Commands — built-in capability catalog (tools / plugins)
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct PluginInfo {
    pub name: String,
    pub description: String,
    /// One of "tools" (Extension wrapping built-in tool groups), "system"
    /// (skills / MCP / hooks etc.), or "middleware" (cross-cutting behaviour).
    pub category: String,
    pub default_enabled: bool,
    /// Actual current enabled state (from plugins.json override, or default).
    pub enabled: bool,
    /// Tools this extension provides (empty for middleware/system-only extensions).
    pub tools: Vec<PluginToolInfo>,
}

#[derive(Serialize, Clone)]
pub struct PluginToolInfo {
    pub name: String,
    pub description: String,
}

// ---------------------------------------------------------------------------
// Plugin config persistence (~/.config/alva/plugins.json)
// ---------------------------------------------------------------------------

use std::collections::HashMap;

/// Build the default plugin enabled/disabled state.
/// This is what every new session starts with.
fn default_plugin_state() -> HashMap<String, bool> {
    let mut m = HashMap::new();
    // Core 7 always on
    m.insert("core".into(), true);
    m.insert("shell".into(), true);
    // Tool extensions — off by default, user enables what they need
    m.insert("interaction".into(), false);
    m.insert("task".into(), false);
    m.insert("team".into(), false);
    m.insert("planning".into(), false);
    m.insert("utility".into(), false);
    m.insert("web".into(), true);
    m.insert("browser".into(), true);
    // System
    m.insert("approval".into(), true);
    m.insert("skills".into(), true);
    m.insert("mcp".into(), true);
    m.insert("hooks".into(), true);
    m.insert("sub-agents".into(), true);
    m.insert("subprocess-loader".into(), true);
    m.insert("security".into(), true);
    // Middleware
    m.insert("loop-detection".into(), true);
    m.insert("dangling-tool-call".into(), true);
    m.insert("tool-timeout".into(), true);
    m.insert("compaction".into(), true);
    m.insert("checkpoint".into(), true);
    m.insert("plan-mode".into(), false);
    m
}

/// Compact fingerprint of a plugin config for cache invalidation.
fn plugin_config_hash(plugins: &HashMap<String, bool>) -> String {
    let mut pairs: Vec<_> = plugins.iter().collect();
    pairs.sort_by_key(|(k, _)| (*k).clone());
    let s: String = pairs.iter().map(|(k, v)| format!("{}={}", k, v)).collect::<Vec<_>>().join(",");
    format!("{:x}", {
        let mut h: u64 = 0;
        for b in s.bytes() { h = h.wrapping_mul(31).wrapping_add(b as u64); }
        h
    })
}

#[tauri::command]
pub async fn set_plugin_enabled(
    state: State<'_, AppState>,
    name: String,
    enabled: bool,
) -> Result<(), String> {
    let session_id = state.active_session_id.read().await.clone();
    let session_id = session_id.ok_or("no active session")?;
    let mut config = state.session_manager.get_plugin_config(&session_id);
    config.insert(name, enabled);
    state.session_manager.set_plugin_config(&session_id, &config);
    Ok(())
}

/// Collect tool names + descriptions from a preset function.
fn tools_from_preset(preset: Vec<Box<dyn alva_kernel_abi::Tool>>) -> Vec<PluginToolInfo> {
    preset
        .iter()
        .map(|t| PluginToolInfo {
            name: t.name().to_string(),
            description: t.description().to_string(),
        })
        .collect()
}

/// Catalog of installable Extensions. `enabled` reflects the active
/// session's plugin state (or `default_enabled` if no session is active).
#[tauri::command]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    use alva_agent_extension_builtin::tool_presets;

    let session_overrides: Option<HashMap<String, bool>> = {
        let sid = state.active_session_id.read().await.clone();
        if let Some(sid) = sid {
            let config = state.session_manager.get_plugin_config(&sid);
            if config.is_empty() { None } else { Some(config) }
        } else {
            None
        }
    };

    let mut plugins = vec![
        // Tool-group extensions (from alva-agent-extension-builtin wrappers)
        PluginInfo { name: "core".into(), description: "文件 IO:读/写/编辑/搜索/列出".into(), category: "tools".into(), default_enabled: true, enabled: false, tools: tools_from_preset(tool_presets::file_io()) },
        PluginInfo { name: "shell".into(), description: "Shell 命令执行".into(), category: "tools".into(), default_enabled: true, enabled: false, tools: tools_from_preset(tool_presets::shell()) },
        PluginInfo { name: "interaction".into(), description: "人工交互(ask_human)".into(), category: "tools".into(), default_enabled: false, enabled: false, tools: tools_from_preset(tool_presets::interaction()) },
        PluginInfo { name: "task".into(), description: "任务管理:创建/更新/获取/列出".into(), category: "tools".into(), default_enabled: false, enabled: false, tools: tools_from_preset(tool_presets::task_management()) },
        PluginInfo { name: "team".into(), description: "Team / 多 agent 协作".into(), category: "tools".into(), default_enabled: false, enabled: false, tools: tools_from_preset(tool_presets::team()) },
        PluginInfo { name: "planning".into(), description: "Plan 模式 + worktree 工具".into(), category: "tools".into(), default_enabled: false, enabled: false, tools: {
            let mut t = tools_from_preset(tool_presets::planning());
            t.extend(tools_from_preset(tool_presets::worktree()));
            t
        }},
        PluginInfo { name: "utility".into(), description: "工具类:config / skill / tool_search / sleep".into(), category: "tools".into(), default_enabled: false, enabled: false, tools: tools_from_preset(tool_presets::utility()) },
        PluginInfo { name: "web".into(), description: "联网搜索 + URL 抓取".into(), category: "tools".into(), default_enabled: true, enabled: false, tools: tools_from_preset(tool_presets::web()) },
        PluginInfo { name: "browser".into(), description: "浏览器自动化(Chrome CDP,7 个工具)".into(), category: "tools".into(), default_enabled: true, enabled: false, tools: tools_from_preset(alva_app_extension_browser::browser_tools()) },
        // System extensions (from alva-app-core)
        PluginInfo { name: "approval".into(), description: "人工审批流(HITL 权限确认)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "skills".into(), description: "技能发现 / 加载 / 上下文注入".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![
            PluginToolInfo { name: "search_skills".into(), description: "搜索可用技能".into() },
            PluginToolInfo { name: "use_skill".into(), description: "按名称激活技能".into() },
        ]},
        PluginInfo { name: "mcp".into(), description: "MCP 服务器集成(挂载外部工具)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![
            PluginToolInfo { name: "mcp_runtime".into(), description: "MCP 操作:list_servers / list_tools / call_tool".into() },
        ]},
        PluginInfo { name: "hooks".into(), description: "生命周期 hook(tool/session 事件上的 shell 脚本)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "sub-agents".into(), description: "子 Agent 派生(通过 agent tool)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![
            PluginToolInfo { name: "agent".into(), description: "派生子 Agent,支持角色和工具子集".into() },
        ]},
        PluginInfo { name: "subprocess-loader".into(), description: "第三方子进程插件加载(JS/Python via AEP)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![] },
        // Default extension (auto-wired by BaseAgentBuilder)
        PluginInfo { name: "security".into(), description: "沙盒安全中间件(路径过滤 + 权限闸门)".into(), category: "system".into(), default_enabled: true, enabled: false, tools: vec![] },
        // Middleware extensions
        PluginInfo { name: "loop-detection".into(), description: "检测重复 tool 调用并打破循环".into(), category: "middleware".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "dangling-tool-call".into(), description: "验证 tool 调用的格式和存在性".into(), category: "middleware".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "tool-timeout".into(), description: "每个 tool 执行 120s 超时".into(), category: "middleware".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "compaction".into(), description: "context 满时自动压缩老消息".into(), category: "middleware".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "checkpoint".into(), description: "写操作前做文件备份".into(), category: "middleware".into(), default_enabled: true, enabled: false, tools: vec![] },
        PluginInfo { name: "plan-mode".into(), description: "Plan 模式(只读 tool 限制,运行时开关)".into(), category: "middleware".into(), default_enabled: false, enabled: false, tools: vec![] },
    ];
    if let Some(ref overrides) = session_overrides {
        for p in &mut plugins {
            p.enabled = overrides.get(&p.name).copied().unwrap_or(p.default_enabled);
        }
    } else {
        for p in &mut plugins {
            p.enabled = p.default_enabled;
        }
    }
    Ok(plugins)
}

#[derive(Deserialize)]
pub struct RemoteModelsRequest {
    pub provider: String,
    pub api_key: String,
    #[serde(default)]
    pub base_url: Option<String>,
}

fn default_base_url_for(provider: &str) -> String {
    match provider {
        "anthropic" => "https://api.anthropic.com".into(),
        "openai-responses" => "https://api.openai.com".into(),
        _ => "https://api.openai.com/v1".into(),
    }
}

#[tauri::command]
pub async fn list_remote_models(
    request: RemoteModelsRequest,
) -> Result<Vec<crate::provider_api::RemoteModelInfo>, String> {
    let base = request
        .base_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_base_url_for(&request.provider));
    crate::provider_api::fetch_remote_models(&request.provider, &request.api_key, &base).await
}

#[tauri::command]
pub async fn test_provider_connection(
    request: RemoteModelsRequest,
) -> crate::provider_api::ConnectionTestResult {
    let base = request
        .base_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_base_url_for(&request.provider));
    crate::provider_api::test_connection(&request.provider, &request.api_key, &base).await
}

/// Open (or focus) the standalone Inspector window. The frontend is
/// expected to have stashed the target session id into
/// `localStorage["alva.inspector.session_id"]` before calling — Tauri
/// shares the WebContext across windows by default, so the new window
/// reads it back from the same storage on mount.
#[tauri::command]
pub async fn open_inspector_window(app: AppHandle) -> Result<(), String> {
    const LABEL: &str = "inspector";

    if let Some(existing) = app.get_webview_window(LABEL) {
        let _ = existing.set_focus();
        return Ok(());
    }

    tauri::WebviewWindowBuilder::new(
        &app,
        LABEL,
        tauri::WebviewUrl::App("inspector.html".into()),
    )
    .title("Alva Inspector")
    .inner_size(1280.0, 820.0)
    .min_inner_size(800.0, 600.0)
    .build()
    .map_err(|e| format!("open inspector window: {e}"))?;

    Ok(())
}

/// Return the full raw event log for a session as a flat JSON array.
/// Each entry is the serialized form of a `SessionEvent` (seq, uuid,
/// parent_uuid, timestamp, event_type, emitter, message, data). Used by
/// the Raw Events tab in Home — survives restart because it reads from
/// the persistent sqlite-backed session store.
#[tauri::command]
pub async fn list_session_events(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<serde_json::Value>, String> {
    let session = ensure_session_loaded(&state, &id).await?;
    let matches = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    Ok(matches
        .into_iter()
        .map(|m| serde_json::to_value(&m.event).unwrap_or(serde_json::Value::Null))
        .collect())
}

#[tauri::command]
pub async fn get_session_record(
    state: State<'_, AppState>,
    id: String,
) -> Result<serde_json::Value, String> {
    let session = ensure_session_loaded(&state, &id).await?;

    let events: Vec<alva_kernel_abi::agent_session::SessionEvent> = session
        .query(&alva_kernel_abi::agent_session::EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await
        .into_iter()
        .map(|m| m.event)
        .collect();

    let record = crate::session_projection::build_run_record(&events);
    serde_json::to_value(&record).map_err(|e| format!("serialize record: {e}"))
}

// ---------------------------------------------------------------------------
// Commands — session management
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionInfo>, String> {
    let manager = state.session_manager.clone();
    let summaries = tokio::task::spawn_blocking(move || {
        let sessions = manager.list_sessions();
        sessions.into_iter().map(|s| summary_to_session_info(s, &manager)).collect::<Vec<_>>()
    })
    .await
    .map_err(|e| format!("list_sessions join: {e}"))?;
    Ok(summaries)
}

#[tauri::command]
pub async fn create_session(state: State<'_, AppState>) -> Result<SessionInfo, String> {
    let session = state.session_manager.create_session("").await;
    let id = session.session_id().to_string();
    let now = now_ms();

    // Save default plugin config for this session
    state.session_manager.set_plugin_config(&id, &default_plugin_state());

    // Auto-provision the default sandbox folder for this session and
    // persist it onto the sessions row so it survives restart + is visible
    // via list_sessions.
    let workspace = default_workspace_for(&id)?;
    let workspace_str = workspace.to_string_lossy().to_string();
    {
        let manager = state.session_manager.clone();
        let sid = id.clone();
        let ws = workspace_str.clone();
        tokio::task::spawn_blocking(move || link_workspace(&manager, &sid, &ws))
            .await
            .ok();
    }

    let info = SessionInfo {
        id: id.clone(),
        title: "New chat".to_string(),
        created_at_ms: now,
        updated_at_ms: now,
        workspace_path: Some(workspace_str),
    };

    {
        let mut sessions = state.sessions.write().await;
        sessions.insert(
            0,
            SessionEntry {
                info: info.clone(),
                session,
                config_snapshot_appended: false,
            },
        );
    }
    *state.active_session_id.write().await = Some(id);
    Ok(info)
}

#[tauri::command]
pub async fn switch_session(
    state: State<'_, AppState>,
    id: String,
) -> Result<Vec<ChatEntry>, String> {
    let session = ensure_session_loaded(&state, &id).await?;
    *state.active_session_id.write().await = Some(id);

    // If the agent exists, swap this session into it so the next send_message
    // continues the right thread. If the agent is not built yet, the swap
    // will happen inside ensure_agent on the first send_message.
    if let Some(agent) = state.agent.read().await.clone() {
        agent.swap_session(session.clone() as Arc<dyn AgentSession>).await;
    }

    let agent_msgs = session.messages().await;
    Ok(messages_to_chat_entries(agent_msgs))
}

/// Update a session's workspace path. Only allowed **before** the first
/// user message is sent — after that, the agent has already run against
/// the old path and changing it silently would corrupt tool results.
///
/// The frontend gates its own button on `messages.length === 0`; we also
/// check here as a defence in depth by counting events with
/// `event_type == "user"` on the session's log.
#[tauri::command]
pub async fn set_session_workspace(
    state: State<'_, AppState>,
    id: String,
    path: String,
) -> Result<(), String> {
    // Make sure the session exists and load its current events.
    let session = ensure_session_loaded(&state, &id).await?;
    let events = session
        .query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        })
        .await;
    let has_started = events
        .iter()
        .any(|m| m.event.event_type == "user" || m.event.event_type == "iteration_start");
    if has_started {
        return Err(
            "对话已开始,不能再修改工作目录。新建一个任务即可选择自己的路径。".into(),
        );
    }

    // Make sure the directory exists — picker returns existing paths but
    // Rust-side we also create on custom selection so ~/.alva/workspaces
    // layout stays consistent.
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create workspace dir {path}: {e}"))?;

    // Persist to db.
    {
        let manager = state.session_manager.clone();
        let sid = id.clone();
        let p = path.clone();
        tokio::task::spawn_blocking(move || link_workspace(&manager, &sid, &p))
            .await
            .ok();
    }

    // Update in-memory cache.
    {
        let mut sessions = state.sessions.write().await;
        if let Some(entry) = sessions.iter_mut().find(|e| e.info.id == id) {
            entry.info.workspace_path = Some(path);
        }
    }

    Ok(())
}

/// Ask the OS to open a session's workspace folder in the native file
/// manager (Finder on macOS, Explorer on Windows, xdg-open on Linux).
#[tauri::command]
pub async fn open_session_workspace(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    // Look up the path from the db (source of truth) so newly-created or
    // just-switched sessions work even if the cache is stale.
    let manager = state.session_manager.clone();
    let target_id = id.clone();
    let path: String = tokio::task::spawn_blocking(move || manager.get_session_workspace_path(&target_id))
        .await
        .map_err(|e| format!("join error: {e}"))?
        .ok_or_else(|| format!("session {id} has no workspace set"))?;

    // If somehow missing (user deleted externally), recreate to avoid a
    // confusing "no such directory" popup.
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("ensure workspace dir {path}: {e}"))?;

    opener::open(&path).map_err(|e| format!("open folder {path}: {e}"))?;
    Ok(())
}

#[tauri::command]
pub async fn delete_session(state: State<'_, AppState>, id: String) -> Result<(), String> {
    // Delete from the db first — source of truth.
    let manager = state.session_manager.clone();
    let target = id.clone();
    let _ = tokio::task::spawn_blocking(move || manager.delete_session(&target)).await;

    // Drop any cached entry.
    {
        let mut sessions = state.sessions.write().await;
        sessions.retain(|e| e.info.id != id);
    }

    let mut active = state.active_session_id.write().await;
    if active.as_deref() == Some(&id) {
        // Pick the most recent remaining session from the db.
        let manager = state.session_manager.clone();
        let next: Option<String> = tokio::task::spawn_blocking(move || {
            manager.list_sessions().into_iter().next().map(|s| s.session_id)
        })
        .await
        .ok()
        .flatten();
        *active = next;
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Commands — run control
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn cancel_run(state: State<'_, AppState>) -> Result<(), String> {
    if let Some(agent) = state.agent.read().await.clone() {
        agent.cancel();
    }
    Ok(())
}

#[tauri::command]
pub async fn send_message(
    app: AppHandle,
    state: State<'_, AppState>,
    request: SendMessageRequest,
) -> Result<String, String> {
    // Resolve (or create) the session this run should write to.
    let session_id = resolve_session_for_send(&state, request.session_id.as_deref()).await?;
    let session_arc = {
        let sessions = state.sessions.read().await;
        sessions
            .iter()
            .find(|e| e.info.id == session_id)
            .map(|e| e.session.clone())
            .ok_or_else(|| format!("session vanished: {session_id}"))?
    };

    let agent = ensure_agent(&state, &request).await?;
    agent
        .swap_session(session_arc.clone() as Arc<dyn AgentSession>)
        .await;

    // Update session title from the first user message if we're still on the
    // default "New chat".
    update_title_if_default(&state, &session_id, &request.text).await;

    if let Some(skills) = &request.skill_names {
        if !skills.is_empty() {
            tracing::info!(?skills, "selected skills for turn (wiring is TODO)");
        }
    }
    if let Some(tools) = &request.tool_names {
        tracing::info!(
            manual_tools = ?tools,
            count = tools.len(),
            "manual tool allow-list for turn (filtering is TODO)"
        );
    }

    // Append an eval_config_snapshot event the first time we send a message
    // on this session so the projection layer (Inspector) has the run's
    // configuration to display.
    append_config_snapshot_if_needed(&state, &session_id, &session_arc, &agent, &request).await;

    let mut rx = agent.prompt_text(&request.text);
    let app_handle = app.clone();
    let sid_for_events = session_id.clone();
    let session_for_flush = session_arc.clone();

    state.tokio.spawn(async move {
        while let Some(event) = rx.recv().await {
            let event_value = match serde_json::to_value(&event) {
                Ok(v) => v,
                Err(e) => {
                    tracing::warn!(error = %e, "agent_event serialize failed");
                    continue;
                }
            };
            let payload = serde_json::json!({
                "session_id": sid_for_events,
                "event": event_value,
            });
            if let Err(e) = app_handle.emit("agent_event", payload) {
                tracing::warn!(error = %e, "failed to emit agent_event");
                break;
            }
        }
        let _ = app_handle.emit(
            "agent_event",
            serde_json::json!({
                "session_id": sid_for_events,
                "event": { "type": "RunChannelClosed" },
            }),
        );

        // Persist the whole session event log to sqlite now that the run
        // has ended. SqliteEvalSession uses deferred flush, so without this
        // call everything would stay in RAM and vanish on process restart.
        if let Err(e) = session_for_flush.flush().await {
            tracing::warn!(
                session_id = %sid_for_events,
                error = %e,
                "failed to flush session to disk"
            );
        }
    });

    Ok(session_id)
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

async fn resolve_session_for_send(
    state: &State<'_, AppState>,
    explicit: Option<&str>,
) -> Result<String, String> {
    if let Some(id) = explicit {
        // Make sure the session exists (in cache OR db) and loads into cache.
        ensure_session_loaded(state, id).await?;
        *state.active_session_id.write().await = Some(id.to_string());
        return Ok(id.to_string());
    }

    if let Some(id) = state.active_session_id.read().await.clone() {
        if ensure_session_loaded(state, &id).await.is_ok() {
            return Ok(id);
        }
    }

    Err("no active session — please create a new task first".into())
}

async fn append_config_snapshot_if_needed(
    state: &State<'_, AppState>,
    session_id: &str,
    session: &Arc<SqliteEvalSession>,
    agent: &BaseAgent,
    request: &SendMessageRequest,
) {
    let already = {
        let sessions = state.sessions.read().await;
        sessions
            .iter()
            .find(|e| e.info.id == session_id)
            .map(|e| e.config_snapshot_appended)
            .unwrap_or(true)
    };
    if already {
        return;
    }

    let tool_definitions = agent.tool_registry().definitions();
    let tool_names = agent.tool_names();
    let system_prompt = request
        .system_prompt
        .clone()
        .unwrap_or_else(|| "You are Alva, a helpful coding assistant.".to_string());
    let extension_names = vec![
        "core",
        "shell",
        "web",
        "loop-detection",
        "dangling-tool-call",
        "tool-timeout",
        "compaction",
    ];

    let snapshot = serde_json::json!({
        "type": "eval_config_snapshot",
        "system_prompt": system_prompt,
        "model_id": request.model.clone(),
        "tool_names": tool_names,
        "tool_definitions": tool_definitions,
        "skill_names": request.skill_names.clone().unwrap_or_default(),
        "max_iterations": 20u32,
        "extension_names": extension_names,
        "middleware_names": Vec::<String>::new(),
    });

    let event = alva_kernel_abi::agent_session::SessionEvent::system(snapshot);
    let _ = session.append(event).await;

    let mut sessions = state.sessions.write().await;
    if let Some(entry) = sessions.iter_mut().find(|e| e.info.id == session_id) {
        entry.config_snapshot_appended = true;
    }
}

async fn update_title_if_default(state: &State<'_, AppState>, id: &str, first_text: &str) {
    let mut new_title: Option<String> = None;
    {
        let mut sessions = state.sessions.write().await;
        if let Some(entry) = sessions.iter_mut().find(|e| e.info.id == id) {
            if entry.info.title == "New chat" && !first_text.trim().is_empty() {
                let title: String = first_text.trim().chars().take(40).collect();
                entry.info.title = title.clone();
                new_title = Some(title);
            }
            entry.info.updated_at_ms = now_ms();
        }
    }

    // Persist the title to the db's `preview` column so it survives restart.
    if let Some(title) = new_title {
        let manager = state.session_manager.clone();
        let session_id = id.to_string();
        tokio::task::spawn_blocking(move || manager.update_preview(&session_id, &title))
            .await
            .ok();
    }
}

async fn ensure_agent(
    state: &State<'_, AppState>,
    req: &SendMessageRequest,
) -> Result<Arc<BaseAgent>, String> {
    // Read the active session's plugin config from db
    let plugin_config: HashMap<String, bool> = {
        let sid = state.active_session_id.read().await.clone();
        if let Some(sid) = sid {
            let config = state.session_manager.get_plugin_config(&sid);
            if config.is_empty() { default_plugin_state() } else { config }
        } else {
            default_plugin_state()
        }
    };

    let agent_key = format!(
        "{}:{}:{}|{}",
        req.provider,
        req.model,
        req.base_url.as_deref().unwrap_or(""),
        plugin_config_hash(&plugin_config),
    );
    let should_rebuild = state
        .current_agent_key
        .read()
        .await
        .as_deref()
        .map(|k| k != agent_key)
        .unwrap_or(true);

    if !should_rebuild {
        if let Some(agent) = state.agent.read().await.clone() {
            return Ok(agent);
        }
    }

    let model = build_model(req)?;
    let workspace = resolve_workspace(req.workspace.as_deref())?;
    let system_prompt = req
        .system_prompt
        .clone()
        .unwrap_or_else(|| "You are Alva, a helpful coding assistant.".to_string());

    let paths = AlvaPaths::new(&workspace);
    let on = |name: &str, default: bool| -> bool {
        plugin_config.get(name).copied().unwrap_or(default)
    };

    let mut builder = BaseAgent::builder();
    builder = builder
        .workspace(&workspace)
        .system_prompt(&system_prompt)
        .max_iterations(20);

    // Core 7 tools — always-on (CoreExtension + ShellExtension)
    builder = builder
        .extension(Box::new(CoreExtension))
        .extension(Box::new(ShellExtension));

    // Conditionally registered tool extensions
    if on("interaction", false) {
        builder = builder.extension(Box::new(InteractionExtension));
    }
    if on("task", false) {
        builder = builder.extension(Box::new(TaskExtension));
    }
    if on("team", false) {
        builder = builder.extension(Box::new(TeamExtension));
    }
    if on("planning", false) {
        builder = builder.extension(Box::new(PlanningExtension));
    }
    if on("utility", false) {
        builder = builder.extension(Box::new(UtilityExtension));
    }
    if on("web", true) {
        builder = builder.extension(Box::new(WebExtension));
    }
    if on("browser", true) {
        builder = builder.extension(Box::new(BrowserExtension));
    }

    // System extensions
    if on("approval", true) {
        let (approval_ext, _approval_rx) = ApprovalExtension::with_channel();
        builder = builder.extension(Box::new(approval_ext));
    }
    if on("skills", true) {
        builder = builder.extension(Box::new(SkillsExtension::new(vec![
            paths.project_skills_dir(),
            paths.global_skills_dir(),
        ])));
    }
    if on("mcp", true) {
        builder = builder.extension(Box::new(McpExtension::new(vec![
            paths.global_mcp_config(),
            paths.project_mcp_config(),
        ])));
    }
    if on("hooks", true) {
        builder = builder.extension(Box::new(HooksExtension::new(
            alva_app_core::settings::HooksSettings::default(),
        )));
    }
    if on("sub-agents", true) {
        builder = builder.extension(Box::new(SubAgentExtension::new(3)));
    }

    // Middleware extensions
    if on("loop-detection", true) {
        builder = builder.extension(Box::new(LoopDetectionExtension));
    }
    if on("dangling-tool-call", true) {
        builder = builder.extension(Box::new(DanglingToolCallExtension));
    }
    if on("tool-timeout", true) {
        builder = builder.extension(Box::new(ToolTimeoutExtension));
    }
    if on("compaction", true) {
        builder = builder.extension(Box::new(CompactionExtension));
    }
    if on("checkpoint", true) {
        builder = builder.extension(Box::new(CheckpointExtension));
    }
    if on("plan-mode", false) {
        builder = builder.extension(Box::new(PlanModeExtension::new()));
    }

    let agent = builder
        .build(model)
        .await
        .map_err(|e| format!("build BaseAgent: {e}"))?;

    let agent = Arc::new(agent);
    *state.agent.write().await = Some(agent.clone());
    *state.current_agent_key.write().await = Some(agent_key);
    Ok(agent)
}

fn build_model(req: &SendMessageRequest) -> Result<Arc<dyn LanguageModel>, String> {
    let api_key = req
        .api_key
        .clone()
        .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
        .or_else(|| std::env::var("OPENAI_API_KEY").ok())
        .unwrap_or_default();

    if api_key.is_empty() {
        return Err("missing api_key (set it in the UI or via ANTHROPIC_API_KEY / OPENAI_API_KEY)".into());
    }

    let base_url = req.base_url.clone().unwrap_or_else(|| match req.provider.as_str() {
        "anthropic" => "https://api.anthropic.com".into(),
        "openai-responses" => "https://api.openai.com".into(),
        _ => "https://api.openai.com/v1".into(),
    });

    let config = ProviderConfig {
        api_key,
        model: req.model.clone(),
        base_url,
        max_tokens: 8192,
        custom_headers: Default::default(),
    };

    let model: Arc<dyn LanguageModel> = match req.provider.as_str() {
        "anthropic" => Arc::new(AnthropicProvider::new(config)),
        "openai-responses" => Arc::new(OpenAIResponsesProvider::new(config)),
        _ => Arc::new(OpenAIChatProvider::new(config)),
    };
    Ok(model)
}

fn resolve_workspace(requested: Option<&str>) -> Result<std::path::PathBuf, String> {
    if let Some(ws) = requested {
        let p = std::path::PathBuf::from(ws);
        if !p.is_dir() {
            return Err(format!("workspace does not exist: {ws}"));
        }
        return Ok(p);
    }
    let home = workspace_home()?;
    let default = home.join(".alva").join("workspace");
    std::fs::create_dir_all(&default).map_err(|e| format!("create workspace dir: {e}"))?;
    Ok(default)
}

fn workspace_home() -> Result<std::path::PathBuf, String> {
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(std::path::PathBuf::from(home));
    }
    if let Some(userprofile) = std::env::var_os("USERPROFILE") {
        return Ok(std::path::PathBuf::from(userprofile));
    }
    Err("cannot determine home directory".into())
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// AgentMessage → ChatMessage projection
// ---------------------------------------------------------------------------

/// Project a session's full `Vec<AgentMessage>` history into an ordered
/// list of `ChatEntry` variants ready for the Home chat renderer.
///
/// The tricky piece: `ToolUse` and `ToolResult` content blocks land in
/// *different* messages (the assistant's reply vs the subsequent user/tool
/// turn). We track tool_use_id → `ChatEntry::ToolCall` index while walking
/// and patch `result` + `is_error` when we later see the matching
/// `ToolResult` block. That preserves 1:1 ordering *and* co-locates the
/// result with its call in the output list.
fn messages_to_chat_entries(msgs: Vec<AgentMessage>) -> Vec<ChatEntry> {
    let mut entries: Vec<ChatEntry> = Vec::new();
    let mut tool_call_indices: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for am in msgs {
        let msg: Message = match am {
            AgentMessage::Standard(m) | AgentMessage::Steering(m) | AgentMessage::FollowUp(m) => m,
            // Marker / Extension variants aren't chat bubbles.
            _ => continue,
        };

        let role = msg.role.clone();

        for block in msg.content {
            match block {
                ContentBlock::Text { text } if !text.is_empty() => match role {
                    MessageRole::User => entries.push(ChatEntry::User { text }),
                    MessageRole::Assistant => entries.push(ChatEntry::Assistant { text }),
                    MessageRole::System | MessageRole::Tool => {
                        entries.push(ChatEntry::System { text })
                    }
                },
                ContentBlock::Reasoning { text } if !text.is_empty() => {
                    entries.push(ChatEntry::Thinking { text });
                }
                ContentBlock::ToolUse { id, name, input } => {
                    let idx = entries.len();
                    entries.push(ChatEntry::ToolCall {
                        id: id.clone(),
                        name,
                        arguments: input,
                        result: None,
                        is_error: false,
                    });
                    tool_call_indices.insert(id, idx);
                }
                ContentBlock::ToolResult {
                    id,
                    content,
                    is_error,
                } => {
                    // Flatten the ToolContent list to a single string for
                    // display — each ToolContent knows its own model_string.
                    let flat = content
                        .iter()
                        .map(|c| c.to_model_string())
                        .collect::<Vec<_>>()
                        .join("\n");
                    if let Some(&idx) = tool_call_indices.get(&id) {
                        if let Some(ChatEntry::ToolCall {
                            result, is_error: e, ..
                        }) = entries.get_mut(idx)
                        {
                            *result = Some(flat);
                            *e = is_error;
                        }
                    }
                }
                _ => {}
            }
        }
    }

    entries
}
