// INPUT:  alva_hub_desktop, tauri, tauri_plugin_dialog, tracing_subscriber
// OUTPUT: binary `alva-hub-desktop` with run_alva_prompt IPC
// POS:    Tauri 2 entrypoint for the Hub Desktop ticket-01 walking skeleton.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

use alva_hub_desktop::{build_alva_engine, run_once, RunOutcome, RunRequest};
use tauri::{AppHandle, Emitter};

#[tauri::command]
async fn run_alva_prompt(app: AppHandle, request: RunRequest) -> Result<RunOutcome, String> {
    let engine = build_alva_engine(&request.workspace)?;
    run_once(engine, request, move |event| {
        if let Err(error) = app.emit("hub_timeline", event) {
            tracing::warn!(%error, "failed to emit Hub timeline event");
        }
    })
    .await
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
        .invoke_handler(tauri::generate_handler![run_alva_prompt])
        .run(tauri::generate_context!())
        .expect("failed to run ALVA Hub Desktop");
}
