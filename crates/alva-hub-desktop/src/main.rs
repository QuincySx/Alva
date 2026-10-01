// INPUT:  alva_hub_desktop, tauri app-data paths, dialog, tracing_subscriber
// OUTPUT: binary with workspace/run/switch/session/policy/Inspector IPC and timestamped timeline events over the crash-repaired Hub store
// POS:    Tauri 2 entrypoint, user-authorized multi-engine Desktop state owner, and product event bridge.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alva_engine_runtime::{EngineRuntime, PermissionDecision};
use alva_hub_core::{HubRuntime, ProductSessionId};
use alva_hub_desktop::{
    build_engine_for_target_with_inspector, create_managed_session_workspace,
    diagnose_provider_inference, discover_provider_models, discover_worktrees,
    permission_eligibility, probe_readiness, run_persisted_with_target, EngineEligibility,
    EngineReadiness, EngineTarget, FallbackPolicy, HubInspectionSnapshot, HubInspectorService,
    HubSessionDetail, HubSessionSummary, HubStore, ModelRoute, PermissionProfile,
    PermissionRouter, ProjectSummary, ProviderCatalog, ProviderDiagnosticResult, ProviderProfile,
    RunOutcome, RunRequest, SaveProviderProfile, CreateProviderProfile, SwitchOutcome, SystemProbe,
    TaskClaim,
    TimelineEvent, TimelineStatus, WorktreeDiscoveryError, WorktreeInfo, WorkspaceBinding,
};
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};

fn timeline_now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as i64
}

struct DesktopState {
    store: HubStore,
    providers: ProviderCatalog,
    history_dir: PathBuf,
    /// Shared with every Alva adapter so Inspector snapshots remain live and
    /// historical reads use the same authoritative native record store.
    inspector: HubInspectorService,
    /// Live per-session HubRuntime so a user Stop can route through
    /// `HubRuntime::cancel` (seam ticket 13 batch 4). Filled by the run seam
    /// for the duration of each turn; entries live only while a run is live.
    active_hubs: Mutex<HashMap<String, HubRuntime>>,
    /// Pending engine permission requests keyed to the exact native session
    /// that asked, so user decisions never leak into a fallback engine.
    permission_router: Arc<PermissionRouter>,
    /// Worker ids whose queue pump is currently running, so each worker's
    /// durable queue has at most one consumer.
    worker_pumps: Mutex<std::collections::HashSet<String>>,
    /// Root holding every Hub-managed worktree; the discard guard requires
    /// a path under it before running any destructive git command.
    worktree_root: PathBuf,
    /// Root for one-directory-per-conversation workspaces explicitly chosen
    /// from the new-session surface. These are not cleaned up implicitly.
    session_workspace_root: PathBuf,
}

impl DesktopState {
    fn execution_target(&self, request: &RunRequest) -> Result<(PathBuf, String), String> {
        let Some(product_session_id) = request.product_session_id.as_deref() else {
            EngineTarget::for_engine(&request.engine_id)?;
            return Ok((request.workspace.clone(), request.engine_id.clone()));
        };
        self.store
            .get_session(product_session_id)?
            .map(|detail| {
                (
                    PathBuf::from(detail.session.workspace),
                    detail.session.engine_id,
                )
            })
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))
    }
}

/// Build the product readiness probe from the same normalized route snapshot
/// used by Settings, Composer, fallback and runtime resolution.
fn provider_backed_probe(
    store: &HubStore,
    providers: &ProviderCatalog,
    workspace: Option<PathBuf>,
) -> Result<SystemProbe, String> {
    let mut provider_engines = std::collections::HashSet::new();
    for route in providers.routes(workspace.as_deref())? {
        if route.available && providers.target_uses_api(&route.target, workspace.as_deref())? {
            provider_engines.insert(route.target.engine_id);
        }
    }
    let mut probe = SystemProbe::new(Some(store), workspace);
    for engine_id in provider_engines {
        probe = probe.with_provider_authenticated(engine_id, true);
    }
    Ok(probe)
}

fn same_route_identity(left: &EngineTarget, right: &EngineTarget) -> bool {
    left.engine_id == right.engine_id
        && left.provider_id == right.provider_id
        && left.model_id == right.model_id
}

/// Effective product routes: provider validity plus fast engine readiness.
/// This is the snapshot consumed by both Settings and the Composer.
fn effective_model_routes(
    store: &HubStore,
    providers: &ProviderCatalog,
    workspace: Option<PathBuf>,
) -> Result<Vec<ModelRoute>, String> {
    let mut routes = providers.routes(workspace.as_deref())?;
    let probe = provider_backed_probe(store, providers, workspace.clone())?;
    for route in &mut routes {
        if !route.available {
            continue;
        }
        let uses_provider_api = providers.target_uses_api(&route.target, workspace.as_deref())?;
        if let Some(reason) =
            probe.quick_veto_for_target(&route.target.engine_id, uses_provider_api)
        {
            route.available = false;
            route.unavailable_reason = Some(reason);
        }
    }
    Ok(routes)
}

fn canonical_available_route(
    store: &HubStore,
    providers: &ProviderCatalog,
    workspace: Option<&std::path::Path>,
    requested: &EngineTarget,
) -> Result<ModelRoute, String> {
    let mut route = effective_model_routes(
        store,
        providers,
        workspace.map(std::path::Path::to_path_buf),
    )?
    .into_iter()
    .find(|route| same_route_identity(&route.target, requested))
    .ok_or_else(|| {
        format!(
            "route '{}/{}/{}' is not configured",
            requested.engine_id, requested.provider_id, requested.model_id
        )
    })?;
    if !route.available {
        return Err(route
            .unavailable_reason
            .clone()
            .unwrap_or_else(|| "the selected model route is unavailable".into()));
    }
    let canonical = providers
        .route_for_target(requested, workspace)?
        .ok_or_else(|| {
            format!(
                "route '{}/{}/{}' is not configured",
                requested.engine_id, requested.provider_id, requested.model_id
            )
        })?;
    route.target = canonical.target;
    Ok(route)
}

#[tauri::command]
async fn run_hub_prompt(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: RunRequest,
    selected_target: Option<EngineTarget>,
    run_token: Option<String>,
) -> Result<RunOutcome, String> {
    let selected_target = if request.product_session_id.is_none() {
        if run_token
            .as_deref()
            .filter(|token| !token.trim().is_empty())
            .is_none()
        {
            return Err("a new Hub run requires a correlation token".into());
        }
        let requested_target = match selected_target {
            Some(target) => target,
            None => state
                .providers
                .default_target(Some(&request.workspace))?
                .map(|route| route.target)
                .ok_or(
                    "Choose an available default model in Settings before starting a session.",
                )?,
        };
        Some(
            canonical_available_route(
                &state.store,
                &state.providers,
                Some(&request.workspace),
                &requested_target,
            )?
            .target,
        )
    } else {
        if selected_target.is_some() {
            return Err("a resumed Hub session cannot override its pinned model route".into());
        }
        None
    };
    let (workspace, _engine_id) = state.execution_target(&request)?;
    execute_session_run(app, &state, workspace, request, selected_target, run_token).await
}

#[derive(Clone, Serialize)]
struct DesktopTimelineEvent {
    #[serde(flatten)]
    event: TimelineEvent,
    run_token: Option<String>,
}

/// The one execution wiring both user prompts and team worker dispatches go
/// through: engine registry, permission routing, health veto, event bridge.
async fn execute_session_run(
    app: AppHandle,
    state: &DesktopState,
    workspace: PathBuf,
    request: RunRequest,
    selected_target: Option<EngineTarget>,
    run_token: Option<String>,
) -> Result<RunOutcome, String> {
    let history_dir = state.history_dir.clone();
    let inspector = state.inspector.clone();
    let health_store = state.store.clone();
    let health_workspace = workspace.clone();
    let build_providers = state.providers.clone();
    let health_providers = state.providers.clone();
    // Run-scoped engine clones for permission routing only (the router stores
    // the engine it must reach when a decision lands). Dropped when the run
    // returns, so it cannot leak or drift (ticket 13 batch 4).
    let live_engines = Arc::new(Mutex::new(HashMap::<String, Arc<dyn EngineRuntime>>::new()));
    let permission_engines = Arc::clone(&live_engines);
    let active_hubs = &state.active_hubs;
    let router: &Arc<PermissionRouter> = &state.permission_router;
    let autonomy_store = state.store.clone();
    // How this session shows up on a grouped approval card: aggregation must
    // never hide WHICH worker is asking.
    let session_key = request.product_session_id.clone().unwrap_or_default();
    let requester_label = match state.store.team_worker_label(&session_key) {
        Ok(Some(label)) => label,
        Ok(None) => format!("{} session", request.engine_id),
        Err(error) => {
            tracing::warn!(%error, "failed to label the approval requester");
            format!("{} session", request.engine_id)
        }
    };
    // Approvals only ever merge within one collaboration scope.
    let approval_scope = state
        .store
        .collaboration_scope(&session_key)
        .unwrap_or_else(|_| session_key.clone());
    // Autonomy is an explicit per-run choice; it decides whether a RAISED
    // request still needs a human, never what the engine may attempt.
    let autonomous_run = state
        .store
        .is_autonomous_scope(&session_key)
        .unwrap_or(false);
    let outcome = run_persisted_with_target(
        &state.store,
        move |target, product_session_id, permission_profile| {
            let provider_config = build_providers.resolve_target(target, &workspace)?;
            let engine = build_engine_for_target_with_inspector(
                target,
                provider_config,
                permission_profile,
                &workspace,
                &history_dir,
                &inspector,
            )?;
            // The engine is registered into the run seam's own HubRuntime
            // (published via the active_hubs sink); keep a run-scoped clone
            // only for permission routing.
            live_engines
                .lock()
                .unwrap()
                .insert(product_session_id.to_string(), Arc::clone(&engine));
            Ok(engine)
        },
        // Fast readiness veto for fallback candidates: missing binaries and
        // signed-out engines are skipped with the reason. quick_veto avoids
        // subprocess probes, so it never blocks the async runtime; version
        // incompatibility surfaces as a classified attempt failure instead.
        move |target| {
            let uses_provider_api = match health_providers.resolve_target(target, &health_workspace)
            {
                Ok(provider) => provider.is_some(),
                Err(reason) => return Some(reason),
            };
            SystemProbe::new(Some(&health_store), Some(health_workspace.clone()))
                .quick_veto_for_target(&target.engine_id, uses_provider_api)
        },
        request,
        selected_target,
        Some(active_hubs),
        move |event| {
            if let Some(info) = &event.permission_request {
                // The live-engines entry is the adapter currently streaming
                // this session, i.e. the one that emitted the request.
                let engine = permission_engines
                    .lock()
                    .unwrap()
                    .get(&event.product_session_id)
                    .cloned();
                match engine {
                    Some(engine) => router.register(
                        &event.product_session_id,
                        &info.request_id,
                        engine,
                        &info.engine_session_id,
                        alva_hub_desktop::PermissionMember {
                            scope: if approval_scope.is_empty() {
                                event.product_session_id.clone()
                            } else {
                                approval_scope.clone()
                            },
                            product_session_id: event.product_session_id.clone(),
                            request_id: info.request_id.clone(),
                            requester: requester_label.clone(),
                            tool_name: info.tool_name.clone(),
                            resource: info.resource.clone(),
                        },
                    ),
                    None => tracing::warn!(
                        product_session = %event.product_session_id,
                        request = %info.request_id,
                        "permission request arrived without a live engine registration; \
                         the approval card will not be answerable"
                    ),
                }
                if autonomous_run {
                    // The user opted this run out of per-request approval:
                    // answer it here, and still leave an audit trail.
                    let router = Arc::clone(router);
                    let store = autonomy_store.clone();
                    let session = event.product_session_id.clone();
                    let request_id = info.request_id.clone();
                    let tool_name = info.tool_name.clone();
                    let resource = info.resource.clone();
                    tauri::async_runtime::spawn(async move {
                        if let Err(error) = router
                            .respond(
                                &session,
                                &request_id,
                                PermissionDecision::Allow {
                                    updated_input: None,
                                },
                            )
                            .await
                        {
                            tracing::warn!(%error, "autonomous approval could not be delivered");
                            return;
                        }
                        let summary = format!(
                            "Auto-allowed '{tool_name}'{} (autonomous TeamRun)",
                            resource
                                .map(|resource| format!(" on {resource}"))
                                .unwrap_or_default()
                        );
                        if let Err(error) = store.record_permission_decision(&session, &summary) {
                            tracing::warn!(%error, "failed to record autonomous approval");
                        }
                    });
                }
            }
            if matches!(
                event.status,
                TimelineStatus::Completed
                    | TimelineStatus::Failed
                    | TimelineStatus::Interrupted
                    | TimelineStatus::Fallback
                    | TimelineStatus::Exhausted
            ) {
                // The emitting attempt reached a terminal state (or fell
                // back); its approvals are dead and must not be answerable.
                router.clear_session(&event.product_session_id);
            }
            if let Err(error) = app.emit(
                "hub_timeline",
                DesktopTimelineEvent {
                    event,
                    run_token: run_token.clone(),
                },
            ) {
                tracing::warn!(%error, "failed to emit Hub timeline event");
            }
        },
    )
    .await;
    // R84: 只有本调用真正认领并流过一轮（outcome Ok——认领前的拒绝，例如
    // already running，以 Err 返回）才允许清理残留控制面。无条件按 pid
    // 清除会把仍在运行的 A 的 Stop 路由与审批入口一并剥掉。认领后的自身
    // 清理由 run seam 的代际移除负责，这里的兜底只在 Ok 时安全。
    if outcome.is_ok() && !session_key.is_empty() {
        state.active_hubs.lock().unwrap().remove(&session_key);
        state.permission_router.clear_session(&session_key);
    }
    outcome
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionDecisionRequest {
    product_session_id: String,
    request_id: String,
    allow: bool,
    message: Option<String>,
}

#[tauri::command]
async fn respond_hub_permission(
    state: State<'_, DesktopState>,
    request: PermissionDecisionRequest,
) -> Result<alva_hub_desktop::PermissionAnswerOutcome, String> {
    let member = state
        .permission_router
        .groups()
        .into_iter()
        .flat_map(|group| group.members)
        .find(|member| {
            member.product_session_id == request.product_session_id
                && member.request_id == request.request_id
        });
    let decision = if request.allow {
        PermissionDecision::Allow {
            updated_input: None,
        }
    } else {
        PermissionDecision::Deny {
            message: request
                .message
                .unwrap_or_else(|| "Denied by the user".to_string()),
        }
    };
    match state
        .permission_router
        .respond(&request.product_session_id, &request.request_id, decision)
        .await
    {
        Ok(()) => {
            if let Some(member) = member {
                let summary = format!(
                    "{} '{}'{} for {}",
                    if request.allow { "Allowed" } else { "Denied" },
                    member.tool_name,
                    member
                        .resource
                        .as_deref()
                        .map(|resource| format!(" on {resource}"))
                        .unwrap_or_default(),
                    member.requester,
                );
                if let Err(error) = state
                    .store
                    .record_permission_decision(&request.product_session_id, &summary)
                {
                    tracing::warn!(%error, "failed to record Hub approval audit");
                }
            }
            Ok(alva_hub_desktop::PermissionAnswerOutcome::Delivered)
        }
        Err(message) if alva_hub_desktop::is_stale_permission_error(&message) => {
            // Already answered / attempt ended / engine auto-denied before the
            // user replied: never record an audit row, tell the UI the card is
            // stale (ticket 03).
            Ok(alva_hub_desktop::PermissionAnswerOutcome::Expired)
        }
        Err(message) => Err(message),
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct GroupDecisionRequest {
    group_key: String,
    allow: bool,
    message: Option<String>,
    /// Optional subset: when present, only these request ids are answered,
    /// so a user can approve some members of an equivalent group and leave
    /// the rest pending.
    request_ids: Option<Vec<String>>,
}

/// Pending approvals, merged by equivalence for a single card.
#[tauri::command]
fn get_permission_groups(state: State<'_, DesktopState>) -> Vec<alva_hub_desktop::PermissionGroup> {
    state.permission_router.groups()
}

/// Answer every (selected) member of one equivalence group, and record what
/// the decision covered.
#[tauri::command]
async fn respond_permission_group(
    state: State<'_, DesktopState>,
    request: GroupDecisionRequest,
) -> Result<usize, String> {
    let mut members = state.permission_router.group_members(&request.group_key);
    if let Some(selected) = &request.request_ids {
        members.retain(|member| selected.contains(&member.request_id));
    }
    if members.is_empty() {
        return Err("those approval requests are no longer pending".to_string());
    }
    let outcome = state
        .permission_router
        .respond_many(&members, request.allow, request.message.clone())
        .await?;
    let answered = outcome.answered.clone();
    // Audit on each covered session: a grouped decision must be traceable
    // back to every request it actually answered.
    for member in &answered {
        let summary = format!(
            "{} '{}'{} for {} (grouped decision covering {} request(s))",
            if request.allow { "Allowed" } else { "Denied" },
            member.tool_name,
            member
                .resource
                .as_deref()
                .map(|resource| format!(" on {resource}"))
                .unwrap_or_default(),
            member.requester,
            answered.len()
        );
        if let Err(error) = state
            .store
            .record_permission_decision(&member.product_session_id, &summary)
        {
            tracing::warn!(%error, "failed to record Hub approval audit");
        }
    }
    if !outcome.failures.is_empty() {
        // Some engines were told and others were not — the user must see it.
        return Err(format!(
            "answered {} of {} request(s); the rest could not be delivered: {}",
            answered.len(),
            members.len(),
            outcome.failures.join("; ")
        ));
    }
    Ok(answered.len())
}

#[tauri::command]
fn set_team_autonomy(
    state: State<'_, DesktopState>,
    team_run_id: String,
    autonomous: bool,
) -> Result<alva_hub_desktop::TeamRunDetail, String> {
    state.store.set_team_autonomy(&team_run_id, autonomous)
}

#[tauri::command]
fn get_permission_eligibility(profile: PermissionProfile) -> Vec<EngineEligibility> {
    permission_eligibility(profile)
}

#[tauri::command]
fn create_team_run(
    state: State<'_, DesktopState>,
    lead_session_id: String,
    worker_engine_id: String,
) -> Result<alva_hub_desktop::TeamRunDetail, String> {
    let target = EngineTarget::for_engine(&worker_engine_id)?;
    state.store.create_team_run(&lead_session_id, &target)
}

#[tauri::command]
fn get_team_run_for_lead(
    state: State<'_, DesktopState>,
    lead_session_id: String,
) -> Result<Option<alva_hub_desktop::TeamRunDetail>, String> {
    state.store.team_run_for_lead(&lead_session_id)
}

#[tauri::command]
fn add_team_worker(
    state: State<'_, DesktopState>,
    team_run_id: String,
    worker_engine_id: String,
    isolated: Option<bool>,
) -> Result<alva_hub_desktop::TeamRunDetail, String> {
    let target = EngineTarget::for_engine(&worker_engine_id)?;
    if !isolated.unwrap_or(false) {
        return state.store.add_team_worker(&team_run_id, &target, None);
    }
    let run = state
        .store
        .get_team_run(&team_run_id)?
        .ok_or_else(|| format!("team run '{team_run_id}' was not found"))?;
    let lead = state
        .store
        .get_session(&run.lead_session_id)?
        .ok_or("The lead session no longer exists.")?;
    // Register the worker first so the worktree carries its real id: an
    // orphan left by a crash can then be correlated back to a worker.
    let registered = state.store.add_team_worker(&team_run_id, &target, None)?;
    let worker_id = registered
        .workers
        .last()
        .map(|worker| worker.worker_id.clone())
        .ok_or("the worker was not registered")?;
    let workspace = PathBuf::from(&lead.session.workspace);
    let worktree =
        alva_hub_desktop::create_worker_worktree(&workspace, &state.worktree_root, &worker_id)?;
    // If attaching the worktree fails, it must not leak.
    match state.store.attach_worker_worktree(&worker_id, &worktree) {
        Ok(()) => state
            .store
            .get_team_run(&team_run_id)?
            .ok_or_else(|| format!("team run '{team_run_id}' was not found")),
        Err(error) => {
            let _ = alva_hub_desktop::discard_worktree(
                &workspace,
                &PathBuf::from(&worktree.path),
                &worktree.branch,
                &state.worktree_root,
            );
            Err(error)
        }
    }
}

/// Release a worker's live runtime while keeping identity, history and
/// queue: cancel anything running, drop the engine registration, and let
/// the pump drain out (a hibernated worker claims nothing).
#[tauri::command]
async fn hibernate_worker(
    app: AppHandle,
    state: State<'_, DesktopState>,
    worker_id: String,
) -> Result<alva_hub_desktop::TeamWorker, String> {
    let worker = state
        .store
        .team_worker(&worker_id)?
        .ok_or("That worker was not found.")?;
    // Flip the lifecycle FIRST so the pump stops claiming the next task the
    // moment the current one ends.
    let hibernated = state
        .store
        .set_worker_lifecycle(&worker_id, alva_hub_desktop::WorkerLifecycle::Hibernated)?;
    debug_assert_eq!(hibernated.worker_session_id, worker.worker_session_id);
    // R85: 复用 release_worker_runtime 的保留语义——"没有运行中的任务"是
    // 预期路径（正常清理）；引擎 cancel 真实失败时保留可达的 hub 句柄与
    // 审批路由并如实告警，不得把活着的运行交给无主状态。失败后的语义：
    // 生命周期已 Hibernated（队列不再被认领），但该次运行仍可 Stop，直到
    // 一次成功的取消完成清理。
    release_worker_runtime(&app, &state, &hibernated.worker_session_id).await;
    Ok(hibernated)
}

/// Restore an archived (or hibernated) worker to the active surface.
#[tauri::command]
fn restore_worker(
    app: AppHandle,
    state: State<'_, DesktopState>,
    worker_id: String,
) -> Result<alva_hub_desktop::TeamWorker, String> {
    let worker = state
        .store
        .set_worker_lifecycle(&worker_id, alva_hub_desktop::WorkerLifecycle::Active)?;
    // Anything queued while it slept starts flowing again.
    if state.store.has_queued_task(&worker_id).unwrap_or(false) {
        ensure_worker_pump(app, worker_id);
    }
    Ok(worker)
}

#[tauri::command]
async fn archive_worker(
    app: AppHandle,
    state: State<'_, DesktopState>,
    worker_id: String,
) -> Result<alva_hub_desktop::TeamWorker, String> {
    // ONE lifecycle write, before releasing the runtime: a two-step
    // hibernate→archive would leave a window where an incoming task
    // auto-resumes the worker and is then archived out from under the user.
    let archived = state
        .store
        .set_worker_lifecycle(&worker_id, alva_hub_desktop::WorkerLifecycle::Archived)?;
    release_worker_runtime(&app, &state, &archived.worker_session_id).await;
    Ok(archived)
}

/// Cancel anything the worker session is running and drop its live handles.
/// Distinguishes "nothing was running" (expected) from a genuine cancel
/// failure, where dropping the engine registration would strand a live
/// process with no way to stop it.
async fn release_worker_runtime(app: &AppHandle, state: &DesktopState, worker_session_id: &str) {
    match stop_attempt_inner(app, state, worker_session_id.to_string()).await {
        Ok(()) => {}
        Err(error) if error.contains("no running attempt") => {
            tracing::debug!(%error, "no live worker runtime to release");
        }
        Err(error) => {
            tracing::warn!(
                %error,
                worker_session = %worker_session_id,
                "failed to release worker runtime; keeping the engine handle so it stays stoppable"
            );
            state.permission_router.clear_session(worker_session_id);
            return;
        }
    }
    state.active_hubs.lock().unwrap().remove(worker_session_id);
    state.permission_router.clear_session(worker_session_id);
}

/// End a TeamRun. Workers are archived, never deleted.
#[tauri::command]
async fn complete_team_run(
    app: AppHandle,
    state: State<'_, DesktopState>,
    team_run_id: String,
) -> Result<alva_hub_desktop::TeamRunDetail, String> {
    let run = state
        .store
        .get_team_run(&team_run_id)?
        .ok_or_else(|| format!("team run '{team_run_id}' was not found"))?;
    for worker in &run.workers {
        if let Err(error) = stop_attempt_inner(&app, &state, worker.worker_session_id.clone()).await
        {
            tracing::debug!(%error, "no live runtime to release for worker");
        }
        state
            .active_hubs
            .lock()
            .unwrap()
            .remove(&worker.worker_session_id);
    }
    state.store.complete_team_run(&team_run_id)
}

/// Read-only: what adopting this isolated worker would change.
#[tauri::command]
async fn preview_worker_adoption(
    state: State<'_, DesktopState>,
    worker_id: String,
) -> Result<alva_hub_desktop::AdoptionPreview, String> {
    let (workspace, worktree, verification) = adoption_context(&state, &worker_id)?;
    tauri::async_runtime::spawn_blocking(move || {
        alva_hub_desktop::adoption_preview(&workspace, &worktree, verification)
    })
    .await
    .map_err(|error| format!("adoption preview task failed: {error}"))?
}

/// Apply an isolated worker's work to the shared workspace. Only reachable
/// from an explicit user/lead adoption after previewing.
#[tauri::command]
async fn adopt_worker_work(
    state: State<'_, DesktopState>,
    team_run_id: String,
    worker_id: String,
) -> Result<Vec<String>, String> {
    // Adopting a worktree that is still being written captures a torn
    // snapshot; require the worker to be idle first.
    if state.store.worker_has_active_task(&worker_id)? {
        return Err(
            "This worker is still working. Wait for it to finish (or interrupt it) before adopting."
                .to_string(),
        );
    }
    let (workspace, worktree, _) = adoption_context(&state, &worker_id)?;
    let adopted = tauri::async_runtime::spawn_blocking(move || {
        alva_hub_desktop::adopt_into_workspace(&workspace, &worktree)
    })
    .await
    .map_err(|error| format!("adoption task failed: {error}"))??;
    state.store.record_adoption(
        &team_run_id,
        &worker_id,
        true,
        &format!("Adopted {} file(s): {}", adopted.len(), adopted.join(", ")),
    )?;
    Ok(adopted)
}

/// Throw the isolated worker's worktree away. Never touches the shared
/// workspace; the durable worker session and its history survive.
#[tauri::command]
async fn discard_worker_worktree(
    state: State<'_, DesktopState>,
    team_run_id: String,
    worker_id: String,
) -> Result<(), String> {
    let run = state
        .store
        .get_team_run(&team_run_id)?
        .ok_or_else(|| format!("team run '{team_run_id}' was not found"))?;
    let worker = run
        .workers
        .iter()
        .find(|worker| worker.worker_id == worker_id)
        .ok_or("That worker is not part of this run.")?;
    let (Some(path), Some(branch)) = (worker.worktree_path.clone(), worker.worktree_branch.clone())
    else {
        return Err("That worker is not isolated.".to_string());
    };
    let lead = state
        .store
        .get_session(&run.lead_session_id)?
        .ok_or("The lead session no longer exists.")?;
    let workspace = PathBuf::from(&lead.session.workspace);
    let managed_root = state.worktree_root.clone();
    let shared = workspace.clone();
    tauri::async_runtime::spawn_blocking(move || {
        alva_hub_desktop::discard_worktree(&shared, &PathBuf::from(&path), &branch, &managed_root)
    })
    .await
    .map_err(|error| format!("discard task failed: {error}"))??;
    // The session's workspace pointed at the now-deleted worktree; move it
    // back to the shared workspace so the durable session stays usable.
    state
        .store
        .clear_worker_worktree(&worker_id, &workspace.to_string_lossy())?;
    state.store.record_adoption(
        &team_run_id,
        &worker_id,
        false,
        "Discarded the isolated worktree without adopting its changes",
    )?;
    Ok(())
}

/// (shared workspace, worktree, verification evidence) for an isolated worker.
fn adoption_context(
    state: &DesktopState,
    worker_id: &str,
) -> Result<(PathBuf, PathBuf, Vec<String>), String> {
    let run = state
        .store
        .team_run_for_worker(worker_id)?
        .ok_or("That worker is not part of any team run.")?;
    let worker = run
        .workers
        .iter()
        .find(|worker| worker.worker_id == worker_id)
        .ok_or("That worker is not part of this run.")?;
    let worktree = worker
        .worktree_path
        .clone()
        .ok_or("That worker is not isolated.")?;
    let lead = state
        .store
        .get_session(&run.lead_session_id)?
        .ok_or("The lead session no longer exists.")?;
    let verification = run
        .tasks
        .iter()
        .filter(|task| task.worker_id == worker_id)
        .flat_map(|task| {
            task.verification.iter().map(|evidence| {
                format!(
                    "{} {}",
                    evidence.tool_name,
                    match evidence.succeeded {
                        Some(true) => "ok",
                        Some(false) => "failed",
                        None => "ran",
                    }
                )
            })
        })
        .collect();
    Ok((
        PathBuf::from(&lead.session.workspace),
        PathBuf::from(worktree),
        verification,
    ))
}

/// Enqueue only — the per-worker pump consumes the durable FIFO queue, so
/// tasks for different workers genuinely run in parallel while each worker
/// stays serial.
#[tauri::command]
async fn dispatch_worker_task(
    app: AppHandle,
    state: State<'_, DesktopState>,
    team_run_id: String,
    worker_id: String,
    scope: String,
    claims: Option<Vec<alva_hub_desktop::ResourceClaim>>,
) -> Result<alva_hub_desktop::WorkerTask, String> {
    let task = state.store.enqueue_worker_task(
        &team_run_id,
        &worker_id,
        &scope,
        &claims.unwrap_or_default(),
    )?;
    ensure_worker_pump(app, worker_id);
    Ok(task)
}

#[tauri::command]
fn update_worker_task(
    state: State<'_, DesktopState>,
    task_id: String,
    scope: String,
) -> Result<alva_hub_desktop::WorkerTask, String> {
    state.store.update_worker_task_scope(&task_id, &scope)
}

#[tauri::command]
fn cancel_worker_task(
    state: State<'_, DesktopState>,
    task_id: String,
) -> Result<alva_hub_desktop::WorkerTask, String> {
    state.store.cancel_worker_task(&task_id)
}

/// Interrupt whatever the worker session is running right now.
#[tauri::command]
async fn interrupt_worker(
    app: AppHandle,
    state: State<'_, DesktopState>,
    worker_session_id: String,
) -> Result<(), String> {
    stop_attempt_inner(&app, &state, worker_session_id).await
}

/// How long a blocked worker waits before re-recording the wait, so a
/// wedged holder surfaces instead of an invisible forever-wait.
const RESOURCE_WAIT_REANNOUNCE: std::time::Duration = std::time::Duration::from_secs(60);

fn ensure_worker_pump(app: AppHandle, worker_id: String) {
    {
        let state = app.state::<DesktopState>();
        let mut pumps = state.worker_pumps.lock().unwrap();
        if !pumps.insert(worker_id.clone()) {
            return; // already pumping this worker's queue
        }
    }
    tauri::async_runtime::spawn(async move {
        let mut announced_wait = false;
        let mut waiting_since: Option<std::time::Instant> = None;
        let mut errors_in_a_row = 0u32;
        let mut dormant = false;
        loop {
            let claimed = {
                let state = app.state::<DesktopState>();
                state.store.claim_next_worker_task(&worker_id)
            };
            match claimed {
                Ok(TaskClaim::Claimed(task)) => {
                    announced_wait = false;
                    waiting_since = None;
                    errors_in_a_row = 0;
                    run_worker_task(&app, *task).await;
                }
                Ok(TaskClaim::Blocked {
                    task_id,
                    blocked_by,
                }) => {
                    // The head of this queue wants resources another running
                    // task holds. Wait — never reorder past it — and record
                    // the wait once per blocked stretch.
                    let waited = waiting_since.get_or_insert_with(std::time::Instant::now);
                    // Re-announce periodically so a wedged holder is visible
                    // rather than an invisible forever-wait.
                    let overdue = waited.elapsed() >= RESOURCE_WAIT_REANNOUNCE;
                    if !announced_wait || overdue {
                        if overdue {
                            waiting_since = Some(std::time::Instant::now());
                        }
                        let state = app.state::<DesktopState>();
                        if let Ok(Some(task)) = state.store.worker_task_by_id(&task_id) {
                            match state.store.record_resource_wait(
                                &task.team_run_id,
                                &worker_id,
                                &blocked_by,
                            ) {
                                // Only a recorded wait counts as announced.
                                Ok(()) => announced_wait = true,
                                Err(error) => {
                                    tracing::warn!(%error, "failed to record Hub resource wait")
                                }
                            }
                            let summary = blocked_by
                                .iter()
                                .map(|blocked| {
                                    format!(
                                        "{} (held by worker {})",
                                        blocked.resource, blocked.holder_worker_id
                                    )
                                })
                                .collect::<Vec<_>>()
                                .join(", ");
                            if let Ok(Some(run)) = state.store.get_team_run(&task.team_run_id) {
                                let engine_id = run
                                    .workers
                                    .iter()
                                    .find(|worker| worker.worker_id == worker_id)
                                    .map(|worker| worker.engine_id.clone())
                                    .unwrap_or_default();
                                let _ = app.emit(
                                    "hub_timeline",
                                    TimelineEvent {
                                        status: TimelineStatus::Worker,
                                        engine_id,
                                        product_session_id: run.lead_session_id.clone(),
                                        created_at: timeline_now_ms(),
                                        message: Some(format!("Waiting on {summary}")),
                                        item_id: Some(task.task_id.clone()),
                                        activity: None,
                                        message_position: None,
                                        permission_request: None,
                                    },
                                );
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
                }
                Ok(TaskClaim::Dormant) => {
                    // Hibernated or archived: the queue stays, the pump goes
                    // away, and the tail must NOT respawn it.
                    dormant = true;
                    break;
                }
                Ok(TaskClaim::Empty) => break,
                Err(error) => {
                    // Back off before the tail re-spawns us, so a persistent
                    // store error cannot become a hot respawn loop.
                    errors_in_a_row += 1;
                    tracing::warn!(%error, worker = %worker_id, "worker queue pump stopped");
                    let backoff = std::time::Duration::from_millis(
                        500u64.saturating_mul(1 << errors_in_a_row.min(5)),
                    );
                    tokio::time::sleep(backoff).await;
                    break;
                }
            }
        }
        let state = app.state::<DesktopState>();
        state.worker_pumps.lock().unwrap().remove(&worker_id);
        // A task enqueued between the final empty claim and the removal
        // above would otherwise sit unconsumed until the next dispatch. A
        // dormant worker is skipped: its queue is intentionally parked.
        if !dormant && state.store.has_queued_task(&worker_id).unwrap_or(false) {
            ensure_worker_pump(app.clone(), worker_id);
        }
    });
}

async fn run_worker_task(app: &AppHandle, task: alva_hub_desktop::WorkerTask) {
    let state = app.state::<DesktopState>();
    // Tell the UI the queue moved: the claimed task is running now, so its
    // Interrupt control arms and Edit/Cancel disappear.
    if let Ok(Some(run)) = state.store.get_team_run(&task.team_run_id) {
        let engine_id = run
            .workers
            .iter()
            .find(|worker| worker.worker_id == task.worker_id)
            .map(|worker| worker.engine_id.clone())
            .unwrap_or_default();
        if let Err(error) = app.emit(
            "hub_timeline",
            TimelineEvent {
                status: TimelineStatus::Worker,
                engine_id,
                product_session_id: run.lead_session_id.clone(),
                created_at: timeline_now_ms(),
                message: None,
                item_id: Some(task.task_id.clone()),
                activity: None,
                message_position: None,
                permission_request: None,
            },
        ) {
            tracing::warn!(%error, "failed to emit Hub worker claim event");
        }
    }
    let context = (|| -> Result<_, String> {
        let run = state
            .store
            .get_team_run(&task.team_run_id)?
            .ok_or("team run vanished")?;
        let worker = run
            .workers
            .iter()
            .find(|worker| worker.worker_id == task.worker_id)
            .cloned()
            .ok_or("worker vanished")?;
        let session = state
            .store
            .get_session(&worker.worker_session_id)?
            .ok_or("worker session vanished")?;
        Ok((run, worker, session))
    })();
    let (run, worker, session) = match context {
        Ok(context) => context,
        Err(error) => {
            tracing::warn!(%error, task = %task.task_id, "worker task context unavailable");
            let _ = state
                .store
                .finish_worker_task(&task.task_id, false, Some(&error));
            return;
        }
    };
    let workspace = PathBuf::from(&session.session.workspace);
    let request = RunRequest {
        prompt: task.scope.clone(),
        workspace: workspace.clone(),
        engine_id: session.session.engine_id.clone(),
        product_session_id: Some(worker.worker_session_id.clone()),
        permission_profile: None,
    };
    let outcome = execute_session_run(app.clone(), &state, workspace, request, None, None).await;
    let finished = match outcome {
        Ok(outcome) => state.store.finish_worker_task(
            &task.task_id,
            !outcome.failed,
            outcome.result.as_deref(),
        ),
        Err(error) => state
            .store
            .finish_worker_task(&task.task_id, false, Some(&error)),
    };
    match finished {
        Ok(task) => {
            let summary = alva_hub_desktop::redact_text(&format!(
                "Worker ({}) {}{}",
                worker.engine_id,
                task.status,
                task.result
                    .as_deref()
                    .map(|result| {
                        let brief: String = result.chars().take(160).collect();
                        format!(": {brief}")
                    })
                    .unwrap_or_default()
            ));
            if let Err(error) = app.emit(
                "hub_timeline",
                TimelineEvent {
                    status: TimelineStatus::Worker,
                    engine_id: worker.engine_id.clone(),
                    product_session_id: run.lead_session_id.clone(),
                    created_at: timeline_now_ms(),
                    message: Some(summary),
                    item_id: Some(task.task_id.clone()),
                    activity: None,
                    message_position: None,
                    permission_request: None,
                },
            ) {
                tracing::warn!(%error, "failed to emit Hub worker outcome event");
            }
        }
        Err(error) => {
            tracing::warn!(%error, task = %task.task_id, "failed to finish worker task");
        }
    }
}

#[derive(serde::Serialize)]
struct RepairResult {
    outcome: alva_hub_desktop::RepairOutcome,
    readiness: EngineReadiness,
}

#[tauri::command]
async fn get_repair_plan(
    state: State<'_, DesktopState>,
    workspace: Option<String>,
) -> Result<Vec<alva_hub_desktop::RepairAction>, String> {
    let probe =
        provider_backed_probe(&state.store, &state.providers, workspace.map(PathBuf::from))?;
    tauri::async_runtime::spawn_blocking(move || {
        probe_readiness(&probe)
            .iter()
            .flat_map(alva_hub_desktop::repair_plan)
            .collect()
    })
    .await
    .map_err(|error| format!("repair planning task failed: {error}"))
}

#[tauri::command]
async fn run_engine_repair(
    state: State<'_, DesktopState>,
    engine_id: String,
    kind: alva_hub_desktop::RepairKind,
    workspace: Option<String>,
) -> Result<RepairResult, String> {
    let workspace = workspace.map(PathBuf::from);
    let probe = provider_backed_probe(&state.store, &state.providers, workspace.clone())?;
    let store = state.store.clone();
    let providers = state.providers.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use alva_hub_desktop::{classify_engine, EngineProbe};
        // The argv comes from the engine's CURRENT readiness, server-side.
        let readiness = classify_engine(&engine_id, probe.facts(&engine_id));
        let argv = alva_hub_desktop::repair_argv(&readiness, kind)?;
        let outcome = alva_hub_desktop::run_repair_command(&argv, None);
        // Re-probe so the caller sees the post-repair version/auth/health.
        let refreshed_probe = provider_backed_probe(&store, &providers, workspace)?;
        let readiness = classify_engine(&engine_id, refreshed_probe.facts(&engine_id));
        Ok(RepairResult { outcome, readiness })
    })
    .await
    .map_err(|error| format!("engine repair task failed: {error}"))?
}

#[tauri::command]
fn set_transcript_sharing(
    state: State<'_, DesktopState>,
    product_session_id: String,
    enabled: bool,
) -> Result<(), String> {
    state
        .store
        .set_transcript_sharing(&product_session_id, enabled)
}

#[tauri::command]
async fn stop_hub_attempt(
    app: AppHandle,
    state: State<'_, DesktopState>,
    product_session_id: String,
) -> Result<(), String> {
    stop_attempt_inner(&app, &state, product_session_id).await
}

async fn stop_attempt_inner(
    _app: &AppHandle,
    state: &DesktopState,
    product_session_id: String,
) -> Result<(), String> {
    let detail = state
        .store
        .get_session(&product_session_id)?
        .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
    if detail.session.status != "running" {
        return Err("This Hub session has no running attempt to stop.".to_string());
    }
    let hub = state
        .active_hubs
        .lock()
        .unwrap()
        .get(&product_session_id)
        .cloned()
        .ok_or("The running attempt is not owned by this app instance.")?;
    // R91: 被停 attempt 的身份在 await cancel 之前捕获——与 R77 的 HTTP
    // 语义一致。同分支 resume 会铸造新 attempt，attempt（而非 branch）才
    // 是 Stop 的身份。
    let stop_attempt = state
        .store
        .running_attempt_id(&product_session_id)?
        .unwrap_or_default();
    // R86: 审批路由不得在 cancel 之前清除——cancel 失败时原权限请求仍须
    // 可回答，运行不得在丢掉审批路由后悬挂。清理只发生在取消事实成功写入
    // （该轮确由本次 Stop 终止）之后。
    // R51/R91: nothing is recorded before the cancel — a failed cancel must
    // not leave a cancelled terminal behind.
    hub.cancel(&ProductSessionId::from_existing(&product_session_id))
        .await
        .map_err(|error| error.to_string())?;
    // R91: 取消事实绑定被停的 attempt。cancel ack 到来时该 attempt 已终态
    // 或已被取代（迟到 ack），则不写事实、不清审批——不与真实终态冲突，
    // 也不销毁仍等待的权限请求。
    let cancelled = state.store.record_attempt_cancelled_bound(
        &product_session_id,
        &stop_attempt,
        "User stopped the attempt",
    )?;
    if cancelled {
        // The stopped attempt's approvals die with it — only now, when the
        // cancel actually took effect on the pinned attempt.
        state.permission_router.clear_session(&product_session_id);
    }
    // R91: 此处不再广播 Interrupted——run seam 在引擎流真实结束时结算唯一
    // 终态（与 R65 后的 HTTP 语义一致）。抢先广播会在自然 Completed/Failed
    // 之上追加互相冲突的取消终态，UI 文案声称"已退出"时流可能仍在输出。
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualSwitchRequest {
    product_session_id: String,
    target: EngineTarget,
    reason: String,
}

#[tauri::command]
fn switch_hub_engine(
    state: State<'_, DesktopState>,
    request: ManualSwitchRequest,
) -> Result<SwitchOutcome, String> {
    let session = state
        .store
        .get_session(&request.product_session_id)?
        .ok_or_else(|| format!("Hub session '{}' was not found", request.product_session_id))?;
    let workspace = PathBuf::from(session.session.workspace);
    let target = canonical_available_route(
        &state.store,
        &state.providers,
        Some(&workspace),
        &request.target,
    )?
    .target;
    state
        .store
        .request_engine_switch(&request.product_session_id, &target, &request.reason)
}

#[tauri::command]
fn list_model_providers(
    state: State<'_, DesktopState>,
    workspace: Option<String>,
) -> Result<Vec<ProviderProfile>, String> {
    state
        .providers
        .list(workspace.as_deref().map(std::path::Path::new))
}

#[tauri::command]
fn save_model_provider(
    state: State<'_, DesktopState>,
    provider: SaveProviderProfile,
    workspace: Option<String>,
) -> Result<ProviderProfile, String> {
    state
        .providers
        .save(provider, workspace.as_deref().map(std::path::Path::new))
}

#[tauri::command]
fn create_model_provider(
    state: State<'_, DesktopState>,
    provider: CreateProviderProfile,
    workspace: Option<String>,
) -> Result<ProviderProfile, String> {
    state
        .providers
        .create(provider, workspace.as_deref().map(std::path::Path::new))
}

#[tauri::command]
fn delete_model_provider(
    state: State<'_, DesktopState>,
    id: String,
    workspace: Option<String>,
) -> Result<ProviderProfile, String> {
    state
        .providers
        .delete(&id, workspace.as_deref().map(std::path::Path::new))
}

#[tauri::command]
fn list_model_routes(
    state: State<'_, DesktopState>,
    workspace: Option<String>,
) -> Result<Vec<ModelRoute>, String> {
    effective_model_routes(&state.store, &state.providers, workspace.map(PathBuf::from))
}

#[tauri::command]
fn get_default_model_route(
    state: State<'_, DesktopState>,
    workspace: Option<String>,
) -> Result<Option<ModelRoute>, String> {
    let workspace = workspace.map(PathBuf::from);
    let Some(default) = state.providers.default_target(workspace.as_deref())? else {
        return Ok(None);
    };
    let effective = effective_model_routes(&state.store, &state.providers, workspace)?;
    Ok(effective
        .into_iter()
        .find(|route| same_route_identity(&route.target, &default.target))
        .or(Some(default)))
}

#[tauri::command]
fn set_default_model_route(
    state: State<'_, DesktopState>,
    target: EngineTarget,
    workspace: Option<String>,
) -> Result<ModelRoute, String> {
    let workspace = workspace.map(PathBuf::from);
    let route = canonical_available_route(
        &state.store,
        &state.providers,
        workspace.as_deref(),
        &target,
    )?;
    state
        .providers
        .set_default_target(route.target, workspace.as_deref())
}

#[tauri::command]
async fn discover_models_for_provider(
    state: State<'_, DesktopState>,
    provider_id: String,
    workspace: Option<String>,
) -> Result<ProviderDiagnosticResult, String> {
    let workspace = workspace.map(PathBuf::from);
    let config = state
        .providers
        .resolve_provider_for_diagnostics(&provider_id, workspace.as_deref())?;
    let kind = config
        .kind
        .clone()
        .ok_or_else(|| format!("provider '{provider_id}' has no protocol kind"))?;
    Ok(discover_provider_models(&kind, &config).await)
}

#[tauri::command]
async fn test_model_route(
    state: State<'_, DesktopState>,
    target: EngineTarget,
    workspace: Option<String>,
) -> Result<ProviderDiagnosticResult, String> {
    let workspace = workspace.map(PathBuf::from);
    let route = canonical_available_route(
        &state.store,
        &state.providers,
        workspace.as_deref(),
        &target,
    )?;
    if !state
        .providers
        .target_uses_api(&route.target, workspace.as_deref())?
    {
        return Err("Inference diagnostics require an API-backed provider route.".into());
    }
    let mut config = state
        .providers
        .resolve_provider_for_diagnostics(&route.target.provider_id, workspace.as_deref())?;
    config.model = route.target.model_id;
    let kind = config
        .kind
        .clone()
        .ok_or_else(|| "the selected provider has no protocol kind".to_string())?;
    Ok(diagnose_provider_inference(&kind, &config).await)
}

#[tauri::command]
fn create_session_workspace(state: State<'_, DesktopState>) -> Result<WorkspaceBinding, String> {
    create_managed_session_workspace(&state.session_workspace_root)
}

// ---- H04/H05: project aggregation + read-only worktree discovery ----------

#[tauri::command]
fn list_projects(state: State<'_, DesktopState>) -> Result<Vec<ProjectSummary>, String> {
    state.store.list_projects()
}

#[tauri::command]
fn create_project(
    state: State<'_, DesktopState>,
    workspace_path: String,
    display_name: Option<String>,
) -> Result<ProjectSummary, String> {
    state
        .store
        .create_project(&workspace_path, display_name.as_deref())
}

#[tauri::command]
fn rename_project(
    state: State<'_, DesktopState>,
    project_id: String,
    display_name: String,
) -> Result<(), String> {
    state.store.rename_project(&project_id, &display_name)
}

/// Forgets a Project. Sessions are detached, never deleted; the directory on
/// disk is never touched.
#[tauri::command]
fn delete_project(state: State<'_, DesktopState>, project_id: String) -> Result<(), String> {
    state.store.delete_project(&project_id)
}

#[tauri::command]
fn set_session_project(
    state: State<'_, DesktopState>,
    product_session_id: String,
    project_id: Option<String>,
) -> Result<(), String> {
    state
        .store
        .set_session_project(&product_session_id, project_id.as_deref())
}

/// Read-only worktree listing for a project's workspace. The inner Result is
/// a product STATE (not-git / missing directory are visible states), so the
/// UI can present them without treating a non-Git project as a failure.
#[tauri::command]
fn discover_project_worktrees(
    state: State<'_, DesktopState>,
    workspace_path: String,
) -> Result<Result<Vec<WorktreeInfo>, WorktreeDiscoveryError>, String> {
    let path = PathBuf::from(workspace_path);
    Ok(discover_worktrees(&path, Some(&state.worktree_root)))
}

#[tauri::command]
fn list_hub_sessions(state: State<'_, DesktopState>) -> Result<Vec<HubSessionSummary>, String> {
    state.store.list_sessions()
}

#[tauri::command]
fn get_hub_session(
    state: State<'_, DesktopState>,
    product_session_id: String,
) -> Result<Option<HubSessionDetail>, String> {
    state.store.get_session(&product_session_id)
}

#[tauri::command]
async fn get_hub_inspection(
    state: State<'_, DesktopState>,
    product_session_id: String,
    native_session_id: Option<String>,
) -> Result<HubInspectionSnapshot, String> {
    state
        .inspector
        .inspect(
            &state.store,
            &product_session_id,
            native_session_id.as_deref(),
        )
        .await
}

#[tauri::command]
async fn get_hub_tool_content(
    state: State<'_, DesktopState>,
    product_session_id: String,
    content_id: String,
    chunk_index: usize,
) -> Result<Option<alva_hub_desktop::ToolContentChunk>, String> {
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        store.read_tool_content(&product_session_id, &content_id, chunk_index)
    }).await.map_err(|_| "tool content reader unavailable".to_string())?
}

#[tauri::command]
fn add_session_memory(
    state: State<'_, DesktopState>,
    product_session_id: String,
    entry: String,
) -> Result<Vec<String>, String> {
    state.store.add_session_memory(&product_session_id, &entry)
}

#[tauri::command]
fn get_global_fallback_policy(state: State<'_, DesktopState>) -> Result<FallbackPolicy, String> {
    state.store.global_fallback_policy()
}

#[tauri::command]
fn set_global_fallback_policy(
    state: State<'_, DesktopState>,
    targets: Vec<EngineTarget>,
) -> Result<FallbackPolicy, String> {
    state.store.set_global_fallback_policy(targets)
}

#[tauri::command]
fn set_session_fallback_policy(
    state: State<'_, DesktopState>,
    product_session_id: String,
    targets: Vec<EngineTarget>,
) -> Result<FallbackPolicy, String> {
    state
        .store
        .set_session_fallback_policy(&product_session_id, targets)
}

#[tauri::command]
async fn get_engine_readiness(
    state: State<'_, DesktopState>,
    workspace: Option<String>,
) -> Result<Vec<EngineReadiness>, String> {
    // The probe copies its store evidence up front, so the slow CLI probing
    // (`--version` child processes) runs off the IPC thread.
    let probe =
        provider_backed_probe(&state.store, &state.providers, workspace.map(PathBuf::from))?;
    tauri::async_runtime::spawn_blocking(move || probe_readiness(&probe))
        .await
        .map_err(|error| format!("engine readiness probe task failed: {error}"))
}

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,alva_hub=info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        // Official-guidance links must actually open from the packaged app:
        // a bare <a target="_blank"> does nothing inside a wry webview.
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let store =
                HubStore::open(data_dir.join("alva-hub.db")).map_err(std::io::Error::other)?;
            let shared_provider_config = app.path().home_dir()?.join(".alva").join("config.json");
            let providers = ProviderCatalog::open_with_shared_and_legacy_config(
                data_dir.join("model-providers.json"),
                shared_provider_config,
            )
            .map_err(std::io::Error::other)?;
            let queued_workers = store.active_workers_with_queued_tasks().unwrap_or_default();
            let history_dir = data_dir.join("alva-history");
            let inspector =
                HubInspectorService::open(&history_dir).map_err(std::io::Error::other)?;
            app.manage(DesktopState {
                store,
                providers,
                history_dir,
                inspector,
                active_hubs: Mutex::new(HashMap::new()),
                worker_pumps: Mutex::new(std::collections::HashSet::new()),
                worktree_root: alva_hub_desktop::worktree_root(&data_dir),
                session_workspace_root: data_dir.join("session-workspaces"),
                permission_router: Arc::new(PermissionRouter::new()),
            });
            // Durable queues survive restarts: resume a pump for every worker
            // that still has queued work.
            for worker_id in queued_workers {
                ensure_worker_pump(app.handle().clone(), worker_id);
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            run_hub_prompt,
            switch_hub_engine,
            list_model_providers,
            save_model_provider,
            create_model_provider,
            delete_model_provider,
            list_model_routes,
            get_default_model_route,
            set_default_model_route,
            discover_models_for_provider,
            test_model_route,
            create_session_workspace,
            list_projects,
            create_project,
            rename_project,
            delete_project,
            set_session_project,
            discover_project_worktrees,
            list_hub_sessions,
            get_hub_session,
            get_hub_inspection,
            get_hub_tool_content,
            add_session_memory,
            get_global_fallback_policy,
            set_global_fallback_policy,
            set_session_fallback_policy,
            get_engine_readiness,
            stop_hub_attempt,
            respond_hub_permission,
            get_permission_eligibility,
            get_permission_groups,
            respond_permission_group,
            set_team_autonomy,
            set_transcript_sharing,
            get_repair_plan,
            run_engine_repair,
            create_team_run,
            get_team_run_for_lead,
            add_team_worker,
            dispatch_worker_task,
            preview_worker_adoption,
            adopt_worker_work,
            discard_worker_worktree,
            hibernate_worker,
            restore_worker,
            archive_worker,
            complete_team_run,
            update_worker_task,
            cancel_worker_task,
            interrupt_worker
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
