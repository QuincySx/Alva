// INPUT:  alva_hub_desktop, tauri app-data paths, dialog, tracing_subscriber
// OUTPUT: binary with run/switch/session/policy IPC over the crash-repaired Hub store
// POS:    Tauri 2 entrypoint and user-authorized multi-engine Desktop state owner.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;

use alva_hub_desktop::{
    build_engine, run_persisted, EngineTarget, FallbackPolicy, HubSessionDetail, HubSessionSummary,
    HubStore, RunOutcome, RunRequest, SwitchOutcome,
};
use serde::Deserialize;
use tauri::{AppHandle, Emitter, Manager, State};

struct DesktopState {
    store: HubStore,
    history_dir: PathBuf,
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
    let (workspace, engine_id) = state.execution_target(&request)?;
    let engine = build_engine(&engine_id, &workspace, &state.history_dir)?;
    run_persisted(&state.store, engine, request, move |event| {
        if let Err(error) = app.emit("hub_timeline", event) {
            tracing::warn!(%error, "failed to emit Hub timeline event");
        }
    })
    .await
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
            set_session_fallback_policy
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
