// INPUT:  gpui + gpui-component, migrated hub-gpui backend (store/run/readiness)
// OUTPUT: HubApp — the full Hub Desktop surface (sidebar / topbar / timeline /
//         composer), assembled from per-panel impl blocks in sibling modules.
// POS:    1:1 GPUI port of crates/alva-hub-desktop/web/src/App.tsx.
//         Follows crates/alva-hub-gpui/AGENTS.md: Entity<State> for stateful
//         components, theme colors only, no hardcoded hex, snapshot fields
//         in render callbacks, weak refs in closures.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alva_engine_runtime::{EngineInspector, EngineRuntime, InspectionSnapshot};
use chrono::{Local, Offset as _, Utc};
use gpui::*;
use gpui_component::{
    input::{InputEvent, InputState},
    searchable_list::SearchableVec,
    select::SelectState,
    ActiveTheme, *,
};

use alva_hub_gpui::{
    build_engine_bundle_for_target_with_provider, permission_eligibility, probe_readiness,
    protocol_supports_runtime, repair_argv, repair_plan, run_persisted_with_target,
    run_repair_command, AgentRuntime, ApiProtocol, AutomationDefinition, AutomationRunStatus,
    AutomationSchedule, AutomationStore, CustomModelProfile, CustomProviderProfile,
    EngineEligibility, EngineReadiness, EngineTarget, FallbackPolicy, HubSessionDetail,
    HubSessionSummary, HubStore, InstalledPlugin, InstalledSkill, ModelCatalog, ModelRoute,
    PermissionProfile, PermissionRouter, PluginStore, ProviderStore, ReasoningEffort, RepairAction,
    RepairKind, RunRequest, SkillCatalog, StoredCustomProvider, SwitchOutcome, SystemProbe,
    TimelineEvent, TimelineStatus,
};

use super::workspace::{HubWorkspace, ToolPanel, WorkspaceTool};
use super::{profile_label, EffortOption, EngineOption, ModelOption, WorkerOption};
use alva_hub_gpui::{project_chat, project_timeline, ProjectedChatItem, ProjectedEvent};

const PROFILE_OPTIONS: [&str; 3] = ["interactive", "workspace_write", "unrestricted"];

fn route_key(route: &ModelRoute) -> String {
    format!("{}::{}", route.provider_id, route.model_id)
}

fn now_ms() -> i64 {
    Utc::now().timestamp_millis()
}

fn local_offset_seconds() -> i32 {
    Local::now().offset().fix().local_minus_utc()
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PrimarySurface {
    #[default]
    Chat,
    Automation,
    Plugins,
    Settings,
}

pub struct HubApp {
    pub store: Arc<HubStore>,
    /// Live engines keyed by product session id, so Stop can cancel the
    /// adapter currently streaming (React/Tauri `active_engines`).
    pub active_engines: Arc<Mutex<HashMap<String, Arc<dyn EngineRuntime>>>>,
    pub active_inspectors: Arc<Mutex<HashMap<String, Arc<dyn EngineInspector>>>>,
    /// Pending approval routing; run emits register requests here.
    pub permission_router: Arc<PermissionRouter>,
    /// Handle of this app's single window, so async contexts can reach the
    /// window for stateful-component updates.
    pub window_handle: AnyWindowHandle,
    pub workspace_dock: Option<WeakEntity<gpui_component::dock::DockArea>>,
    pub workspace_tool_panels: HashMap<WorkspaceTool, WeakEntity<ToolPanel>>,
    pub workspace_open_tools: Vec<WorkspaceTool>,
    pub primary_surface: PrimarySurface,
    pub sidebar_search_open: bool,
    pub workspace: Option<String>,
    pub provider_store: ProviderStore,
    pub custom_providers: Vec<StoredCustomProvider>,
    pub model_catalog: ModelCatalog,
    pub automation_store: AutomationStore,
    pub automations: Vec<AutomationDefinition>,
    pub plugin_store: PluginStore,
    pub plugins: Vec<InstalledPlugin>,
    pub skill_catalog: SkillCatalog,
    pub skills: Vec<InstalledSkill>,
    pub skills_loading: bool,
    pub engine: Entity<SelectState<SearchableVec<EngineOption>>>,
    pub model: Entity<SelectState<SearchableVec<ModelOption>>>,
    pub effort: Entity<SelectState<SearchableVec<EffortOption>>>,
    pub model_query: Entity<InputState>,
    pub permission: Entity<SelectState<SearchableVec<&'static str>>>,
    pub prompt: Entity<InputState>,
    pub automation_name: Entity<InputState>,
    pub automation_prompt: Entity<InputState>,
    pub automation_schedule: Entity<SelectState<SearchableVec<&'static str>>>,
    pub extension_query: Entity<InputState>,
    pub extension_tab: usize,
    pub provider_name: Entity<InputState>,
    pub provider_id: Entity<InputState>,
    pub provider_base_url: Entity<InputState>,
    pub provider_api_key: Entity<InputState>,
    pub provider_model_id: Entity<InputState>,
    pub provider_model_name: Entity<InputState>,
    pub provider_context_window: Entity<InputState>,
    pub provider_max_output: Entity<InputState>,
    pub provider_protocol: Entity<SelectState<SearchableVec<&'static str>>>,
    pub provider_runtimes: HashSet<AgentRuntime>,
    pub provider_images: bool,
    pub provider_tools: bool,
    pub timeline: Vec<ProjectedEvent>,
    pub chat: Vec<ProjectedChatItem>,
    pub inspector_enabled: bool,
    pub inspector_snapshot: Option<InspectionSnapshot>,
    pub inspector_error: Option<String>,
    pub inspector_loading: bool,
    pub inspector_generation: u64,
    pub inspector_session_stack: Vec<String>,
    pub inspector_open_records: HashSet<u64>,
    /// Timeline index → handoff card expanded (keyed by index like React).
    pub open_handoffs: HashMap<usize, bool>,
    pub open_activities: HashSet<usize>,
    // Sessions surface.
    pub sessions: Vec<HubSessionSummary>,
    pub session_query: Entity<InputState>,
    pub active_session_id: Option<String>,
    pub session_detail: Option<HubSessionDetail>,
    pub session_error: Option<String>,
    // Topbar / engine surface.
    pub pending_engine: Option<String>,
    pub transcript_sharing: bool,
    pub eligibility: Vec<EngineEligibility>,
    pub readiness: Vec<EngineReadiness>,
    pub readiness_busy: bool,
    pub repair_plan: Vec<RepairAction>,
    pub repair_logs: HashMap<String, String>,
    pub repairing_engine: Option<String>,
    pub busy: bool,
    // Fallback policy (session or default) — editable in the sidebar.
    pub fallback_policy: FallbackPolicy,
    #[allow(dead_code)]
    pub policy_saving: bool,
    /// Active team run for the open lead session.
    pub team_run: Option<alva_hub_gpui::TeamRunDetail>,
    /// Sidebar sections the user can collapse (fallback/team start collapsed).
    pub collapsed: HashMap<String, bool>,
    /// Per-target model/provider inputs, aligned with `fallback_policy.targets`.
    pub policy_model_inputs: Vec<Entity<InputState>>,
    pub policy_provider_inputs: Vec<Entity<InputState>>,
    // Team surface.
    pub team_worker_engine: Entity<SelectState<SearchableVec<String>>>,
    pub dispatch_worker: Entity<SelectState<SearchableVec<WorkerOption>>>,
    pub team_scope: Entity<InputState>,
    pub team_claims: Entity<InputState>,
    pub editing_scope: Entity<InputState>,
    pub isolate_worker: bool,
    pub show_archived: bool,
    pub editing_task: Option<String>,
    pub adoption: HashMap<String, Option<alva_hub_gpui::AdoptionPreview>>,
    /// Worker ids whose queue pump is currently running.
    #[allow(dead_code)]
    pub worker_pumps: Arc<Mutex<HashSet<String>>>,
    pub worktree_root: PathBuf,
    /// Details drawer surface.
    pub memory_draft: Entity<InputState>,
    /// Drawer handoff envelopes expanded by handoff id.
    pub open_drawer_handoffs: HashMap<String, bool>,
    /// Merged approval cards (multi-member groups), refreshed on events.
    pub approval_groups: Vec<alva_hub_gpui::PermissionGroup>,
    /// request_id → answered, so an answered card can never re-send.
    pub answered_permissions: HashMap<String, bool>,
}

/// Run a closure against the window's root view (the HubApp) from any
/// AppContext, where `&mut Window` is available for stateful-component
/// updates (Select/Input).
pub fn with_window<C, F>(cx: &mut C, handle: AnyWindowHandle, f: F)
where
    C: AppContext,
    F: FnOnce(&mut HubApp, &mut Window, &mut Context<HubApp>) + 'static,
{
    let _ = handle.update(cx, |root, window, cx| {
        if let Ok(workspace) = root.clone().downcast::<HubWorkspace>() {
            let hub = workspace.read(cx).hub();
            hub.update(cx, |hub, cx| f(hub, window, cx));
        } else if let Ok(view) = root.downcast::<HubApp>() {
            view.update(cx, |hub, cx| f(hub, window, cx));
        }
    });
}

impl HubApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let store_path = std::env::var("ALVA_HUB_DB").unwrap_or_else(|_| {
            let home = std::env::var("HOME").unwrap_or_else(|_| "/tmp".into());
            format!("{home}/Library/Application Support/com.alva.hub-gpui/alva-hub.db")
        });
        let store = Arc::new(HubStore::open(&store_path).expect("open Hub store"));
        let worktree_root = alva_hub_gpui::worktree_root(
            store
                .path()
                .parent()
                .unwrap_or(std::path::Path::new("/tmp")),
        );
        let catalog_workspace = store
            .path()
            .parent()
            .unwrap_or(std::path::Path::new("/tmp"));
        let provider_store = ProviderStore::new(catalog_workspace);
        let custom_providers = provider_store.load().unwrap_or_default();
        let model_catalog = ModelCatalog::load(catalog_workspace, &custom_providers);
        let automation_store = AutomationStore::new(catalog_workspace);
        let mut automations = automation_store.load().unwrap_or_default();
        for automation in &mut automations {
            if automation.last_status == AutomationRunStatus::Running {
                automation.last_status = AutomationRunStatus::Failed;
                automation.last_error =
                    Some("ALVA Hub closed before this automation reached a terminal state.".into());
            }
        }
        let _ = automation_store.save(&automations);
        let plugin_store = PluginStore::new(&catalog_workspace.join("conversations"));
        let plugins = plugin_store.scan().unwrap_or_default();
        let skill_catalog = SkillCatalog::new(&catalog_workspace.join("conversations"));
        let initial_route = model_catalog
            .default_route(AgentRuntime::Alva)
            .or_else(|| model_catalog.default_route(AgentRuntime::ClaudeCode))
            .expect("built-in model catalog must expose at least one route");
        let initial_engine_index = alva_hub_gpui::HUB_ENGINE_IDS
            .iter()
            .position(|engine| *engine == initial_route.runtime.id());
        let initial_models = Self::model_options(&model_catalog, initial_route.runtime);
        let initial_model_index = initial_models
            .iter()
            .position(|option| option.route_key == route_key(&initial_route));
        let initial_efforts = Self::effort_options(&model_catalog, &initial_route);
        let initial_effort_index = initial_efforts
            .iter()
            .position(|option| option.effort == initial_route.reasoning_effort);
        let sessions = store.list_sessions().unwrap_or_default();
        let fallback_policy = store
            .global_fallback_policy()
            .unwrap_or_else(|_| FallbackPolicy {
                targets: Vec::new(),
                updated_at: 0,
            });
        let inspector_enabled = store.preference_bool("inspector_enabled").unwrap_or(false);
        let eligibility = permission_eligibility(PermissionProfile::WorkspaceWrite);
        let mut app = Self {
            store,
            active_engines: Arc::new(Mutex::new(HashMap::new())),
            active_inspectors: Arc::new(Mutex::new(HashMap::new())),
            permission_router: Arc::new(PermissionRouter::new()),
            window_handle: window.window_handle(),
            workspace_dock: None,
            workspace_tool_panels: HashMap::new(),
            workspace_open_tools: Vec::new(),
            primary_surface: PrimarySurface::Chat,
            sidebar_search_open: false,
            workspace: None,
            provider_store,
            custom_providers,
            model_catalog,
            automation_store,
            automations,
            plugin_store,
            plugins,
            skill_catalog,
            skills: Vec::new(),
            skills_loading: false,
            timeline: Vec::new(),
            chat: Vec::new(),
            inspector_enabled,
            inspector_snapshot: None,
            inspector_error: None,
            inspector_loading: false,
            inspector_generation: 0,
            inspector_session_stack: Vec::new(),
            inspector_open_records: HashSet::new(),
            open_handoffs: HashMap::new(),
            open_activities: HashSet::new(),
            sessions,
            session_query: cx
                .new(|cx| InputState::new(window, cx).placeholder("Search workspace or prompt…")),
            active_session_id: None,
            session_detail: None,
            session_error: None,
            pending_engine: None,
            transcript_sharing: false,
            eligibility,
            readiness: Vec::new(),
            readiness_busy: false,
            repair_plan: Vec::new(),
            repair_logs: HashMap::new(),
            repairing_engine: None,
            busy: false,
            fallback_policy,
            policy_saving: false,
            team_run: None,
            collapsed: HashMap::from([("fallback".to_string(), true), ("team".to_string(), true)]),
            policy_model_inputs: Vec::new(),
            policy_provider_inputs: Vec::new(),
            team_worker_engine: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec![
                        "codex".to_string(),
                        "alva".to_string(),
                        "claude".to_string(),
                    ]),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            dispatch_worker: cx
                .new(|cx| SelectState::new(SearchableVec::new(Vec::new()), None, window, cx)),
            team_scope: cx.new(|cx| {
                InputState::new(window, cx)
                    .multi_line(true)
                    .placeholder("Task to queue — or a message to the worker…")
            }),
            team_claims: cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder("Claims: src/lib.rs, read:docs, facility:target-dir")
            }),
            editing_scope: cx.new(|cx| InputState::new(window, cx).multi_line(true)),
            isolate_worker: false,
            show_archived: false,
            editing_task: None,
            adoption: HashMap::new(),
            worker_pumps: Arc::new(Mutex::new(HashSet::new())),
            worktree_root,
            memory_draft: cx.new(|cx| {
                InputState::new(window, cx).placeholder("Always run cargo test before finishing…")
            }),
            open_drawer_handoffs: HashMap::new(),
            approval_groups: Vec::new(),
            answered_permissions: HashMap::new(),
            engine: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(Self::engine_options(&[])),
                    initial_engine_index.map(IndexPath::new),
                    window,
                    cx,
                )
            }),
            model: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(initial_models),
                    initial_model_index.map(IndexPath::new),
                    window,
                    cx,
                )
            }),
            effort: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(initial_efforts),
                    initial_effort_index.map(IndexPath::new),
                    window,
                    cx,
                )
            }),
            model_query: cx.new(|cx| InputState::new(window, cx).placeholder("Search models…")),
            permission: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(PROFILE_OPTIONS.to_vec()),
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            }),
            prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .auto_grow(3, 8)
                    .placeholder("Ask Alva anything…")
            }),
            automation_name: cx
                .new(|cx| InputState::new(window, cx).placeholder("Nightly self-healing tests")),
            automation_prompt: cx.new(|cx| {
                InputState::new(window, cx)
                    .auto_grow(4, 10)
                    .placeholder("Describe the work this automation should perform…")
            }),
            automation_schedule: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(
                        AutomationSchedule::ALL
                            .into_iter()
                            .map(AutomationSchedule::label)
                            .collect::<Vec<_>>(),
                    ),
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
            extension_query: cx
                .new(|cx| InputState::new(window, cx).placeholder("Search plugins or skills…")),
            extension_tab: 0,
            provider_name: cx.new(|cx| InputState::new(window, cx).placeholder("Acme AI")),
            provider_id: cx.new(|cx| InputState::new(window, cx).placeholder("acme")),
            provider_base_url: cx
                .new(|cx| InputState::new(window, cx).placeholder("https://api.example.com/v1")),
            provider_api_key: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder("API key")
            }),
            provider_model_id: cx.new(|cx| InputState::new(window, cx).placeholder("model-id")),
            provider_model_name: cx
                .new(|cx| InputState::new(window, cx).placeholder("Model display name")),
            provider_context_window: cx
                .new(|cx| InputState::new(window, cx).default_value("128000")),
            provider_max_output: cx.new(|cx| InputState::new(window, cx).default_value("32000")),
            provider_protocol: cx.new(|cx| {
                SelectState::new(
                    SearchableVec::new(vec![
                        "anthropic",
                        "openai_responses",
                        "openai_chat",
                        "gemini",
                    ]),
                    Some(IndexPath::new(1)),
                    window,
                    cx,
                )
            }),
            provider_runtimes: HashSet::from([AgentRuntime::Alva]),
            provider_images: true,
            provider_tools: true,
        };
        app.subscribe_sessions(window, cx);
        app.subscribe_extension_query(window, cx);
        app.subscribe_engine(window, cx);
        app.subscribe_model(window, cx);
        app.subscribe_effort(window, cx);
        app.subscribe_model_query(window, cx);
        app.subscribe_prompt(window, cx);
        app.sync_policy_inputs(window, cx);
        app.refresh_readiness(cx);
        app.refresh_skills(cx);
        app.start_automation_scheduler(cx);
        app
    }

    fn engine_options(readiness: &[EngineReadiness]) -> Vec<EngineOption> {
        alva_hub_gpui::HUB_ENGINE_IDS
            .iter()
            .map(|id| {
                let display = match *id {
                    "alva" => "Alva",
                    "claude" => "Claude Code",
                    "codex" => "Codex",
                    "pi" => "Pi",
                    _ => id,
                };
                let suffix = readiness
                    .iter()
                    .find(|report| report.engine_id == *id)
                    .map(|report| match report.status {
                        alva_hub_gpui::ReadinessStatus::Healthy => String::new(),
                        alva_hub_gpui::ReadinessStatus::Missing => " — Not installed".to_string(),
                        alva_hub_gpui::ReadinessStatus::Incompatible => {
                            " — Incompatible".to_string()
                        }
                        alva_hub_gpui::ReadinessStatus::Unauthenticated => {
                            " — Sign-in needed".to_string()
                        }
                        alva_hub_gpui::ReadinessStatus::Unhealthy => " — Unhealthy".to_string(),
                    })
                    .unwrap_or_default();
                EngineOption {
                    engine_id: (*id).to_string(),
                    label: format!("{display}{suffix}"),
                    enabled: true,
                }
            })
            .collect()
    }

    fn model_options(catalog: &ModelCatalog, runtime: AgentRuntime) -> Vec<ModelOption> {
        catalog
            .models_for_runtime(runtime)
            .into_iter()
            .map(|model| {
                let provider = catalog
                    .provider(&model.provider_id)
                    .expect("catalog model provider must exist");
                ModelOption {
                    route_key: format!("{}::{}", model.provider_id, model.model_id),
                    label: model.display_name.clone(),
                    metadata: format!(
                        "{} · {} · {}k context{}{}",
                        provider.display_name,
                        provider.protocol.label(),
                        model.capabilities.context_window / 1_000,
                        if model.capabilities.images {
                            " · images"
                        } else {
                            ""
                        },
                        if model.capabilities.reasoning_efforts.is_empty() {
                            ""
                        } else {
                            " · adjustable reasoning"
                        }
                    ),
                    enabled: provider.connected && model.enabled,
                }
            })
            .collect()
    }

    fn effort_options(catalog: &ModelCatalog, route: &ModelRoute) -> Vec<EffortOption> {
        let Some(model) = catalog.model(&route.provider_id, &route.model_id) else {
            return vec![EffortOption {
                effort: None,
                value: "default".into(),
                label: "Default reasoning".into(),
            }];
        };
        if model.capabilities.reasoning_efforts.is_empty() {
            return vec![EffortOption {
                effort: None,
                value: "default".into(),
                label: "Model default".into(),
            }];
        }
        model
            .capabilities
            .reasoning_efforts
            .iter()
            .copied()
            .map(|effort| EffortOption {
                effort: Some(effort),
                value: effort.id().to_string(),
                label: format!("{} reasoning", effort.label()),
            })
            .collect()
    }

    pub fn selected_model_route(&self, cx: &App) -> Result<ModelRoute, String> {
        let runtime = AgentRuntime::parse(&self.selected_engine(cx))
            .ok_or_else(|| "selected runtime is unknown".to_string())?;
        let route_key = self
            .model
            .read(cx)
            .selected_value()
            .map(ToString::to_string)
            .ok_or_else(|| format!("{} has no selected model", runtime.label()))?;
        let (provider_id, model_id) = route_key
            .split_once("::")
            .ok_or_else(|| "selected model route is malformed".to_string())?;
        let effort = self
            .effort
            .read(cx)
            .selected_value()
            .and_then(|value| ReasoningEffort::parse(value));
        self.model_catalog.resolve(ModelRoute {
            runtime,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            reasoning_effort: effort,
        })
    }

    fn sync_model_options(
        &self,
        runtime: AgentRuntime,
        preferred: Option<(&str, &str)>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let options = Self::model_options(&self.model_catalog, runtime);
        let selected_index = preferred
            .and_then(|(provider_id, model_id)| {
                let key = format!("{provider_id}::{model_id}");
                options.iter().position(|option| option.route_key == key)
            })
            .or_else(|| options.iter().position(|option| option.enabled));
        self.model.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options.clone()), window, cx);
            state.set_selected_index(selected_index.map(IndexPath::new), window, cx);
        });
        if let Some(index) = selected_index {
            if let Some((provider_id, model_id)) = options[index].route_key.split_once("::") {
                self.sync_effort_options(
                    &ModelRoute {
                        runtime,
                        provider_id: provider_id.to_string(),
                        model_id: model_id.to_string(),
                        reasoning_effort: None,
                    },
                    window,
                    cx,
                );
            }
        }
    }

    fn sync_effort_options(&self, route: &ModelRoute, window: &mut Window, cx: &mut Context<Self>) {
        let options = Self::effort_options(&self.model_catalog, route);
        let selected_index = route
            .reasoning_effort
            .and_then(|effort| {
                options
                    .iter()
                    .position(|option| option.effort == Some(effort))
            })
            .or_else(|| {
                self.model_catalog
                    .model(&route.provider_id, &route.model_id)
                    .and_then(|model| model.capabilities.default_reasoning_effort)
                    .and_then(|effort| {
                        options
                            .iter()
                            .position(|option| option.effort == Some(effort))
                    })
            })
            .or((!options.is_empty()).then_some(0));
        self.effort.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options), window, cx);
            state.set_selected_index(selected_index.map(IndexPath::new), window, cx);
        });
    }

    pub fn set_provider_runtime(
        &mut self,
        runtime: AgentRuntime,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        if enabled {
            self.provider_runtimes.insert(runtime);
        } else {
            self.provider_runtimes.remove(&runtime);
        }
        cx.notify();
    }

    pub fn set_provider_capability(
        &mut self,
        capability: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) {
        match capability {
            "images" => self.provider_images = enabled,
            "tools" => self.provider_tools = enabled,
            _ => return,
        }
        cx.notify();
    }

    pub fn reset_provider_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        for input in [
            &self.provider_name,
            &self.provider_id,
            &self.provider_base_url,
            &self.provider_api_key,
            &self.provider_model_id,
            &self.provider_model_name,
        ] {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
        self.provider_context_window
            .update(cx, |state, cx| state.set_value("128000", window, cx));
        self.provider_max_output
            .update(cx, |state, cx| state.set_value("32000", window, cx));
        self.provider_protocol.update(cx, |state, cx| {
            state.set_selected_index(Some(IndexPath::new(1)), window, cx)
        });
        self.provider_runtimes = HashSet::from([AgentRuntime::Alva]);
        self.provider_images = true;
        self.provider_tools = true;
    }

    pub fn save_provider_form(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_string();
        let display_name = value(&self.provider_name);
        let provider_id = value(&self.provider_id);
        let base_url = value(&self.provider_base_url);
        let api_key = value(&self.provider_api_key);
        let model_id = value(&self.provider_model_id);
        let model_name = value(&self.provider_model_name);
        if display_name.is_empty()
            || provider_id.is_empty()
            || base_url.is_empty()
            || api_key.is_empty()
            || model_id.is_empty()
        {
            return Err("Provider name, ID, URL, API key, and model ID are required.".into());
        }
        if !provider_id.chars().all(|character| {
            character.is_ascii_lowercase()
                || character.is_ascii_digit()
                || matches!(character, '-' | '_')
        }) {
            return Err(
                "Provider ID may only contain lowercase letters, digits, '-' and '_'.".into(),
            );
        }
        if !base_url.starts_with("https://") && !base_url.starts_with("http://localhost") {
            return Err("Provider URL must use HTTPS (localhost may use HTTP).".into());
        }
        let protocol = self
            .provider_protocol
            .read(cx)
            .selected_value()
            .and_then(|value| ApiProtocol::parse(value))
            .ok_or_else(|| "Select an API protocol.".to_string())?;
        let mut runtimes = self.provider_runtimes.iter().copied().collect::<Vec<_>>();
        runtimes.sort_by_key(|runtime| runtime.id());
        if runtimes.is_empty() {
            return Err("Select at least one compatible agent runtime.".into());
        }
        if let Some(runtime) = runtimes
            .iter()
            .find(|runtime| !protocol_supports_runtime(protocol, **runtime))
        {
            return Err(format!(
                "{} cannot use the {} protocol.",
                runtime.label(),
                protocol.label()
            ));
        }
        let context_window = value(&self.provider_context_window)
            .parse::<u32>()
            .map_err(|_| "Context window must be a positive integer.".to_string())?;
        let max_output_tokens = value(&self.provider_max_output)
            .parse::<u32>()
            .map_err(|_| "Maximum output tokens must be a positive integer.".to_string())?;
        if context_window == 0 || max_output_tokens == 0 {
            return Err("Token limits must be greater than zero.".into());
        }
        let reasoning_efforts = if matches!(
            protocol,
            ApiProtocol::AnthropicMessages | ApiProtocol::OpenAiResponses
        ) {
            vec![
                ReasoningEffort::Low,
                ReasoningEffort::Medium,
                ReasoningEffort::High,
                ReasoningEffort::ExtraHigh,
            ]
        } else {
            Vec::new()
        };
        let model = CustomModelProfile {
            model_id: model_id.clone(),
            display_name: if model_name.is_empty() {
                model_id
            } else {
                model_name
            },
            context_window,
            max_output_tokens,
            images: self.provider_images,
            tools: self.provider_tools,
            reasoning_efforts,
        };
        if let Some(existing) = self
            .custom_providers
            .iter_mut()
            .find(|provider| provider.profile.provider_id == provider_id)
        {
            if existing.profile.protocol != protocol
                || existing.profile.base_url != base_url
                || existing.profile.runtimes != runtimes
            {
                return Err(
                    "An existing provider ID must keep the same protocol, URL, and compatible agents."
                        .into(),
                );
            }
            existing.profile.display_name = display_name.clone();
            existing
                .profile
                .models
                .retain(|existing| existing.model_id != model.model_id);
            existing.profile.models.push(model);
            existing.api_key = api_key;
        } else {
            self.custom_providers.push(StoredCustomProvider {
                profile: CustomProviderProfile {
                    provider_id: provider_id.clone(),
                    display_name: display_name.clone(),
                    protocol,
                    base_url,
                    runtimes,
                    models: vec![model],
                },
                api_key,
            });
        }
        self.provider_store.save(&self.custom_providers)?;
        self.reload_model_catalog(window, cx);
        Ok(display_name)
    }

    pub fn remove_custom_provider(
        &mut self,
        provider_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.custom_providers
            .retain(|provider| provider.profile.provider_id != provider_id);
        self.provider_store.save(&self.custom_providers)?;
        self.reload_model_catalog(window, cx);
        Ok(())
    }

    fn reload_model_catalog(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let support_dir = self
            .store
            .path()
            .parent()
            .unwrap_or(std::path::Path::new("/tmp"));
        let workspace = self
            .workspace
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or(support_dir);
        self.model_catalog = ModelCatalog::load(workspace, &self.custom_providers);
        let runtime = AgentRuntime::parse(&self.selected_engine(cx)).unwrap_or(AgentRuntime::Alva);
        self.sync_model_options(runtime, None, window, cx);
        cx.notify();
    }

    pub fn set_workspace_context(
        &mut self,
        workspace: String,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.workspace = Some(workspace.clone());
        self.plugin_store = PluginStore::new(std::path::Path::new(&workspace));
        self.skill_catalog = SkillCatalog::new(std::path::Path::new(&workspace));
        self.refresh_plugins();
        self.skills_loading = false;
        self.refresh_skills(cx);
        self.reload_model_catalog(window, cx);
    }

    pub fn refresh_plugins(&mut self) {
        if let Ok(plugins) = self.plugin_store.scan() {
            self.plugins = plugins;
        }
    }

    pub fn install_plugin_folder(
        &mut self,
        source: &std::path::Path,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let installed = self.plugin_store.install(source)?;
        let name = installed.manifest.name;
        self.refresh_plugins();
        cx.notify();
        Ok(name)
    }

    pub fn set_plugin_enabled(
        &mut self,
        name: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let plugin = self
            .plugins
            .iter()
            .find(|plugin| plugin.manifest.name == name)
            .cloned()
            .ok_or_else(|| "Plugin no longer exists.".to_string())?;
        self.plugin_store.set_enabled(&plugin, enabled)?;
        self.refresh_plugins();
        cx.notify();
        Ok(())
    }

    pub fn chat_with_plugin(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.primary_surface = PrimarySurface::Chat;
        self.prompt.update(cx, |input, cx| {
            input.set_value(format!("@{name} "), window, cx)
        });
        cx.notify();
    }

    pub fn refresh_skills(&mut self, cx: &mut Context<Self>) {
        if self.skills_loading {
            return;
        }
        self.skills_loading = true;
        let catalog = self.skill_catalog.clone();
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let result = executor.spawn(async move { catalog.list_blocking() }).await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.skills_loading = false;
                    match result {
                        Ok(skills) => hub.skills = skills,
                        Err(error) => hub.push_local_failure(&error, cx),
                    }
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn set_skill_enabled(&mut self, name: String, enabled: bool, cx: &mut Context<Self>) {
        let catalog = self.skill_catalog.clone();
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.spawn(async move |_this, cx| {
            let result = executor
                .spawn(async move { catalog.set_enabled_blocking(&name, enabled) })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    let message = match result {
                        Ok(()) => {
                            hub.refresh_skills(cx);
                            "Skill setting saved; it applies to the next ALVA turn.".to_string()
                        }
                        Err(error) => error,
                    };
                    with_window(cx, handle, move |_, window, cx| {
                        window.push_notification(message, cx);
                    });
                });
            }
        })
        .detach();
    }

    pub fn chat_with_skill(&mut self, name: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.primary_surface = PrimarySurface::Chat;
        self.prompt.update(cx, |input, cx| {
            input.set_value(format!("Use the {name} skill to "), window, cx)
        });
        cx.notify();
    }

    pub fn create_automation_from_form(
        &mut self,
        cx: &mut Context<Self>,
    ) -> Result<String, String> {
        let name = self.automation_name.read(cx).value().trim().to_string();
        let prompt = self.automation_prompt.read(cx).value().trim().to_string();
        if name.is_empty() || prompt.is_empty() {
            return Err("Automation name and prompt are required.".into());
        }
        let schedule = self
            .automation_schedule
            .read(cx)
            .selected_value()
            .and_then(|value| AutomationSchedule::parse(value))
            .ok_or_else(|| "Select an automation schedule.".to_string())?;
        let target = self
            .model_catalog
            .engine_target(self.selected_model_route(cx)?)?;
        let workspace = self
            .workspace
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.store
                    .path()
                    .parent()
                    .unwrap_or(std::path::Path::new("/tmp"))
                    .join("conversations")
            });
        let automation = AutomationDefinition::new(
            uuid::Uuid::new_v4().to_string(),
            name.clone(),
            prompt,
            schedule,
            workspace,
            target,
            self.selected_profile(cx),
            now_ms(),
            local_offset_seconds(),
        );
        self.automation_store.insert(automation)?;
        self.refresh_automations();
        cx.notify();
        Ok(name)
    }

    pub fn set_automation_enabled(
        &mut self,
        id: &str,
        enabled: bool,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        self.automation_store
            .set_enabled(id, enabled, now_ms(), local_offset_seconds())?;
        self.refresh_automations();
        cx.notify();
        Ok(())
    }

    pub fn delete_automation(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        self.automation_store.delete(id)?;
        self.refresh_automations();
        cx.notify();
        Ok(())
    }

    pub fn run_automation_now(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        self.start_automation_run(id, cx)
    }

    fn refresh_automations(&mut self) {
        if let Ok(automations) = self.automation_store.load() {
            self.automations = automations;
        }
    }

    fn start_automation_scheduler(&mut self, cx: &mut Context<Self>) {
        self.poll_due_automations(cx);
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| loop {
            cx.background_executor()
                .timer(Duration::from_secs(30))
                .await;
            if weak
                .update(cx, |hub, cx| hub.poll_due_automations(cx))
                .is_err()
            {
                break;
            }
        })
        .detach();
    }

    fn poll_due_automations(&mut self, cx: &mut Context<Self>) {
        let due = match self.automation_store.due(now_ms()) {
            Ok(due) => due,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        for automation in due {
            if let Err(error) = self.start_automation_run(&automation.id, cx) {
                self.push_local_failure(&error, cx);
            }
        }
    }

    fn start_automation_run(&mut self, id: &str, cx: &mut Context<Self>) -> Result<(), String> {
        let current = self
            .automations
            .iter()
            .find(|automation| automation.id == id)
            .ok_or_else(|| "Automation no longer exists.".to_string())?;
        if current.last_status == AutomationRunStatus::Running {
            return Err("This automation is already running.".into());
        }
        let automation = self.automation_store.mark_started(id, now_ms())?;
        self.refresh_automations();
        cx.notify();

        if let Err(error) = std::fs::create_dir_all(&automation.workspace) {
            let error = format!("prepare automation workspace: {error}");
            let _ = self.automation_store.mark_finished(
                id,
                now_ms(),
                local_offset_seconds(),
                None,
                Some(error.clone()),
            );
            self.refresh_automations();
            return Err(error);
        }
        let hub_store = Arc::clone(&self.store);
        let automation_store = self.automation_store.clone();
        let model_catalog = self.model_catalog.clone();
        let target = automation.target.clone();
        let automation_id = automation.id.clone();
        let automation_name = automation.name.clone();
        let workspace = automation.workspace.clone();
        let permission_profile = automation.permission_profile;
        let prompt = automation.prompt.clone();
        let history_dir = hub_store
            .path()
            .parent()
            .unwrap_or(automation.workspace.as_path())
            .to_path_buf();
        let weak = cx.entity().downgrade();
        let handle = self.window_handle;
        cx.spawn(async move |_this, cx| {
            let outcome = run_persisted_with_target(
                &hub_store,
                {
                    let workspace = workspace.clone();
                    let history_dir = history_dir.clone();
                    move |target, _, profile| {
                        build_engine_bundle_for_target_with_provider(
                            target,
                            profile,
                            &workspace,
                            &history_dir,
                            model_catalog.runtime_config(&target.provider_id),
                        )
                        .map(|built| built.runtime)
                    }
                },
                |_| None,
                RunRequest {
                    prompt,
                    workspace,
                    engine_id: target.engine_id.clone(),
                    product_session_id: None,
                    permission_profile: Some(permission_profile),
                },
                Some(target),
                |_| {},
            )
            .await;
            let (session_id, error) = match outcome {
                Ok(outcome) if !outcome.failed => (Some(outcome.product_session_id), None),
                Ok(outcome) => (
                    Some(outcome.product_session_id),
                    Some("The automation attempt failed.".to_string()),
                ),
                Err(error) => (None, Some(error)),
            };
            let finished = automation_store.mark_finished(
                &automation_id,
                now_ms(),
                local_offset_seconds(),
                session_id,
                error.clone(),
            );
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.refresh_automations();
                    hub.refresh_sessions();
                    let message = match (&finished, error) {
                        (Ok(_), None) => format!("Automation completed: {automation_name}"),
                        (Ok(_), Some(error)) => {
                            format!("Automation failed: {automation_name} — {error}")
                        }
                        (Err(error), _) => format!("Unable to save automation result: {error}"),
                    };
                    with_window(cx, handle, move |_, window, cx| {
                        window.push_notification(message, cx);
                    });
                    cx.notify();
                });
            }
        })
        .detach();
        Ok(())
    }

    /// Rebuild the engine selector options after readiness changes.
    pub fn sync_engine_options(&self, window: &mut Window, cx: &mut Context<Self>) {
        let options = Self::engine_options(&self.readiness);
        let current = self.selected_engine(cx);
        self.engine.update(cx, |state, cx| {
            state.set_items(SearchableVec::new(options.clone()), window, cx);
            let index = options
                .iter()
                .position(|option| option.engine_id == current);
            state.set_selected_index(index.map(IndexPath::new), window, cx);
        });
    }

    pub fn selected_engine(&self, cx: &App) -> String {
        self.engine
            .read(cx)
            .selected_value()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "alva".to_string())
    }

    pub fn selected_profile(&self, cx: &App) -> PermissionProfile {
        let value = self
            .permission
            .read(cx)
            .selected_value()
            .map(|s| s.to_string())
            .unwrap_or_else(|| "workspace_write".to_string());
        PermissionProfile::parse(&value).unwrap_or(PermissionProfile::WorkspaceWrite)
    }

    /// Everything that describes "the session I am looking at" must reset
    /// together so a previous session's profile/policy/timeline can never
    /// leak into the next one (React `resetSessionSurface`).
    pub fn reset_session_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.primary_surface = PrimarySurface::Chat;
        self.active_session_id = None;
        self.session_detail = None;
        self.team_run = None;
        self.timeline.clear();
        self.chat.clear();
        self.inspector_snapshot = None;
        self.inspector_error = None;
        self.inspector_session_stack.clear();
        self.inspector_open_records.clear();
        self.open_handoffs.clear();
        self.open_activities.clear();
        self.pending_engine = None;
        self.transcript_sharing = false;
        let runtime = if self
            .model_catalog
            .default_route(AgentRuntime::Alva)
            .is_some()
        {
            AgentRuntime::Alva
        } else {
            AgentRuntime::ClaudeCode
        };
        self.set_selected_engine(runtime.id(), window, cx);
        if let Some(route) = self.model_catalog.default_route(runtime) {
            self.sync_model_options(
                runtime,
                Some((&route.provider_id, &route.model_id)),
                window,
                cx,
            );
        }
        self.set_selected_profile("workspace_write", window, cx);
    }

    pub fn set_selected_engine(
        &self,
        engine_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = alva_hub_gpui::HUB_ENGINE_IDS
            .iter()
            .position(|candidate| *candidate == engine_id);
        self.engine.update(cx, |state, cx| {
            state.set_selected_index(index.map(IndexPath::new), window, cx);
        });
    }

    pub fn set_selected_profile(&self, profile: &str, window: &mut Window, cx: &mut Context<Self>) {
        let index = PROFILE_OPTIONS
            .iter()
            .position(|candidate| *candidate == profile);
        self.permission.update(cx, |state, cx| {
            state.set_selected_index(index.map(IndexPath::new), window, cx);
        });
    }

    /// Subscribe the engine selector: a confirm updates the selection
    /// (manual switch when a session is open).
    pub fn subscribe_engine(&self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::select::SelectEvent;
        let weak = cx.entity().downgrade();
        let _subscription = cx.subscribe_in(
            &self.engine,
            window,
            move |_view, _state, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(cx, |hub, cx| {
                            hub.select_engine(value, window, cx);
                        });
                    }
                }
            },
        );
    }

    pub fn subscribe_model(&self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::select::SelectEvent;
        let weak = cx.entity().downgrade();
        let _subscription = cx.subscribe_in(
            &self.model,
            window,
            move |_view, _state, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(cx, |hub, cx| {
                            hub.select_model_route(value, window, cx);
                        });
                    }
                }
            },
        );
    }

    pub fn subscribe_effort(&self, window: &mut Window, cx: &mut Context<Self>) {
        use gpui_component::select::SelectEvent;
        let weak = cx.entity().downgrade();
        let _subscription = cx.subscribe_in(
            &self.effort,
            window,
            move |_view, _state, event, window, cx| {
                if let SelectEvent::Confirm(Some(value)) = event {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(cx, |hub, cx| hub.select_reasoning_effort(value, window, cx));
                    }
                }
            },
        );
    }

    pub fn subscribe_model_query(&self, window: &mut Window, cx: &mut Context<Self>) {
        let _subscription = cx.subscribe_in(
            &self.model_query,
            window,
            |_view, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );
    }

    /// Enter in the composer submits the run.
    pub fn subscribe_prompt(&self, window: &mut Window, cx: &mut Context<Self>) {
        let weak = cx.entity().downgrade();
        let _subscription = cx.subscribe_in(
            &self.prompt,
            window,
            move |_view, _state, event, window, cx| {
                if matches!(event, InputEvent::PressEnter { .. }) {
                    if let Some(entity) = weak.upgrade() {
                        entity.update(cx, |hub, cx| {
                            hub.run(window, cx);
                        });
                    }
                }
            },
        );
    }

    pub fn subscribe_extension_query(&self, window: &mut Window, cx: &mut Context<Self>) {
        let _subscription = cx.subscribe_in(
            &self.extension_query,
            window,
            |_view, _state, event, _window, cx| {
                if matches!(event, InputEvent::Change) {
                    cx.notify();
                }
            },
        );
    }

    /// Search filter over sessions, matching React `filteredSessions`.
    pub fn filtered_sessions(&self, cx: &Context<Self>) -> Vec<HubSessionSummary> {
        let query = self.session_query.read(cx).value().trim().to_lowercase();
        if query.is_empty() {
            return self.sessions.clone();
        }
        self.sessions
            .iter()
            .filter(|session| {
                [
                    session.workspace.as_str(),
                    session.last_prompt.as_deref().unwrap_or(""),
                    session.engine_id.as_str(),
                ]
                .join(" ")
                .to_lowercase()
                .contains(&query)
            })
            .cloned()
            .collect()
    }

    pub fn set_primary_surface(&mut self, surface: PrimarySurface, cx: &mut Context<Self>) {
        self.primary_surface = surface;
        cx.notify();
    }

    pub fn open_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.primary_surface = PrimarySurface::Settings;
        let Some(dock_area) = self.workspace_dock.as_ref().and_then(WeakEntity::upgrade) else {
            cx.notify();
            return;
        };
        let (left_dock, right_dock) = {
            let dock_area = dock_area.read(cx);
            (
                dock_area.left_dock().cloned(),
                dock_area.right_dock().cloned(),
            )
        };
        if let Some(left_dock) = left_dock {
            left_dock.update(cx, |dock, cx| dock.set_open(false, window, cx));
        }
        if let Some(right_dock) = right_dock {
            right_dock.update(cx, |dock, cx| dock.set_open(false, window, cx));
        }
        cx.notify();
    }

    pub fn close_settings(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.primary_surface = PrimarySurface::Chat;
        let Some(dock_area) = self.workspace_dock.as_ref().and_then(WeakEntity::upgrade) else {
            cx.notify();
            return;
        };
        let left_dock = dock_area.read(cx).left_dock().cloned();
        if let Some(left_dock) = left_dock {
            left_dock.update(cx, |dock, cx| dock.set_open(true, window, cx));
        }
        cx.notify();
    }

    pub fn open_workspace_tool(
        &mut self,
        tool: WorkspaceTool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(dock_area) = self.workspace_dock.as_ref().and_then(WeakEntity::upgrade) else {
            return;
        };
        let already_open = self.workspace_open_tools.contains(&tool);
        if !already_open {
            self.workspace_open_tools.push(tool);
            let Some(panel) = self
                .workspace_tool_panels
                .get(&tool)
                .and_then(WeakEntity::upgrade)
            else {
                self.workspace_open_tools
                    .retain(|candidate| *candidate != tool);
                return;
            };
            dock_area.update(cx, |dock, cx| {
                dock.add_panel(
                    Arc::new(panel),
                    gpui_component::dock::DockPlacement::Right,
                    None,
                    window,
                    cx,
                );
            });
        }
        let Some(index) = self
            .workspace_open_tools
            .iter()
            .position(|candidate| *candidate == tool)
        else {
            return;
        };
        let right_dock = dock_area.read(cx).right_dock().cloned();
        let Some(right_dock) = right_dock else {
            return;
        };
        right_dock.update(cx, |dock, cx| {
            if already_open {
                let panel = dock.panel().clone().active_index(index, cx);
                dock.set_panel(panel, window, cx);
            }
            dock.set_open(true, window, cx);
        });
    }

    /// Store reads that must happen off the UI thread.
    pub fn refresh_sessions(&mut self) {
        self.session_error = None;
        match self.store.list_sessions() {
            Ok(sessions) => self.sessions = sessions,
            Err(error) => self.session_error = Some(error),
        }
    }

    pub fn refresh_readiness(&mut self, cx: &mut Context<Self>) {
        let store = Arc::clone(&self.store);
        let workspace = self.workspace.clone();
        self.readiness_busy = true;
        cx.notify();
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let (readiness, plan) = executor
                .spawn(async move {
                    let probe = SystemProbe::new(Some(&store), workspace.map(PathBuf::from));
                    let readiness = probe_readiness(&probe);
                    let plan: Vec<RepairAction> = readiness.iter().flat_map(repair_plan).collect();
                    (readiness, plan)
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.readiness = readiness;
                    hub.repair_plan = plan;
                    hub.readiness_busy = false;
                    with_window(cx, hub.window_handle, |hub, window, cx| {
                        hub.sync_engine_options(window, cx);
                    });
                    cx.notify();
                });
            }
        })
        .detach();
    }

    #[allow(dead_code)]
    pub fn refresh_eligibility(&self, cx: &mut Context<Self>) {
        let profile = self.selected_profile(cx);
        let weak = cx.entity().downgrade();
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let report = executor
                .spawn(async move { permission_eligibility(profile) })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    hub.eligibility = report;
                    cx.notify();
                });
            }
        })
        .detach();
    }

    pub fn load_session(
        &mut self,
        product_session_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Result<(), String> {
        let detail = self
            .store
            .get_session(product_session_id)?
            .ok_or_else(|| "This Hub session no longer exists.".to_string())?;
        self.active_session_id = Some(product_session_id.to_string());
        self.primary_surface = PrimarySurface::Chat;
        self.set_workspace_context(detail.session.workspace.clone(), window, cx);
        self.pending_engine = detail
            .pending_switch
            .as_ref()
            .map(|s| s.target.engine_id.clone());
        self.transcript_sharing = detail.session.include_full_transcript;
        self.fallback_policy = detail.fallback_policy.clone();
        self.timeline = project_timeline(&detail);
        self.chat = project_chat(&detail);
        self.session_detail = Some(detail);
        self.open_handoffs.clear();
        self.open_activities.clear();
        let session = self.session_detail.as_ref().unwrap().session.clone();
        let selected_target = self.session_detail.as_ref().and_then(|detail| {
            detail
                .pending_switch
                .as_ref()
                .map(|switch| switch.target.clone())
        });
        let selected_engine = selected_target
            .as_ref()
            .map(|target| target.engine_id.as_str())
            .unwrap_or(session.engine_id.as_str());
        let selected_provider = selected_target
            .as_ref()
            .map(|target| target.provider_id.as_str())
            .unwrap_or(session.provider_id.as_str());
        let selected_model = selected_target
            .as_ref()
            .map(|target| target.model_id.as_str())
            .unwrap_or(session.model_id.as_str());
        let selected_effort = selected_target
            .as_ref()
            .and_then(|target| target.reasoning_effort.as_deref())
            .or(session.reasoning_effort.as_deref());
        self.set_selected_engine(selected_engine, window, cx);
        if let Some(runtime) = AgentRuntime::parse(selected_engine) {
            let preferred = self
                .model_catalog
                .model(selected_provider, selected_model)
                .map(|_| (selected_provider, selected_model));
            self.sync_model_options(runtime, preferred, window, cx);
            if preferred.is_some() {
                self.sync_effort_options(
                    &ModelRoute {
                        runtime,
                        provider_id: selected_provider.to_string(),
                        model_id: selected_model.to_string(),
                        reasoning_effort: selected_effort.and_then(ReasoningEffort::parse),
                    },
                    window,
                    cx,
                );
            }
        }
        self.set_selected_profile(
            self.session_detail
                .as_ref()
                .unwrap()
                .session
                .permission_profile
                .as_str(),
            window,
            cx,
        );
        self.team_run = self
            .store
            .team_run_for_lead(product_session_id)
            .ok()
            .flatten();
        self.sync_policy_inputs(window, cx);
        if self.inspector_enabled {
            self.refresh_inspector(cx);
        }
        Ok(())
    }

    fn apply_live_event(&mut self, event: TimelineEvent, cx: &mut Context<Self>) {
        let product_session_id = event.product_session_id.clone();
        let engine_id = event.engine_id.clone();
        let status = event.status;
        let projected = ProjectedEvent::from_live(event);

        if status == TimelineStatus::Streaming {
            if let Some(existing) = self.timeline.iter_mut().rev().find(|existing| {
                existing.status == TimelineStatus::Streaming
                    && existing.engine_id == engine_id
                    && existing.product_session_id == product_session_id
            }) {
                existing.message = projected.message.clone();
            } else {
                self.timeline.push(projected.clone());
            }
            if let Some(existing) = self.chat.iter_mut().rev().find(|existing| {
                existing.status == Some(TimelineStatus::Streaming)
                    && existing.engine_id == engine_id
            }) {
                existing.message = projected.message.clone().unwrap_or_default();
            } else {
                let timeline_index = self.timeline.len().saturating_sub(1);
                self.chat
                    .push(ProjectedChatItem::from_live(&projected, timeline_index));
            }
        } else {
            if status == TimelineStatus::Completed {
                self.timeline.retain(|existing| {
                    !(existing.status == TimelineStatus::Streaming
                        && existing.engine_id == engine_id
                        && existing.product_session_id == product_session_id)
                });
                self.chat.retain(|existing| {
                    !(existing.status == Some(TimelineStatus::Streaming)
                        && existing.engine_id == engine_id)
                });
            }
            let timeline_index = self.timeline.len();
            self.chat
                .push(ProjectedChatItem::from_live(&projected, timeline_index));
            self.timeline.push(projected);
        }

        self.active_session_id = Some(product_session_id.clone());
        if let Ok(Some(detail)) = self.store.get_session(&product_session_id) {
            let inspector_needs_session = self.inspector_enabled
                && detail.session.engine_session_id.is_some()
                && self.inspector_snapshot.as_ref().is_none_or(|snapshot| {
                    Some(snapshot.native_session_id.as_str())
                        != detail.session.engine_session_id.as_deref()
                });
            self.session_detail = Some(detail);
            if inspector_needs_session {
                self.refresh_inspector(cx);
            }
        }
        cx.notify();
    }

    /// Run through a channel so chat activity is applied as each engine event
    /// arrives instead of after the whole attempt completes.
    pub fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.pending_engine.is_some() {
            window.push_notification("The selected model route is still being applied.", cx);
            return;
        }
        let workspace = self
            .workspace
            .clone()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                self.store
                    .path()
                    .parent()
                    .unwrap_or(std::path::Path::new("/tmp"))
                    .join("conversations")
            });
        if let Err(error) = std::fs::create_dir_all(&workspace) {
            window.push_notification(
                format!("Unable to prepare the conversation workspace: {error}"),
                cx,
            );
            return;
        }
        let workspace = workspace.display().to_string();
        let prompt = self.prompt.read(cx).value().to_string();
        if prompt.trim().is_empty() || self.busy {
            return;
        }
        let route = match self.selected_model_route(cx) {
            Ok(route) => route,
            Err(error) => {
                window.push_notification(error, cx);
                return;
            }
        };
        let initial_target = match self.model_catalog.engine_target(route) {
            Ok(target) => target,
            Err(error) => {
                window.push_notification(error, cx);
                return;
            }
        };
        let engine_id = initial_target.engine_id.clone();
        let profile = self.selected_profile(cx);
        let product_session_id = self.active_session_id.clone();
        let store = Arc::clone(&self.store);
        let engines = Arc::clone(&self.active_engines);
        let inspectors = Arc::clone(&self.active_inspectors);
        let model_catalog = self.model_catalog.clone();
        let handle = self.window_handle;
        let history_dir = store.path().parent().map(PathBuf::from);
        self.busy = true;
        self.chat
            .push(ProjectedChatItem::user(prompt.clone(), None));
        self.prompt
            .update(cx, |state, cx| state.set_value("", window, cx));
        cx.notify();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx: &mut AsyncApp| {
            let (event_sender, mut event_receiver) = tokio::sync::mpsc::unbounded_channel();
            let mut run = Box::pin(run_persisted_with_target(
                &store,
                {
                    let ws_path = PathBuf::from(&workspace);
                    let engines = Arc::clone(&engines);
                    move |target, product_session_id, profile| {
                        let hist = history_dir.clone().unwrap_or_else(|| ws_path.clone());
                        let built = build_engine_bundle_for_target_with_provider(
                            target,
                            profile,
                            &ws_path,
                            &hist,
                            model_catalog.runtime_config(&target.provider_id),
                        )?;
                        let engine = built.runtime;
                        // Register the live engine so Stop can cancel it;
                        // fallback replaces the entry with the new engine.
                        engines
                            .lock()
                            .unwrap()
                            .insert(product_session_id.to_string(), Arc::clone(&engine));
                        if let Some(inspector) = built.inspector {
                            inspectors
                                .lock()
                                .unwrap()
                                .insert(product_session_id.to_string(), inspector);
                        }
                        Ok(engine)
                    }
                },
                |_| None,
                RunRequest {
                    prompt: prompt.clone(),
                    workspace: PathBuf::from(&workspace),
                    engine_id: engine_id.clone(),
                    product_session_id,
                    permission_profile: Some(profile),
                },
                Some(initial_target),
                move |event| {
                    let _ = event_sender.send(event);
                },
            ));
            let outcome = loop {
                tokio::select! {
                    outcome = &mut run => break outcome,
                    event = event_receiver.recv() => {
                        let Some(event) = event else { continue };
                        if let Some(hub) = weak.upgrade() {
                            hub.update(cx, |hub, cx| hub.apply_live_event(event, cx));
                        }
                    }
                }
            };
            while let Ok(event) = event_receiver.try_recv() {
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| hub.apply_live_event(event, cx));
                }
            }
            let outcome_session = outcome.as_ref().ok().map(|o| o.product_session_id.clone());
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    if let Err(error) = &outcome {
                        hub.push_local_failure(error, cx);
                    }
                    hub.busy = false;
                    if let Some(pid) = outcome_session {
                        hub.active_session_id = Some(pid.clone());
                        hub.refresh_sessions();
                        if let Ok(Some(detail)) = hub.store.get_session(&pid) {
                            hub.session_detail = Some(detail.clone());
                            hub.workspace = Some(detail.session.workspace.clone());
                            hub.pending_engine = detail
                                .pending_switch
                                .as_ref()
                                .map(|s| s.target.engine_id.clone());
                            hub.transcript_sharing = detail.session.include_full_transcript;
                            hub.fallback_policy = detail.fallback_policy.clone();
                            hub.timeline = project_timeline(&detail);
                            hub.chat = project_chat(&detail);
                            hub.open_handoffs.clear();
                            if hub.inspector_enabled {
                                hub.refresh_inspector(cx);
                            }
                        }
                    }
                    cx.notify();
                });
                // Refresh the profile selector when a brand-new session was
                // created (it is fixed from then on).
                if let Some(hub) = weak.upgrade() {
                    hub.update(cx, |hub, cx| {
                        if let Some(detail) = &hub.session_detail {
                            let profile = detail.session.permission_profile.as_str().to_string();
                            with_window(cx, handle, move |hub, window, cx| {
                                hub.set_selected_profile(&profile, window, cx);
                            });
                        }
                    });
                }
            }
        })
        .detach();
    }

    /// Manual engine switch — queues behind a running attempt; applies
    /// immediately otherwise (React `selectEngine`).
    pub fn select_engine(&mut self, engine_id: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(runtime) = AgentRuntime::parse(engine_id) else {
            self.push_local_failure("The selected runtime is unknown.", cx);
            return;
        };
        let Some(route) = self.model_catalog.default_route(runtime) else {
            self.push_local_failure(&format!("{} has no configured model.", runtime.label()), cx);
            return;
        };
        self.set_selected_engine(engine_id, window, cx);
        self.sync_model_options(
            runtime,
            Some((&route.provider_id, &route.model_id)),
            window,
            cx,
        );
        let target = match self.model_catalog.engine_target(route) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        self.request_target_switch(target, format!("User explicitly selected {engine_id}"), cx);
    }

    pub(crate) fn select_model_route(
        &mut self,
        route_key: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some((provider_id, model_id)) = route_key.split_once("::") else {
            self.push_local_failure("The selected model route is malformed.", cx);
            return;
        };
        let Some(runtime) = AgentRuntime::parse(&self.selected_engine(cx)) else {
            return;
        };
        let route = ModelRoute {
            runtime,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            reasoning_effort: None,
        };
        let options = Self::model_options(&self.model_catalog, runtime);
        let selected_index = options
            .iter()
            .position(|option| option.route_key == route_key);
        self.model.update(cx, |state, cx| {
            state.set_selected_index(selected_index.map(IndexPath::new), window, cx);
        });
        self.sync_effort_options(&route, window, cx);
        let target = match self.model_catalog.engine_target(route) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        self.request_target_switch(
            target,
            format!("User explicitly selected {model_id} from {provider_id}"),
            cx,
        );
    }

    pub(crate) fn select_reasoning_effort(
        &mut self,
        value: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(runtime) = AgentRuntime::parse(&self.selected_engine(cx)) else {
            return;
        };
        let Some(route_key) = self
            .model
            .read(cx)
            .selected_value()
            .map(ToString::to_string)
        else {
            return;
        };
        let Some((provider_id, model_id)) = route_key.split_once("::") else {
            return;
        };
        let route = ModelRoute {
            runtime,
            provider_id: provider_id.to_string(),
            model_id: model_id.to_string(),
            reasoning_effort: ReasoningEffort::parse(value),
        };
        let effort_index = Self::effort_options(&self.model_catalog, &route)
            .iter()
            .position(|option| option.value == value);
        self.effort.update(cx, |state, cx| {
            state.set_selected_index(effort_index.map(IndexPath::new), window, cx);
        });
        let target = match self.model_catalog.engine_target(route) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        self.request_target_switch(
            target,
            format!("User explicitly selected {value} reasoning"),
            cx,
        );
    }

    fn request_target_switch(
        &mut self,
        target: EngineTarget,
        reason: String,
        cx: &mut Context<Self>,
    ) {
        let Some(session_id) = self.active_session_id.clone() else {
            self.pending_engine = None;
            cx.notify();
            return;
        };
        self.pending_engine = Some(target.engine_id.clone());
        cx.notify();
        let store = Arc::clone(&self.store);
        let weak = cx.entity().downgrade();
        let handle = self.window_handle;
        let executor = cx.background_executor().clone();
        cx.spawn(async move |_this, cx| {
            let outcome = executor
                .spawn(async move {
                    store
                        .request_engine_switch(&session_id, &target, &reason)
                        .map(|outcome| (outcome, store))
                })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| match outcome {
                    Ok((outcome, store)) => {
                        let pid = hub.active_session_id.clone();
                        with_window(cx, handle, move |hub, window, cx| {
                            hub.apply_switch_outcome(&outcome, &store, window, cx);
                            if let Some(pid) = pid {
                                let _ = hub.load_session(&pid, window, cx);
                            }
                        });
                        hub.refresh_sessions();
                        cx.notify();
                    }
                    Err(error) => {
                        hub.pending_engine = None;
                        hub.push_local_failure(&error, cx);
                    }
                });
            }
        })
        .detach();
    }

    pub fn apply_switch_outcome(
        &mut self,
        outcome: &SwitchOutcome,
        _store: &HubStore,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.set_selected_engine(&outcome.active_engine_id, window, cx);
        self.pending_engine = match outcome.disposition {
            alva_hub_gpui::SwitchDisposition::Applied => None,
            alva_hub_gpui::SwitchDisposition::Queued => Some(outcome.target_engine_id.clone()),
        };
    }

    /// Run an allowlisted repair command on a background thread, then
    /// re-probe so the plan never goes stale (React `repairEngine`).
    pub fn run_repair(&mut self, engine_id: String, kind: RepairKind, cx: &mut Context<Self>) {
        let Some(readiness) = self
            .readiness
            .iter()
            .find(|report| report.engine_id == engine_id)
            .cloned()
        else {
            return;
        };
        let argv = match repair_argv(&readiness, kind) {
            Ok(argv) => argv,
            Err(error) => {
                self.repair_logs
                    .insert(engine_id, format!("Repair failed. {error}"));
                cx.notify();
                return;
            }
        };
        self.repairing_engine = Some(engine_id.clone());
        cx.notify();
        let executor = cx.background_executor().clone();
        let weak = cx.entity().downgrade();
        cx.spawn(async move |_this, cx| {
            let outcome = executor
                .spawn(async move { run_repair_command(&argv, None) })
                .await;
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    let summary = format!(
                        "Repair {}. {}",
                        if outcome.success {
                            "finished"
                        } else {
                            "failed"
                        },
                        outcome.log.trim()
                    );
                    hub.repair_logs.insert(engine_id.clone(), summary);
                    hub.repairing_engine = None;
                    hub.refresh_readiness(cx);
                    cx.notify();
                });
            }
        })
        .detach();
    }

    /// Local failure surfaces as a failed timeline card ("setup" session),
    /// matching React `appendLocalFailure`.
    pub fn push_local_failure(&mut self, error: &str, cx: &mut Context<Self>) {
        let engine_id = self.selected_engine(cx);
        self.timeline.push(ProjectedEvent {
            status: TimelineStatus::Failed,
            engine_id,
            product_session_id: "setup".to_string(),
            message: Some(error.to_string()),
            permission_request: None,
            created_at: None,
            fallback: None,
        });
        let timeline_index = self.timeline.len() - 1;
        self.chat.push(ProjectedChatItem::from_live(
            &self.timeline[timeline_index],
            timeline_index,
        ));
        cx.notify();
    }
}

impl Render for HubApp {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .size_full()
            .bg(cx.theme().colors.background)
            .child(self.sidebar(window, cx))
            .child(self.main_column(window, cx))
    }
}

pub fn status_color(status: &TimelineStatus, cx: &App) -> (String, gpui::Hsla) {
    use gpui_component::ActiveTheme as _;
    match status {
        TimelineStatus::Completed => ("completed".to_string(), cx.theme().colors.success),
        TimelineStatus::Failed => ("failed".to_string(), cx.theme().colors.danger),
        TimelineStatus::Fallback => ("fallback".to_string(), cx.theme().colors.warning),
        TimelineStatus::Exhausted => ("exhausted".to_string(), cx.theme().colors.danger),
        TimelineStatus::PermissionRequired => ("permission".to_string(), cx.theme().colors.accent),
        TimelineStatus::Worker => ("worker".to_string(), cx.theme().colors.accent),
        TimelineStatus::Interrupted => (
            "interrupted".to_string(),
            cx.theme().colors.muted_foreground,
        ),
        TimelineStatus::Queued => ("queued".to_string(), cx.theme().colors.accent),
        TimelineStatus::Cancelled => ("cancelled".to_string(), cx.theme().colors.muted_foreground),
        TimelineStatus::Switched => ("switched".to_string(), cx.theme().colors.accent),
        TimelineStatus::Running => ("running".to_string(), cx.theme().colors.accent),
        TimelineStatus::Streaming => ("streaming".to_string(), cx.theme().colors.accent),
        TimelineStatus::ToolRunning => ("tool".to_string(), cx.theme().colors.accent),
        TimelineStatus::ToolCompleted => ("tool done".to_string(), cx.theme().colors.success),
    }
}

pub fn profile_label_of(profile: &str) -> String {
    profile_label(profile)
}
