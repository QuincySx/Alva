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
    TimelineEvent, TimelineStatus,
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
            app.manage(DesktopState {
                store,
                history_dir: data_dir.join("alva-history"),
                active_engines: Mutex::new(HashMap::new()),
                permission_router: PermissionRouter::new(),
            });
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
            set_transcript_sharing
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
