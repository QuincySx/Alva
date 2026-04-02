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
mod event_handler;
mod output;
mod repl;
mod session_store;
mod setup;

use std::io::{self, Read as _};

use alva_app_core::AlvaPaths;
use alva_provider::ProviderConfig;

use session_store::SessionStore;

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

    rt.block_on(run());
}

async fn run() {
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let paths = AlvaPaths::new(&workspace);

    // 1. Config — layered: env > project (.alva/config.json) > global (~/.config/alva/config.json)
    let config = match ProviderConfig::load(&workspace) {
        Ok(c) => c,
        Err(_) => {
            match setup::run_setup_wizard(&workspace) {
                Some(c) => c,
                None => {
                    eprintln!();
                    output::print_error("Setup incomplete. You can also configure manually:");
                    eprintln!("  export ALVA_API_KEY=sk-...");
                    eprintln!("  export ALVA_MODEL=gpt-4o");
                    return;
                }
            }
        }
    };

    let store = SessionStore::for_workspace(&workspace);

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
            std::process::exit(1);
        }

        let exit_code = event_handler::run_print_mode(&agent, &prompt).await;
        std::process::exit(exit_code);
    }

    // 4. Print banner (interactive modes only)
    output::print_banner(&config.model, &workspace.display().to_string());
    output::print_git_status(&workspace);
    output::print_banner_end();

    // 5. Check for single-shot mode (positional arg without -p)
    if let Some(prompt) = std::env::args().nth(1) {
        let session_id = store.create(&prompt);
        event_handler::run_prompt(&agent, &prompt, &mut approval_rx).await;
        let messages = agent.messages().await;
        store.save_messages(&session_id, &messages);
        return;
    }

    // 6. Interactive REPL
    repl::run_repl(
        &agent,
        &config,
        &workspace,
        &paths,
        &store,
        &_checkpoint_mgr,
        &mut approval_rx,
    )
    .await;
}
