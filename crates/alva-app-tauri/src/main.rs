// INPUT:  tauri, alva_app_core (BaseAgent + extensions), alva_llm_provider, alva_kernel_core
// OUTPUT: binary `alva-agent-tauri` — Tauri desktop shell hosting a React frontend
// POS:    L6 app layer. Replaces alva-app (GPUI). Minimal MVP: one chat session wired
//         through Tauri IPC (`send_message`, `cancel_run`) and an `agent_event` emit stream.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod agent;
mod mcp;
mod provider_api;
mod session_projection;
mod skills;
mod sqlite_session;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,alva_kernel_core=info,alva_app_core=info")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(
            agent::AppState::new(runtime.handle().clone())
                .expect("AppState init (open ~/.alva/sessions.db)"),
        )
        .invoke_handler(tauri::generate_handler![
            agent::send_message,
            agent::cancel_run,
            agent::list_providers,
            agent::list_sessions,
            agent::create_session,
            agent::switch_session,
            agent::delete_session,
            agent::list_skill_sources,
            agent::scan_skills,
            agent::list_all_skills,
            agent::list_mcp_servers,
            agent::list_remote_models,
            agent::test_provider_connection,
            agent::get_session_record,
            agent::list_session_events,
            agent::list_plugins,
            agent::set_plugin_enabled,
            agent::open_inspector_window,
            agent::set_session_workspace,
            agent::open_session_workspace,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
