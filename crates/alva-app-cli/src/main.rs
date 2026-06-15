//! alva-cli — Minimal CLI agent with session management.
//!
//! Usage:
//!   export ALVA_API_KEY=sk-...
//!   cargo run -p alva-app-core --bin alva-cli
//!
//! Commands:
//!   /new             Start a fresh session
//!   /resume          Resume latest session (or pick from list)
//!   /sessions        List all sessions in current directory
//!   /help            Show available commands
//!   /clear           Clear terminal
//!   /config          Show current config
//!   /quit /exit      Exit
//!   !<cmd>           Run shell command directly
//!
//! Sessions are stored under `.alva/sessions/` in the working directory.

mod agent_setup;
mod checkpoint;
mod commands;
mod context;
mod event_handler;
mod output;
#[allow(dead_code)] // MINI MODE:整模块暂休眠,Skills 组件加回时复用
mod bundled_skills;
mod plugins;
mod repl;
mod repl_completer;
pub mod services;
mod session;
mod settings_cmd;
mod setup;
pub mod ui;

use std::io::{self, Read as _};

use alva_app_core::AlvaPaths;
use alva_kernel_abi::agent_session::EventQuery;
use alva_kernel_abi::AgentSession;
use alva_llm_provider::ProviderConfig;

use session::JsonFileSessionManager;

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .with_writer(io::stderr)
        .init();

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("failed to build tokio runtime");

    // run() returns an exit code; all destructors for its stack locals
    // (agent, extensions, subprocess loader, plugin proxies...) run
    // inside block_on() before we drop the runtime. Previously we used
    // `std::process::exit()` inside run(), which bypassed all Drop
    // impls and caused loaded subprocess plugins to be reparented
    // instead of cleanly shut down.
    let exit_code = rt.block_on(run());

    // Give in-flight tokio tasks a bounded window to finish (reader /
    // writer loops on subprocess stdio shutting down cleanly, etc.).
    rt.shutdown_timeout(std::time::Duration::from_secs(5));

    if exit_code != 0 {
        std::process::exit(exit_code);
    }
}

async fn run() -> i32 {
    // Early dispatch: `alva plugins ...` and `alva context ...`
    // short-circuit before any agent setup — pure debugging commands
    // that need no LLM config or session init.
    let argv: Vec<String> = std::env::args().collect();
    match argv.get(1).map(|s| s.as_str()) {
        Some("plugins") => return plugins::run(&argv[2..]).await,
        Some("context") => return context::run(&argv[2..]).await,
        Some("settings") => return settings_cmd::run(&argv[2..]).await,
        _ => {}
    }

    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    // 1. Config — resolution order:
    //    a. env vars (ALVA_*) — `ProviderConfig::load` will pick these up
    //    b. project / global flat-format files (legacy CLI paths)
    //    c. shared `~/.alva/config.json` active provider — same file Tauri reads
    //    d. setup wizard
    //
    // The shared config is checked AFTER env vars + legacy files so old setups
    // keep working unchanged. New users go through the wizard or `alva settings set`.
    let config = match ProviderConfig::load(&workspace) {
        Ok(c) if !c.api_key.is_empty() => c,
        _ => match settings_cmd::try_load_provider_from_shared() {
            Some(mut shared) => {
                // Env vars still override individual fields when present.
                if let Ok(k) = std::env::var("ALVA_API_KEY") { if !k.is_empty() { shared.api_key = k; } }
                if let Ok(m) = std::env::var("ALVA_MODEL") { if !m.is_empty() { shared.model = m; } }
                if let Ok(b) = std::env::var("ALVA_BASE_URL") { if !b.is_empty() { shared.base_url = b; } }
                if let Ok(k) = std::env::var("ALVA_PROVIDER_KIND") { if !k.is_empty() { shared.kind = Some(k); } }
                shared
            }
            None => match setup::run_setup_wizard(&workspace) {
                Some(c) => c,
                None => {
                    eprintln!();
                    output::print_error("Setup incomplete. You can also configure manually:");
                    eprintln!("  export ALVA_API_KEY=sk-...");
                    eprintln!("  export ALVA_MODEL=gpt-4o");
                    eprintln!("  alva settings set anthropic --api-key sk-... --model claude-opus-4-7");
                    return 1;
                }
            }
        }
    };

    let session_manager = JsonFileSessionManager::for_workspace(&workspace);

    // 2. Build agent (provider, skills, approval channel, checkpoint callback)
    let agent_setup::AgentBundle {
        agent,
        mut approval_rx,
        checkpoint_mgr: _checkpoint_mgr,
    } = agent_setup::build_agent(&config, &workspace, &paths).await;

    // 3. Check for -p/--print mode (non-interactive, single prompt, stdout-only)
    let args: Vec<String> = std::env::args().collect();
    let print_mode = args.iter().any(|a| a == "-p" || a == "--print");

    if print_mode {
        let prompt_args: Vec<String> = args
            .iter()
            .skip(1) // skip binary name
            .filter(|a| *a != "-p" && *a != "--print")
            .cloned()
            .collect();

        let prompt = if prompt_args.is_empty() {
            let mut buf = String::new();
            io::stdin().read_to_string(&mut buf).ok();
            buf.trim().to_string()
        } else {
            prompt_args.join(" ")
        };

        if prompt.is_empty() {
            eprintln!("Error: no prompt provided. Usage: alva -p \"your prompt\"");
            return 1;
        }

        let exit_code = event_handler::run_print_mode(&agent, &prompt).await;
        return exit_code;
    }

    // 4. Print banner (interactive modes only)
    output::print_banner(&config.model, &workspace.display().to_string());
    output::print_git_status(&workspace);
    output::print_banner_end();

    // 5. Interactive — TUI by default. Inline (20 rows) unless explicitly
    //    flipped via flag/env/config. `--repl` falls back to the legacy
    //    reedline line-mode for users who prefer it.
    //
    //    UI mode resolution order (first hit wins):
    //      1. `--ui-mode <inline|fullscreen>` flag
    //      2. `ALVA_UI_MODE` env var
    //      3. `ui_mode` field in ~/.alva/config.json (shared with Tauri)
    //      4. default "inline"
    let cli_args: Vec<String> = std::env::args().collect();
    let want_repl = cli_args.iter().any(|a| a == "--repl")
        || std::env::var("ALVA_REPL").ok().as_deref() == Some("1");
    if !want_repl {
        let mut mode_override: Option<String> = None;
        if let Some(i) = cli_args.iter().position(|a| a == "--ui-mode") {
            mode_override = cli_args.get(i + 1).cloned();
        }
        if mode_override.is_none() {
            if let Ok(env_mode) = std::env::var("ALVA_UI_MODE") {
                if !env_mode.is_empty() { mode_override = Some(env_mode); }
            }
        }
        let shared_cfg = alva_app_core::config::load();
        let cfg_mode = shared_cfg.as_ref().and_then(|c| c.ui_mode.clone());
        let cfg_inline_rows = shared_cfg.as_ref().and_then(|c| c.ui_inline_rows);
        let mode_str = mode_override.or(cfg_mode);
        let viewport = ui::app::UiViewport::parse(mode_str.as_deref(), cfg_inline_rows);
        if let Err(e) = ui::app::run_tui(
            &agent,
            &config,
            &workspace,
            &paths,
            &session_manager,
            &_checkpoint_mgr,
            &mut approval_rx,
            viewport,
        )
        .await
        {
            output::print_error(&format!("TUI exited with error: {e}"));
            return 1;
        }
        return 0;
    }

    // 6. Check for single-shot mode (positional arg without leading -).
    //    Skips any value that follows `--ui-mode`/`--print` so flag
    //    arguments don't get treated as prompts.
    let argv: Vec<String> = std::env::args().collect();
    let mut prompt_arg: Option<String> = None;
    let mut i = 1;
    while i < argv.len() {
        let a = &argv[i];
        if a == "--ui-mode" { i += 2; continue; }
        if a == "-p" || a == "--print" || a == "--tui" || a == "--repl" { i += 1; continue; }
        if a.starts_with('-') { i += 1; continue; }
        prompt_arg = Some(a.clone());
        break;
    }
    if let Some(prompt) = prompt_arg {
        let initial_session = session_manager.create(&prompt).await;
        agent.swap_session(initial_session.clone()).await;
        // Append eval_config_snapshot before the first turn so RunRecord
        // captures the actual model + tool/skill/extension config used.
        // Same shape as Tauri's append_config_snapshot_if_needed.
        session_manager
            .append_config_snapshot_if_needed(&initial_session, &agent, &config.model)
            .await;
        event_handler::run_prompt(&agent, &prompt, &mut approval_rx).await;
        // Persistence is automatic — refresh the index summary, then drop a
        // structured RunRecord next to the raw event log so external tooling
        // gets the same turn / llm-call / tool-call view Tauri builds for its
        // Inspector. Same projection (alva_app_core::session_projection).
        let event_count = initial_session.count(&EventQuery::default()).await;
        session_manager.refresh_summary(initial_session.session_id(), event_count, Some(&prompt));
        session_manager.write_run_record(&initial_session).await;
        return 0;
    }

    // 7. Interactive REPL (default when not TUI / not single-shot).
    repl::run_repl(
        &agent,
        &config,
        &workspace,
        &paths,
        &session_manager,
        &_checkpoint_mgr,
        &mut approval_rx,
    )
    .await;

    0
}
