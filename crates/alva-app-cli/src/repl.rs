// INPUT:  alva_app_core, alva_provider, alva_agent_runtime, checkpoint, session_store, output, event_handler
// OUTPUT: run_repl, restore_messages
// POS:    Interactive REPL loop — session management, slash commands, and user input dispatch

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AgentMessage, AlvaPaths, BaseAgent, PermissionMode};
use alva_agent_runtime::middleware::security::ApprovalRequest;
use alva_provider::{OpenAIProvider, ProviderConfig};
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::event_handler;
use crate::output;
use crate::session_store::SessionStore;

/// Restore saved messages into the agent's state.
pub(crate) async fn restore_messages(agent: &BaseAgent, messages: Vec<AgentMessage>) {
    if !messages.is_empty() {
        agent.restore_messages(messages).await;
    }
}

/// Run the interactive REPL loop with session management.
pub(crate) async fn run_repl(
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &std::path::Path,
    paths: &AlvaPaths,
    store: &SessionStore,
    checkpoint_mgr: &checkpoint::CheckpointManager,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) {
    // Auto-resume latest or start new
    let mut session_id = match store.latest() {
        Some(id) => {
            let sessions = store.list();
            let meta = sessions.iter().find(|m| m.id == id).unwrap();
            output::print_session_resumed(&id, meta.message_count, &meta.summary);

            // Restore messages — clear first to avoid stale data
            agent.new_session().await;
            let saved = store.load_messages(&id);
            if !saved.is_empty() {
                restore_messages(agent, saved).await;
            }
            id
        }
        None => {
            let id = store.create("");
            output::print_session_new(&id);
            id
        }
    };

    output::print_divider();

    loop {
        output::print_prompt();

        let mut line = String::new();
        match io::stdin().lock().read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }

                match trimmed {
                    "/quit" | "/exit" => break,
                    "/help" => {
                        output::print_help();
                    }
                    "/clear" => {
                        // ANSI clear screen + move cursor to top
                        print!("\x1B[2J\x1B[1;1H");
                        io::stdout().flush().ok();
                        output::print_banner(
                            &config.model,
                            &workspace.display().to_string(),
                        );
                        output::print_git_status(workspace);
                        output::print_banner_end();
                    }
                    "/config" => {
                        eprintln!("  Model:     {}", config.model);
                        eprintln!("  Base URL:  {}", config.base_url);
                        eprintln!("  Workspace: {}", workspace.display());
                        eprintln!("  Session:   {}", session_id);
                        eprintln!();
                        eprintln!("  Paths:");
                        let mark = |p: &std::path::Path| if p.exists() { "" } else { " (not found)" };
                        eprintln!("    Global config:  {}{}", paths.global_config().display(), mark(&paths.global_config()));
                        eprintln!("    Global MCP:     {}{}", paths.global_mcp_config().display(), mark(&paths.global_mcp_config()));
                        eprintln!("    Global skills:  {}{}", paths.global_skills_dir().display(), mark(&paths.global_skills_dir()));
                        if paths.project_config().exists() {
                            eprintln!("    Project config: {}", paths.project_config().display());
                        }
                        if paths.project_mcp_config().exists() {
                            eprintln!("    Project MCP:    {}", paths.project_mcp_config().display());
                        }
                        if paths.project_skills_dir().exists() {
                            eprintln!("    Project skills: {}", paths.project_skills_dir().display());
                        }
                    }
                    "/setup" => {
                        eprintln!("  Reconfiguring requires restart.");
                        if let Some(new_config) = crate::setup::run_setup_wizard(workspace) {
                            eprintln!();
                            eprintln!("  Configuration saved. Please restart alva-cli to use the new settings.");
                            let _ = new_config; // config saved to alva.json by wizard
                        }
                    }
                    "/plan" => {
                        let current = agent.permission_mode();
                        let new_mode = if current == PermissionMode::Plan {
                            PermissionMode::Ask
                        } else {
                            PermissionMode::Plan
                        };
                        agent.set_permission_mode(new_mode);
                        if new_mode == PermissionMode::Plan {
                            eprintln!("  Plan mode ON — read-only, no file changes or commands");
                        } else {
                            eprintln!("  Plan mode OFF — tools can modify files");
                        }
                    }
                    "/model" => {
                        let current = agent.model_id().await;
                        eprintln!("  Current model: {}", current);
                        eprintln!("  Usage: /model <model_id>");
                    }
                    cmd if cmd.starts_with("/model ") => {
                        let model_id = cmd.strip_prefix("/model ").unwrap().trim();
                        if model_id.is_empty() {
                            let current = agent.model_id().await;
                            eprintln!("  Current model: {}", current);
                        } else {
                            let mut new_config = config.clone();
                            new_config.model = model_id.to_string();
                            let new_model = Arc::new(OpenAIProvider::new(new_config));
                            agent.set_model(new_model).await;
                            eprintln!("  Switched to model: {}", model_id);
                        }
                    }
                    "/rewind" => {
                        let checkpoints = checkpoint_mgr.list();
                        if checkpoints.is_empty() {
                            eprintln!("  No checkpoints available.");
                            continue;
                        }
                        eprintln!("  Checkpoints:");
                        for (i, cp) in checkpoints.iter().enumerate().take(10) {
                            let date = chrono::DateTime::from_timestamp_millis(cp.created_at)
                                .map(|d| d.format("%H:%M:%S").to_string())
                                .unwrap_or_default();
                            eprintln!(
                                "  [{}] {} | {} files | {}",
                                i + 1,
                                date,
                                cp.files.len(),
                                cp.description
                            );
                        }
                        eprint!(
                            "  Rewind to [1-{}] or Enter to cancel: ",
                            checkpoints.len().min(10)
                        );
                        io::stderr().flush().ok();

                        let mut choice = String::new();
                        if io::stdin().lock().read_line(&mut choice).is_ok() {
                            let trimmed = choice.trim();
                            if !trimmed.is_empty() {
                                if let Ok(idx) = trimmed.parse::<usize>() {
                                    if idx >= 1 && idx <= checkpoints.len().min(10) {
                                        let cp = &checkpoints[idx - 1];
                                        match checkpoint_mgr.rewind(&cp.id) {
                                            Ok(restored) => {
                                                eprintln!(
                                                    "  Restored {} files from checkpoint {}",
                                                    restored.len(),
                                                    cp.id
                                                );
                                                for f in &restored {
                                                    eprintln!("    - {}", f);
                                                }
                                            }
                                            Err(e) => {
                                                output::print_error(&format!(
                                                    "rewind failed: {}",
                                                    e
                                                ));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                    "/new" => {
                        // Save current, start fresh
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);

                        agent.new_session().await;
                        session_id = store.create("");
                        output::print_session_new(&session_id);
                        output::print_divider();
                    }
                    "/fork" => {
                        // Fork: save current, create new session with same messages
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);

                        let old_id = session_id.clone();
                        session_id = store.create("");
                        store.save_messages(&session_id, &messages);
                        eprintln!("  Forked from {} → {}", &old_id[..8.min(old_id.len())], &session_id[..8.min(session_id.len())]);
                        eprintln!("  {} messages carried over. Try a different approach.", messages.len());
                        output::print_divider();
                    }
                    "/resume" => {
                        // Save current first
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);

                        let sessions = store.list();
                        if sessions.is_empty() {
                            output::print_error("No sessions found.");
                            continue;
                        }

                        eprintln!("Sessions:");
                        for (i, s) in sessions.iter().enumerate().take(10) {
                            let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                                .map(|d| d.format("%m-%d %H:%M").to_string())
                                .unwrap_or_default();
                            let marker = if s.id == session_id { " ◀" } else { "" };
                            eprintln!(
                                "  [{}] {} | {} msgs | {}{}",
                                i + 1,
                                date,
                                s.message_count,
                                s.summary,
                                marker,
                            );
                        }
                        eprint!("Pick [1-{}] or Enter for latest: ", sessions.len().min(10));
                        io::stderr().flush().ok();

                        let mut choice = String::new();
                        if io::stdin().lock().read_line(&mut choice).is_ok() {
                            let idx: usize = choice.trim().parse().unwrap_or(1);
                            if idx >= 1 && idx <= sessions.len().min(10) {
                                let picked = &sessions[idx - 1];
                                session_id = picked.id.clone();

                                agent.new_session().await;
                                let saved = store.load_messages(&session_id);
                                restore_messages(agent, saved).await;

                                output::print_session_resumed(
                                    &session_id,
                                    agent.messages().await.len(),
                                    &picked.summary,
                                );
                            }
                        }
                        output::print_divider();
                    }
                    "/sessions" => {
                        let sessions = store.list();
                        if sessions.is_empty() {
                            eprintln!("No sessions.");
                        } else {
                            for s in sessions.iter().take(20) {
                                let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                                    .map(|d| d.format("%m-%d %H:%M").to_string())
                                    .unwrap_or_default();
                                let marker = if s.id == session_id { " ◀" } else { "" };
                                eprintln!(
                                    "  {} | {} msgs | {}{}",
                                    date, s.message_count, s.summary, marker,
                                );
                            }
                        }
                    }
                    cmd if cmd.starts_with('!') => {
                        let shell_cmd = cmd[1..].trim();
                        if shell_cmd.is_empty() {
                            output::print_error("Usage: !<command>");
                            continue;
                        }
                        match std::process::Command::new("sh")
                            .arg("-c")
                            .arg(shell_cmd)
                            .current_dir(workspace)
                            .output()
                        {
                            Ok(out) => {
                                if !out.stdout.is_empty() {
                                    print!("{}", String::from_utf8_lossy(&out.stdout));
                                }
                                if !out.stderr.is_empty() {
                                    eprint!("{}", String::from_utf8_lossy(&out.stderr));
                                }
                                if !out.status.success() {
                                    output::print_error(&format!(
                                        "exit code: {}",
                                        out.status.code().unwrap_or(-1)
                                    ));
                                }
                            }
                            Err(e) => output::print_error(&format!("failed to execute: {}", e)),
                        }
                    }
                    cmd if cmd.starts_with('/') => {
                        output::print_error(&format!("Unknown command: {}", cmd));
                        eprintln!("Type /help for available commands.");
                    }
                    _ => {
                        // Regular prompt
                        event_handler::run_prompt(agent, trimmed, approval_rx).await;

                        // Auto-save after each prompt
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);
                    }
                }
            }
            Err(e) => {
                output::print_error(&format!("stdin error: {}", e));
                break;
            }
        }
    }

    // Final save
    let messages = agent.messages().await;
    store.save_messages(&session_id, &messages);
    eprintln!("Session saved: {}", session_id);
}
