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

use alva_engine_runtime::EngineRuntime;
use gpui::*;
use gpui_component::{
    input::{InputEvent, InputState},
    searchable_list::SearchableVec,
    select::SelectState,
    ActiveTheme, *,
};

use alva_hub_gpui::{
    build_engine, permission_eligibility, probe_readiness, repair_argv, repair_plan, run_persisted,
    run_repair_command, EngineEligibility, EngineReadiness, EngineTarget, FallbackPolicy,
    HubSessionDetail, HubSessionSummary, HubStore, PermissionProfile, PermissionRouter,
    RepairAction, RepairKind, RunRequest, SwitchOutcome, SystemProbe, TimelineEvent,
    TimelineStatus,
};

use super::workspace::HubWorkspace;
use super::{profile_label, EngineOption, WorkerOption};
use alva_hub_gpui::{project_chat, project_timeline, ProjectedChatItem, ProjectedEvent};

const ENGINE_OPTIONS: [&str; 3] = ["alva", "claude", "codex"];
const PROFILE_OPTIONS: [&str; 3] = ["interactive", "workspace_write", "unrestricted"];

pub struct HubApp {
    pub store: Arc<HubStore>,
    /// Live engines keyed by product session id, so Stop can cancel the
    /// adapter currently streaming (React/Tauri `active_engines`).
    pub active_engines: Arc<Mutex<HashMap<String, Arc<dyn EngineRuntime>>>>,
    /// Pending approval routing; run emits register requests here.
    pub permission_router: Arc<PermissionRouter>,
    /// Handle of this app's single window, so async contexts can reach the
    /// window for stateful-component updates.
    pub window_handle: AnyWindowHandle,
    pub workspace: Option<String>,
    pub engine: Entity<SelectState<SearchableVec<EngineOption>>>,
    pub permission: Entity<SelectState<SearchableVec<&'static str>>>,
    pub prompt: Entity<InputState>,
    pub timeline: Vec<ProjectedEvent>,
    pub chat: Vec<ProjectedChatItem>,
    /// Timeline index → handoff card expanded (keyed by index like React).
    pub open_handoffs: HashMap<usize, bool>,
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
        let sessions = store.list_sessions().unwrap_or_default();
        let fallback_policy = store
            .global_fallback_policy()
            .unwrap_or_else(|_| FallbackPolicy {
                targets: Vec::new(),
                updated_at: 0,
            });
        let eligibility = permission_eligibility(PermissionProfile::WorkspaceWrite);
        let mut app = Self {
            store,
            active_engines: Arc::new(Mutex::new(HashMap::new())),
            permission_router: Arc::new(PermissionRouter::new()),
            window_handle: window.window_handle(),
            workspace: None,
            timeline: Vec::new(),
            chat: Vec::new(),
            open_handoffs: HashMap::new(),
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
                    Some(IndexPath::new(0)),
                    window,
                    cx,
                )
            }),
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
                    .multi_line(true)
                    .placeholder("Ask alva to inspect, change, or explain this project…")
            }),
        };
        app.subscribe_sessions(window, cx);
        app.subscribe_engine(window, cx);
        app.subscribe_prompt(window, cx);
        app.sync_policy_inputs(window, cx);
        app.refresh_readiness(cx);
        app
    }

    fn engine_options(readiness: &[EngineReadiness]) -> Vec<EngineOption> {
        ENGINE_OPTIONS
            .iter()
            .map(|id| {
                let display = match *id {
                    "alva" => "Alva",
                    "claude" => "Claude",
                    "codex" => "Codex",
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

    pub fn label_small(
        &self,
        text: impl Into<SharedString>,
        cx: &Context<Self>,
    ) -> impl IntoElement {
        div()
            .text_xs()
            .text_color(cx.theme().colors.muted_foreground)
            .child(text.into())
    }

    /// Status pill label, matching React `statusLabel`.
    pub fn status_label(&self) -> String {
        let last = self.timeline.last();
        let engine_id = last.map(|e| e.engine_id.as_str()).unwrap_or("engine");
        match last.map(|e| &e.status) {
            Some(TimelineStatus::Running) => format!("{engine_id} is working"),
            Some(TimelineStatus::Completed) => "Completed".to_string(),
            Some(TimelineStatus::Failed) => "Failed".to_string(),
            Some(TimelineStatus::Interrupted) => "Interrupted · ready to continue".to_string(),
            Some(TimelineStatus::Fallback) => "Automatic fallback".to_string(),
            Some(TimelineStatus::Exhausted) => "Fallback exhausted".to_string(),
            Some(TimelineStatus::PermissionRequired) => {
                format!("{engine_id} is waiting for your approval")
            }
            Some(TimelineStatus::Worker) => "Worker outcome".to_string(),
            Some(TimelineStatus::Queued) => "Engine switch queued".to_string(),
            Some(TimelineStatus::Cancelled) => "Engine switch cancelled".to_string(),
            Some(TimelineStatus::Switched) => "Engine switched".to_string(),
            None => "Ready".to_string(),
        }
    }

    /// Everything that describes "the session I am looking at" must reset
    /// together so a previous session's profile/policy/timeline can never
    /// leak into the next one (React `resetSessionSurface`).
    pub fn reset_session_surface(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.active_session_id = None;
        self.session_detail = None;
        self.team_run = None;
        self.timeline.clear();
        self.chat.clear();
        self.open_handoffs.clear();
        self.pending_engine = None;
        self.transcript_sharing = false;
        self.set_selected_engine("alva", window, cx);
        self.set_selected_profile("workspace_write", window, cx);
    }

    pub fn set_selected_engine(
        &self,
        engine_id: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let index = ENGINE_OPTIONS
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
        self.workspace = Some(detail.session.workspace.clone());
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
        self.set_selected_engine(
            &self.session_detail.as_ref().unwrap().session.engine_id,
            window,
            cx,
        );
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
        Ok(())
    }

    /// Batch 2+: wire Run to the migrated backend `run_persisted`. Emits are
    /// collected locally and applied in one pass (the emit callback must be
    /// Send, so it cannot touch UI state directly).
    pub fn run(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(workspace) = self.workspace.clone() else {
            window.push_notification("Choose a workspace before running an engine.", cx);
            return;
        };
        let prompt = self.prompt.read(cx).value().to_string();
        if prompt.trim().is_empty() || self.busy {
            return;
        }
        let engine_id = self.selected_engine(cx);
        let profile = self.selected_profile(cx);
        let product_session_id = self.active_session_id.clone();
        let store = Arc::clone(&self.store);
        let engines = Arc::clone(&self.active_engines);
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
            let mut events = Vec::new();
            let outcome = run_persisted(
                &store,
                {
                    let ws_path = PathBuf::from(&workspace);
                    let engines = Arc::clone(&engines);
                    move |target, product_session_id, profile| {
                        let hist = history_dir.clone().unwrap_or_else(|| ws_path.clone());
                        let engine = build_engine(&target.engine_id, profile, &ws_path, &hist)?;
                        // Register the live engine so Stop can cancel it;
                        // fallback replaces the entry with the new engine.
                        engines
                            .lock()
                            .unwrap()
                            .insert(product_session_id.to_string(), Arc::clone(&engine));
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
                |event| events.push(event),
            )
            .await;
            let outcome_session = outcome.as_ref().ok().map(|o| o.product_session_id.clone());
            if let Ok(outcome) = outcome {
                events.push(TimelineEvent {
                    status: if outcome.failed {
                        TimelineStatus::Failed
                    } else {
                        TimelineStatus::Completed
                    },
                    engine_id: engine_id.clone(),
                    product_session_id: outcome.product_session_id.clone(),
                    message: outcome.result.clone(),
                    permission_request: None,
                });
            }
            if let Some(hub) = weak.upgrade() {
                hub.update(cx, |hub, cx| {
                    for event in events {
                        let event = ProjectedEvent::from_live(event);
                        let timeline_index = hub.timeline.len();
                        hub.chat
                            .push(ProjectedChatItem::from_live(&event, timeline_index));
                        hub.timeline.push(event);
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
        let Some(session_id) = self.active_session_id.clone() else {
            self.set_selected_engine(engine_id, window, cx);
            self.pending_engine = None;
            return;
        };
        let target = match EngineTarget::for_engine(engine_id) {
            Ok(target) => target,
            Err(error) => {
                self.push_local_failure(&error, cx);
                return;
            }
        };
        let store = Arc::clone(&self.store);
        let reason = format!("User explicitly selected {engine_id}");
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
                    Err(error) => hub.push_local_failure(&error, cx),
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
    }
}

pub fn profile_label_of(profile: &str) -> String {
    profile_label(profile)
}
