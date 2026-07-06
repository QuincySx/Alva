// INPUT:  tauri (State, AppHandle, Emitter), alva_app_core (BaseAgent + extensions),
//         alva_llm_provider, alva_kernel_abi (InMemoryAgentSession, AgentMessage, ContentBlock),
//         tokio
// OUTPUT: Tauri commands for chat sessions (send/cancel, list/create/switch/delete),
//         provider discovery, and an `agent_event` emit stream tagged with the session id.
// POS:    The bridge between the Tauri shell and `alva-app-core::BaseAgent`. One
//         BaseAgent is built lazily on first `send_message`; N in-memory sessions
//         are managed in `AppState` and swapped into the agent per turn via
//         `BaseAgent::swap_session`.
//
// This module was split from a single 1978-line `agent.rs` (PR-13a). `mod.rs`
// keeps the shared substrate — `AppState`, the cache `SessionEntry`, the DTOs
// exchanged across command groups, and the leaf helpers used by more than one
// submodule. The `#[tauri::command]` functions live in the topic submodules
// below; the command NAME is the function name (independent of module path),
// so `main.rs`'s `generate_handler!` references them via `agent::<sub>::<fn>`.

pub mod approval;
pub mod discovery;
pub mod gateway;
pub mod run;
pub mod session_cmds;

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tokio::runtime::Handle;
use tokio::sync::RwLock;

use alva_app_core::BaseAgent;

use crate::sqlite_session::{SqliteEvalSession, SqliteEvalSessionManager, SqliteSessionRegistry};

// ---------------------------------------------------------------------------
// State
// ---------------------------------------------------------------------------

pub(crate) struct SessionEntry {
    pub(crate) info: SessionInfo,
    pub(crate) session: Arc<SqliteEvalSession>,
    /// Set to `true` once we've appended an `eval_config_snapshot` event
    /// to this session. The session_projection layer reads it as the source
    /// of truth for the run's configuration, so we want exactly one per
    /// session lifecycle.
    pub(crate) config_snapshot_appended: bool,
}

/// Snapshot of a pending approval request — what the UI needs to render
/// the inline "Allow / Reject" prompt and what `respond_approval` needs
/// to dispatch the resolution back to the agent.
#[derive(Clone, Debug, Serialize)]
pub struct PendingApproval {
    pub request_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

pub struct AppState {
    pub tokio: Handle,
    pub agent: RwLock<Option<Arc<BaseAgent>>>,
    /// Cache key: "provider:model:base_url|ws=...|plugin_hash"
    pub(crate) current_agent_key: RwLock<Option<String>>,
    pub(crate) session_manager: Arc<SqliteEvalSessionManager>,
    /// SessionRegistry trait impl backed by the SAME SQLite connection as
    /// `session_manager`. Available for new code that wants the trait API
    /// (e.g., third-party extensions / future Tauri commands). The legacy
    /// `session_manager` still owns the column-tied operations
    /// (preview / plugin_config / workspace mapping) — they coexist on
    /// the same `sessions` table row.
    #[allow(dead_code)]
    pub session_registry: Arc<SqliteSessionRegistry>,
    /// In-memory cache of loaded session entries. The db is the source of
    /// truth for `list_sessions`; this cache keeps the Arcs alive while
    /// they're in active use (BaseAgent.swap_session needs the Arc to
    /// outlive the turn).
    pub(crate) sessions: RwLock<Vec<SessionEntry>>,
    pub(crate) active_session_id: RwLock<Option<String>>,
    /// Pending approval requests waiting for the user's decision. Keyed by
    /// `request_id`. Populated by the drain task spawned alongside each
    /// `ensure_agent` build; cleared by `respond_approval` once the user
    /// answers (or by the drain task on agent rebuild — receiver dies and
    /// pending entries are stale, so we clear them when a new agent
    /// starts to avoid ghost prompts).
    pub pending_approvals: Arc<RwLock<std::collections::HashMap<String, PendingApproval>>>,
    /// Abort handle for an embedded gateway instance started via
    /// `start_gateway`. `None` when no gateway is running.
    pub gateway: std::sync::Mutex<Option<tokio::task::AbortHandle>>,
    /// Serializes the turn-start critical section of `send_message`
    /// (ensure_agent → swap_session → per-turn knobs → prompt_text). The
    /// shared agent is process-global state: without this lock two
    /// concurrent send_message invocations (multi-window / double-send)
    /// interleave across the awaits in that section, and turn A ends up
    /// prompting against turn B's session and reasoning/extra_body knobs.
    pub(crate) turn_start_lock: tokio::sync::Mutex<()>,
}

impl AppState {
    pub fn new(tokio: Handle) -> Result<Self, String> {
        let home = workspace_home()?;
        let alva_dir = home.join(".alva");
        std::fs::create_dir_all(&alva_dir).map_err(|e| format!("create ~/.alva: {e}"))?;
        let db_path = alva_dir.join("sessions.db");
        let manager = SqliteEvalSessionManager::open(db_path)?;
        // Registry shares the manager's connection — both write/read the
        // same `sessions` table (legacy columns from manager, registry-
        // shaped columns from registry; one row per session).
        let registry = Arc::new(SqliteSessionRegistry::new(manager.conn().clone()));
        Ok(Self {
            tokio,
            agent: RwLock::new(None),
            current_agent_key: RwLock::new(None),
            session_manager: Arc::new(manager),
            session_registry: registry,
            sessions: RwLock::new(Vec::new()),
            active_session_id: RwLock::new(None),
            pending_approvals: Arc::new(RwLock::new(std::collections::HashMap::new())),
            gateway: std::sync::Mutex::new(None),
            turn_start_lock: tokio::sync::Mutex::new(()),
        })
    }
}

// ---------------------------------------------------------------------------
// API types shared across command groups
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
    /// wired into the agent (next batch: rebuild BaseAgent with SkillsPlugin
    /// targeting these skills).
    #[serde(default)]
    pub skill_names: Option<Vec<String>>,
    /// Manual tool allow-list. `None` means "auto mode" (every tool the
    /// agent knows about is exposed to the LLM). Currently just logged —
    /// per-turn tool filtering is a future kernel enhancement.
    #[serde(default)]
    pub tool_names: Option<Vec<String>>,
    /// Deprecated — SubAgentPlugin is now always registered and the
    /// `agent` tool appears in the ToolPicker like any other tool. Field
    /// kept for a release or two so older frontend builds don't 400.
    #[allow(dead_code)]
    #[serde(default)]
    pub enable_sub_agent: Option<bool>,
    /// Per-turn reasoning effort override. Accepts lowercase strings:
    /// `"none"` / `"minimal"` / `"low"` / `"medium"` / `"high"` / `"xhigh"`.
    /// Applies to all LLM calls within this turn (Anthropic requires a
    /// single mode per turn — don't rely on mid-iteration changes).
    /// Unknown values are ignored (no error, no override).
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    /// Resolved per-model output cap (override → API caps → fallback)
    /// computed by the frontend. Backend uses
    /// `unwrap_or(DEFAULT_MAX_OUTPUT_TOKENS)` so a missing field still
    /// produces a sane value — same default pi-mono ships with.
    #[serde(default)]
    pub max_output_tokens: Option<u32>,
    /// Free-form vendor-specific options merged into the LLM request
    /// body verbatim. Comes from the per-model override panel
    /// (Settings → 模型 → ✎ → Provider Options JSON). Last-write-wins
    /// against whatever the provider's `build_body` set.
    #[serde(default)]
    pub provider_options: Option<serde_json::Map<String, serde_json::Value>>,
    /// `true` when the user (or runtime probe) marked the active model
    /// as not supporting function calling. The backend skips all tool
    /// injection — request goes out without a `tools` field. Resolved
    /// on the frontend from `modelCaps.supports_tools` (false →
    /// disable_tools=true).
    #[serde(default)]
    pub disable_tools: Option<bool>,
    pub text: String,
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

// ---------------------------------------------------------------------------
// Shared helpers (used by more than one command group)
// ---------------------------------------------------------------------------

/// Compute the default per-session workspace path:
/// `~/.alva/workspaces/{session_id}` and create the directory if needed.
pub(crate) fn default_workspace_for(session_id: &str) -> Result<std::path::PathBuf, String> {
    let home = workspace_home()?;
    let path = home.join(".alva").join("workspaces").join(session_id);
    std::fs::create_dir_all(&path)
        .map_err(|e| format!("create workspace dir {}: {e}", path.display()))?;
    Ok(path)
}

/// Build the default plugin enabled/disabled state.
/// This is what every new session starts with.
pub(crate) fn default_plugin_state() -> HashMap<String, bool> {
    // Single source of truth: derived from the shared component catalog so the
    // UI's default toggle state matches what `apply_components` actually builds
    // (Stage C). `approval` is substrate (not in the catalog) but toggleable in
    // Tauri, so it's added explicitly, default-on.
    let mut m: HashMap<String, bool> = alva_app_core::components::COMPONENTS
        .iter()
        .map(|c| (c.id.to_string(), c.default_on))
        .collect();
    m.insert("approval".into(), true);
    m
}

pub(crate) fn workspace_home() -> Result<std::path::PathBuf, String> {
    if let Some(home) = std::env::var_os("HOME") {
        return Ok(std::path::PathBuf::from(home));
    }
    if let Some(userprofile) = std::env::var_os("USERPROFILE") {
        return Ok(std::path::PathBuf::from(userprofile));
    }
    Err("cannot determine home directory".into())
}

pub(crate) fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}
