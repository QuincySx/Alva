// INPUT:  tauri, alva_app_core (BaseAgent + extensions), alva_llm_provider, alva_kernel_core
// OUTPUT: binary `alva-agent-tauri` — Tauri desktop shell hosting a React frontend
// POS:    L6 app layer. Replaces alva-app (GPUI). Minimal MVP: one chat session wired
//         through Tauri IPC (`send_message`, `cancel_run`) and an `agent_event` emit stream.

#![cfg_attr(
    all(not(debug_assertions), target_os = "windows"),
    windows_subsystem = "windows"
)]

mod agent;
mod bundled_skills;
mod mcp;
mod provider_api;
mod skills;
mod sqlite_session;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
                tracing_subscriber::EnvFilter::new("info,alva_kernel_core=info,alva_app_core=info")
            }),
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
            agent::run::send_message,
            agent::run::cancel_run,
            agent::discovery::list_providers,
            agent::session_cmds::list_sessions,
            agent::session_cmds::create_session,
            agent::session_cmds::switch_session,
            agent::session_cmds::delete_session,
            agent::discovery::list_skill_sources,
            agent::discovery::scan_skills,
            agent::discovery::list_all_skills,
            agent::discovery::list_mcp_servers,
            agent::discovery::list_remote_models,
            agent::discovery::test_provider_connection,
            agent::session_cmds::get_session_record,
            agent::session_cmds::list_session_events,
            agent::discovery::list_plugins,
            agent::discovery::set_plugin_enabled,
            agent::discovery::open_inspector_window,
            agent::session_cmds::set_session_workspace,
            agent::session_cmds::open_session_workspace,
            agent::approval::respond_approval,
            agent::approval::list_pending_approvals,
            agent::gateway::start_gateway,
            agent::gateway::stop_gateway,
        ])
        .run(tauri::generate_context!())
        .expect("failed to run tauri app");
}
