// INPUT:  alva_hub_desktop, tauri app-data paths, dialog, tracing_subscriber
// OUTPUT: binary with crash-repaired persisted run/session-list/session-detail IPC
// POS:    Tauri 2 entrypoint and instance-locked private Hub Desktop state owner.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use std::path::PathBuf;

use alva_hub_desktop::{
    build_alva_engine, run_persisted, HubSessionDetail, HubSessionSummary, HubStore, RunOutcome,
    RunRequest,
};
use tauri::{AppHandle, Emitter, Manager, State};

struct DesktopState {
    store: HubStore,
    history_dir: PathBuf,
}

impl DesktopState {
    fn workspace_for(&self, request: &RunRequest) -> Result<PathBuf, String> {
        let Some(product_session_id) = request.product_session_id.as_deref() else {
            return Ok(request.workspace.clone());
        };
        self.store
            .get_session(product_session_id)?
            .map(|detail| PathBuf::from(detail.session.workspace))
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))
    }
}

#[tauri::command]
async fn run_alva_prompt(
    app: AppHandle,
    state: State<'_, DesktopState>,
    request: RunRequest,
) -> Result<RunOutcome, String> {
    let workspace = state.workspace_for(&request)?;
    let engine = build_alva_engine(&workspace, &state.history_dir)?;
    run_persisted(&state.store, engine, request, move |event| {
        if let Err(error) = app.emit("hub_timeline", event) {
            tracing::warn!(%error, "failed to emit Hub timeline event");
        }
    })
    .await
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
            run_alva_prompt,
            list_hub_sessions,
            get_hub_session
        ])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
