// Provider / skills / MCP discovery and the built-in capability catalog
// (tools & plugins), plus the standalone Inspector window opener. Split from
// `agent.rs` (PR-13a); command names are unchanged (= function names).

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager, State};

use super::AppState;

// ---------------------------------------------------------------------------
// Commands — provider discovery
// ---------------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub label: &'static str,
    pub default_model: &'static str,
}

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
        ProviderInfo {
            id: "gemini",
            label: "Google Gemini",
            default_model: "gemini-1.5-pro",
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
    /// Stable component id (matches `ComponentMeta::id`); used as the toggle key
    /// passed back to `set_plugin_enabled`.
    pub name: String,
    /// Human-friendly display name (`ComponentMeta::label`).
    pub label: String,
    pub description: String,
    /// The component's `category` straight from `COMPONENTS`
    /// ("tools" / "safety" / "context" / "collab" / "infra" / "ext").
    pub category: String,
    pub default_enabled: bool,
    /// Actual current enabled state (from session plugin_config override, or default).
    pub enabled: bool,
    /// Tools this component provides (empty for middleware / no-tool components).
    pub tools: Vec<PluginToolInfo>,
}

#[derive(Serialize, Clone)]
pub struct PluginToolInfo {
    pub name: String,
    pub description: String,
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
    state
        .session_manager
        .set_plugin_config(&session_id, &config);
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

/// The tool list each component contributes, keyed by `ComponentMeta::id`.
///
/// `COMPONENTS` is pure display metadata and carries no tool inventory, so the
/// rich per-component tool list is reconstructed here from the same
/// `tool_presets::*` groups (+ `browser_tools()`) that `apply_components`
/// actually attaches. Components whose tools aren't a single preset
/// (skills / mcp / sub-agents) get a small hand-curated list; pure middleware
/// and no-tool plugins get an empty vec.
fn tools_for_component(id: &str) -> Vec<PluginToolInfo> {
    use alva_agent_extension_builtin::tool_presets;
    match id {
        "core" => tools_from_preset(tool_presets::file_io()),
        "shell" => tools_from_preset(tool_presets::shell()),
        "interaction" => tools_from_preset(tool_presets::interaction()),
        "web" => tools_from_preset(tool_presets::web()),
        "utility" => tools_from_preset(tool_presets::utility()),
        "planning" => {
            let mut t = tools_from_preset(tool_presets::planning());
            t.extend(tools_from_preset(tool_presets::worktree()));
            t
        }
        "task" => tools_from_preset(tool_presets::task_management()),
        "team" => tools_from_preset(tool_presets::team()),
        "browser" => tools_from_preset(alva_app_extension_browser::browser_tools()),
        // Components whose tools aren't exposed as a static preset.
        "skills" => vec![
            PluginToolInfo {
                name: "search_skills".into(),
                description: "搜索可用技能".into(),
            },
            PluginToolInfo {
                name: "use_skill".into(),
                description: "按名称激活技能".into(),
            },
        ],
        "mcp" => vec![PluginToolInfo {
            name: "mcp_runtime".into(),
            description: "MCP 操作:list_servers / list_tools / call_tool".into(),
        }],
        "sub-agents" => vec![PluginToolInfo {
            name: "agent".into(),
            description: "派生子 Agent,支持角色和工具子集".into(),
        }],
        // Pure middleware / infra / no-tool plugins (loop-detection, compaction,
        // hooks, analytics, provider-registry, tool-lock, checkpoint,
        // subprocess-loader, permission, …).
        _ => Vec::new(),
    }
}

/// Catalog of toggleable components, derived from the shared `COMPONENTS`
/// catalog (the single source of truth). `enabled` reflects the active
/// session's plugin state (or `default_enabled` if no session is active).
#[tauri::command]
pub async fn list_plugins(state: State<'_, AppState>) -> Result<Vec<PluginInfo>, String> {
    let session_overrides: Option<HashMap<String, bool>> = {
        let sid = state.active_session_id.read().await.clone();
        if let Some(sid) = sid {
            let config = state.session_manager.get_plugin_config(&sid);
            if config.is_empty() {
                None
            } else {
                Some(config)
            }
        } else {
            None
        }
    };

    let plugins = alva_app_core::components::COMPONENTS
        .iter()
        .map(|c| {
            let enabled = session_overrides
                .as_ref()
                .and_then(|o| o.get(c.id).copied())
                .unwrap_or(c.default_on);
            PluginInfo {
                name: c.id.to_string(),
                label: c.label.to_string(),
                description: c.description.to_string(),
                category: c.category.to_string(),
                default_enabled: c.default_on,
                enabled,
                tools: tools_for_component(c.id),
            }
        })
        .collect();

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
    // Canonical table lives in alva-llm-provider (PR-10).
    alva_llm_provider::default_base_url(Some(provider)).to_string()
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

#[derive(Deserialize)]
pub struct ConnectionTestRequest {
    pub provider: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub base_url: Option<String>,
}

#[tauri::command]
pub async fn test_provider_connection(
    request: ConnectionTestRequest,
) -> crate::provider_api::ConnectionTestResult {
    let base = request
        .base_url
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| default_base_url_for(&request.provider));
    crate::provider_api::test_connection(&request.provider, &request.api_key, &base, &request.model)
        .await
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

    tauri::WebviewWindowBuilder::new(&app, LABEL, tauri::WebviewUrl::App("inspector.html".into()))
        .title("Alva Inspector")
        .inner_size(1280.0, 820.0)
        .min_inner_size(800.0, 600.0)
        .build()
        .map_err(|e| format!("open inspector window: {e}"))?;

    Ok(())
}
