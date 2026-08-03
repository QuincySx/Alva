// INPUT:  alva_hub_desktop, tauri app-data paths, dialog, tracing_subscriber
// OUTPUT: binary with run/switch/session/policy IPC over the crash-repaired Hub store
// POS:    Tauri 2 entrypoint and user-authorized multi-engine Desktop state owner.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use alva_engine_runtime::{EngineRuntime, PermissionDecision};
use alva_hub_desktop::{
    build_engine, permission_eligibility, probe_readiness, run_persisted, EngineEligibility,
    EngineReadiness, EngineTarget, FallbackPolicy, HubSessionDetail, HubSessionSummary, HubStore,
    PermissionProfile, PermissionRouter, RunOutcome, RunRequest, SwitchOutcome, SystemProbe,
    TaskClaim, TimelineEvent, TimelineStatus,
};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};

struct DesktopState {
    store: HubStore,
    history_dir: PathBuf,
    /// Engine currently executing each product session, so a user Stop can
    /// reach the live adapter. Entries live only for the duration of a run.
    active_engines: Mutex<HashMap<String, Arc<dyn EngineRuntime>>>,
    /// Pending engine permission requests keyed to the exact native session
    /// that asked, so user decisions never leak into a fallback engine.
    permission_router: PermissionRouter,
    /// Worker ids whose queue pump is currently running, so each worker's
    /// durable queue has at most one consumer.
    worker_pumps: Mutex<std::collections::HashSet<String>>,
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

#[tauri::command]
async fn run_hub_prompt(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: RunRequest,
) -> Result<RunOutcome, String> {
    let (workspace, _engine_id) = state.execution_target(&request)?;
    execute_session_run(app, &state, workspace, request).await
}

/// The one execution wiring both user prompts and team worker dispatches go
/// through: engine registry, permission routing, health veto, event bridge.
async fn execute_session_run(
    app: AppHandle,
    state: &DesktopState,
    workspace: PathBuf,
    request: RunRequest,
) -> Result<RunOutcome, String> {
    let history_dir = state.history_dir.clone();
    let health_store = state.store.clone();
    let health_workspace = workspace.clone();
    let registry = Arc::new(Mutex::new(Vec::<String>::new()));
    let build_registry = Arc::clone(&registry);
    let engines = &state.active_engines;
    let router = &state.permission_router;
    let outcome = run_persisted(
        &state.store,
        move |target, product_session_id, permission_profile| {
            let engine = build_engine(
                &target.engine_id,
                permission_profile,
                &workspace,
                &history_dir,
            )?;
            // Register the live engine so `stop_hub_attempt` can cancel it;
            // fallback replaces the entry with the newly active engine.
            engines
                .lock()
                .unwrap()
                .insert(product_session_id.to_string(), Arc::clone(&engine));
            build_registry
                .lock()
                .unwrap()
                .push(product_session_id.to_string());
            Ok(engine)
        },
        // Fast readiness veto for fallback candidates: missing binaries and
        // signed-out engines are skipped with the reason. quick_veto avoids
        // subprocess probes, so it never blocks the async runtime; version
        // incompatibility surfaces as a classified attempt failure instead.
        move |target| {
            SystemProbe::new(Some(&health_store), Some(health_workspace.clone()))
                .quick_veto(&target.engine_id)
        },
        request,
        move |event| {
            if let Some(info) = &event.permission_request {
                // The active-engines entry is the adapter currently streaming
                // this session, i.e. the one that emitted the request.
                let engine = engines
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
                    ),
                    None => tracing::warn!(
                        product_session = %event.product_session_id,
                        request = %info.request_id,
                        "permission request arrived without a live engine registration; \
                         the approval card will not be answerable"
                    ),
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
            if let Err(error) = app.emit("hub_timeline", event) {
                tracing::warn!(%error, "failed to emit Hub timeline event");
            }
        },
    )
    .await;
    let mut engines = state.active_engines.lock().unwrap();
    for product_session_id in registry.lock().unwrap().drain(..) {
        engines.remove(&product_session_id);
        state.permission_router.clear_session(&product_session_id);
    }
    drop(engines);
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
) -> Result<(), String> {
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
    state
        .permission_router
        .respond(&request.product_session_id, &request.request_id, decision)
        .await
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
) -> Result<alva_hub_desktop::TeamRunDetail, String> {
    let target = EngineTarget::for_engine(&worker_engine_id)?;
    state.store.add_team_worker(&team_run_id, &target)
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
                                        message: Some(format!("Waiting on {summary}")),
                                        permission_request: None,
                                    },
                                );
                            }
                        }
                    }
                    tokio::time::sleep(std::time::Duration::from_millis(250)).await;
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
        // above would otherwise sit unconsumed until the next dispatch.
        if state.store.has_queued_task(&worker_id).unwrap_or(false) {
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
                message: None,
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
    let outcome = execute_session_run(app.clone(), &state, workspace, request).await;
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
                    message: Some(summary),
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
    let probe = SystemProbe::new(Some(&state.store), workspace.map(PathBuf::from));
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
    let probe = SystemProbe::new(Some(&state.store), workspace.clone());
    let store = state.store.clone();
    tauri::async_runtime::spawn_blocking(move || {
        use alva_hub_desktop::{classify_engine, EngineProbe};
        // The argv comes from the engine's CURRENT readiness, server-side.
        let readiness = classify_engine(&engine_id, probe.facts(&engine_id));
        let argv = alva_hub_desktop::repair_argv(&readiness, kind)?;
        let outcome = alva_hub_desktop::run_repair_command(&argv, None);
        // Re-probe so the caller sees the post-repair version/auth/health.
        let refreshed_probe = SystemProbe::new(Some(&store), workspace);
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
    app: &AppHandle,
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
    let engine = state
        .active_engines
        .lock()
        .unwrap()
        .get(&product_session_id)
        .cloned()
        .ok_or("The running attempt is not owned by this app instance.")?;
    let engine_session_id = detail
        .session
        .engine_session_id
        .ok_or("The engine has not started a native session yet; try again in a moment.")?;
    state
        .store
        .record_attempt_cancelled(&product_session_id, "User stopped the attempt")?;
    // The stopped attempt's approvals die with it — clear them before the
    // cancel so a stale Allow can no longer reach the cancelled session.
    state.permission_router.clear_session(&product_session_id);
    engine
        .cancel(&engine_session_id)
        .await
        .map_err(|error| error.to_string())?;
    if let Err(error) = app.emit(
        "hub_timeline",
        TimelineEvent {
            status: TimelineStatus::Interrupted,
            engine_id: detail.session.engine_id,
            product_session_id,
            message: Some("User stopped the attempt".to_string()),
            permission_request: None,
        },
    ) {
        tracing::warn!(%error, "failed to emit Hub cancel event");
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ManualSwitchRequest {
    product_session_id: String,
    engine_id: String,
    reason: String,
}

#[tauri::command]
fn switch_hub_engine(
    state: State<'_, DesktopState>,
    request: ManualSwitchRequest,
) -> Result<SwitchOutcome, String> {
    let target = EngineTarget::for_engine(&request.engine_id)?;
    state
        .store
        .request_engine_switch(&request.product_session_id, &target, &request.reason)
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
    let probe = SystemProbe::new(Some(&state.store), workspace.map(PathBuf::from));
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
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            let store =
                HubStore::open(data_dir.join("alva-hub.db")).map_err(std::io::Error::other)?;
            let queued_workers = store.workers_with_queued_tasks().unwrap_or_default();
            app.manage(DesktopState {
                store,
                history_dir: data_dir.join("alva-history"),
                active_engines: Mutex::new(HashMap::new()),
                worker_pumps: Mutex::new(std::collections::HashSet::new()),
                permission_router: PermissionRouter::new(),
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
            list_hub_sessions,
            get_hub_session,
            get_global_fallback_policy,
            set_global_fallback_policy,
            set_session_fallback_policy,
            get_engine_readiness,
            stop_hub_attempt,
            respond_hub_permission,
            get_permission_eligibility,
            set_transcript_sharing,
            get_repair_plan,
            run_engine_repair,
            create_team_run,
            get_team_run_for_lead,
            add_team_worker,
            dispatch_worker_task,
            update_worker_task,
            cancel_worker_task,
            interrupt_worker
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
