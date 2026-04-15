// INPUT:  alva_app_core, alva_llm_provider, alva_host_native, checkpoint, session, output, event_handler, commands
// OUTPUT: run_repl
// POS:    Interactive REPL loop — session management, slash commands, and user input dispatch

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, PermissionMode};
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::agent_session::EventQuery;
use alva_kernel_abi::AgentSession;
use alva_llm_provider::{OpenAIChatProvider, ProviderConfig};
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::commands::{CommandContext, CommandRegistry, CommandResult, TokenUsage};
use crate::event_handler;
use crate::output;
use crate::session::{JsonFileAgentSession, JsonFileSessionManager, SessionSummary};

// Session-level token accumulation uses TokenUsage directly.

/// Build a CommandContext from current REPL state.
fn build_command_context<'a>(
    workspace: &'a std::path::Path,
    config: &'a ProviderConfig,
    session_id: &'a str,
    message_count: usize,
    tokens: &TokenUsage,
    tool_names: Vec<String>,
    plan_mode: bool,
    home_dir: &std::path::Path,
) -> CommandContext<'a> {
    CommandContext {
        workspace,
        home_dir: home_dir.to_path_buf(),
        model: &config.model,
        session_id,
        message_count,
        token_usage: tokens.clone(),
        tool_names,
        plan_mode,
        extra: std::collections::HashMap::new(),
    }
}

/// Run the interactive REPL loop with session management.
pub(crate) async fn run_repl(
    agent: &BaseAgent,
    config: &ProviderConfig,
    workspace: &std::path::Path,
    paths: &AlvaPaths,
    session_manager: &JsonFileSessionManager,
    checkpoint_mgr: &checkpoint::CheckpointManager,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) {
    let registry = CommandRegistry::new();
    let mut tokens = TokenUsage::default();
    let home_dir = dirs::home_dir().unwrap_or_default();

    // Auto-resume latest or start new
    let (mut session_id, mut active_session) = match session_manager.latest() {
        Some(id) => match session_manager.load(&id).await {
            Some(sess) => {
                let sessions = session_manager.list();
                let meta = sessions.iter().find(|m| m.session_id == id);
                let (msg_count, summary) = meta
                    .map(|m| (m.event_count, m.preview.as_str()))
                    .unwrap_or((0, ""));
                output::print_session_resumed(&id, msg_count, summary);
                agent.swap_session(sess.clone()).await;
                (id, sess)
            }
            None => {
                let sess = session_manager.create("").await;
                let id = sess.session_id().to_string();
                agent.swap_session(sess.clone()).await;
                output::print_session_new(&id);
                (id, sess)
            }
        },
        None => {
            let sess = session_manager.create("").await;
            let id = sess.session_id().to_string();
            agent.swap_session(sess.clone()).await;
            output::print_session_new(&id);
            (id, sess)
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

                // === Commands handled directly by REPL (need mutable agent/state access) ===
                match trimmed {
                    "/quit" | "/exit" => break,
                    "/clear" => {
                        print!("\x1B[2J\x1B[1;1H");
                        io::stdout().flush().ok();
                        output::print_banner(
                            &config.model,
                            &workspace.display().to_string(),
                        );
                        output::print_git_status(workspace);
                        output::print_banner_end();
                        continue;
                    }
                    "/setup" => {
                        eprintln!("  Reconfiguring requires restart.");
                        if let Some(new_config) = crate::setup::run_setup_wizard(workspace) {
                            eprintln!();
                            eprintln!("  Configuration saved. Please restart alva-cli to use the new settings.");
                            let _ = new_config;
                        }
                        continue;
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
                        continue;
                    }
                    "/model" => {
                        let current = agent.model_id().await;
                        eprintln!("  Current model: {}", current);
                        eprintln!("  Usage: /model <model_id>");
                        continue;
                    }
                    "/rewind" => {
                        handle_rewind(checkpoint_mgr);
                        continue;
                    }
                    "/new" => {
                        let _ = active_session.flush().await;
                        let new_session = session_manager.create("").await;
                        session_id = new_session.session_id().to_string();
                        agent.swap_session(new_session.clone()).await;
                        active_session = new_session;
                        tokens = TokenUsage::default();
                        output::print_session_new(&session_id);
                        output::print_divider();
                        continue;
                    }
                    "/fork" => {
                        let _ = active_session.flush().await;
                        let old_id = session_id.clone();
                        // Load current messages before swapping
                        let messages = agent.messages().await;
                        let new_session = session_manager.create("").await;
                        session_id = new_session.session_id().to_string();
                        // Copy messages into new session
                        for msg in &messages {
                            new_session.append_message(msg.clone(), None).await;
                        }
                        agent.swap_session(new_session.clone()).await;
                        active_session = new_session;
                        eprintln!(
                            "  Forked from {} → {}",
                            &old_id[..8.min(old_id.len())],
                            &session_id[..8.min(session_id.len())]
                        );
                        eprintln!(
                            "  {} messages carried over. Try a different approach.",
                            messages.len()
                        );
                        output::print_divider();
                        continue;
                    }
                    "/resume" => {
                        if let Some((new_id, new_session)) =
                            handle_resume(agent, session_manager, &active_session, &session_id).await
                        {
                            session_id = new_id;
                            active_session = new_session;
                        }
                        output::print_divider();
                        continue;
                    }
                    "/sessions" => {
                        handle_sessions(session_manager, &session_id);
                        continue;
                    }
                    _ => {} // Fall through to registry or model/shell handling
                }

                // === /model <id> ===
                if let Some(model_id) = trimmed.strip_prefix("/model ") {
                    let model_id = model_id.trim();
                    if !model_id.is_empty() {
                        let mut new_config = config.clone();
                        new_config.model = model_id.to_string();
                        let new_model = Arc::new(OpenAIChatProvider::new(new_config));
                        agent.set_model(new_model).await;
                        eprintln!("  Switched to model: {}", model_id);
                    }
                    continue;
                }

                // === !shell ===
                if trimmed.starts_with('!') {
                    handle_shell(trimmed, workspace);
                    continue;
                }

                // === Slash commands via registry ===
                if trimmed.starts_with('/') {
                    let message_count = agent.messages().await.len();
                    let tool_names = agent.tool_names();
                    let plan_mode = agent.permission_mode() == PermissionMode::Plan;
                    let ctx = build_command_context(
                        workspace,
                        config,
                        &session_id,
                        message_count,
                        &tokens,
                        tool_names,
                        plan_mode,
                        &home_dir,
                    );

                    if let Some(result) = registry.execute(trimmed, &ctx) {
                        match result {
                            CommandResult::Text(text) => {
                                eprintln!("{}", text);
                            }
                            CommandResult::Prompt {
                                content,
                                progress_message,
                                ..
                            } => {
                                if let Some(msg) = &progress_message {
                                    eprintln!("  {}", msg);
                                }
                                let (in_tok, out_tok) = event_handler::run_prompt(
                                    agent,
                                    &content,
                                    approval_rx,
                                )
                                .await;
                                tokens.input_tokens += in_tok;
                                tokens.output_tokens += out_tok;

                                // Persistence is automatic; refresh index summary.
                                let event_count = active_session.count(&EventQuery::default()).await;
                                session_manager.refresh_summary(&session_id, event_count, None);
                            }
                            CommandResult::Compact { summary } => {
                                eprintln!("  {}", summary);
                                // Trigger compaction via prompt
                                let (in_tok, out_tok) = event_handler::run_prompt(
                                    agent,
                                    &summary,
                                    approval_rx,
                                )
                                .await;
                                tokens.input_tokens += in_tok;
                                tokens.output_tokens += out_tok;

                                // Persistence is automatic; refresh index summary.
                                let event_count = active_session.count(&EventQuery::default()).await;
                                session_manager.refresh_summary(&session_id, event_count, None);
                            }
                            CommandResult::Error(e) => {
                                output::print_error(&e);
                            }
                            CommandResult::Skip => {}
                        }
                    }
                    continue;
                }

                // === Regular prompt ===
                let (in_tok, out_tok) =
                    event_handler::run_prompt(agent, trimmed, approval_rx).await;
                tokens.input_tokens += in_tok;
                tokens.output_tokens += out_tok;

                // Persistence is automatic; refresh index summary.
                let event_count = active_session.count(&EventQuery::default()).await;
                session_manager.refresh_summary(&session_id, event_count, None);
            }
            Err(e) => {
                output::print_error(&format!("stdin error: {}", e));
                break;
            }
        }
    }

    // Final flush
    let _ = active_session.flush().await;
    eprintln!("Session saved: {}", session_id);
}

// === Extracted handlers for complex commands that need mutable state ===

fn handle_rewind(checkpoint_mgr: &checkpoint::CheckpointManager) {
    let checkpoints = checkpoint_mgr.list();
    if checkpoints.is_empty() {
        eprintln!("  No checkpoints available.");
        return;
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
                            output::print_error(&format!("rewind failed: {}", e));
                        }
                    }
                }
            }
        }
    }
}

async fn handle_resume(
    agent: &BaseAgent,
    session_manager: &JsonFileSessionManager,
    active_session: &Arc<JsonFileAgentSession>,
    current_session_id: &str,
) -> Option<(String, Arc<JsonFileAgentSession>)> {
    // Flush current session before switching.
    let _ = active_session.flush().await;

    let sessions = session_manager.list();
    if sessions.is_empty() {
        output::print_error("No sessions found.");
        return None;
    }

    eprintln!("Sessions:");
    for (i, s) in sessions.iter().enumerate().take(10) {
        let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
            .map(|d| d.format("%m-%d %H:%M").to_string())
            .unwrap_or_default();
        let marker = if s.session_id == current_session_id {
            " ◀"
        } else {
            ""
        };
        eprintln!(
            "  [{}] {} | {} events | {}{}",
            i + 1,
            date,
            s.event_count,
            s.preview,
            marker,
        );
    }
    eprint!("Pick [1-{}] or Enter for latest: ", sessions.len().min(10));
    io::stderr().flush().ok();

    let mut choice = String::new();
    if io::stdin().lock().read_line(&mut choice).is_ok() {
        let idx: usize = choice.trim().parse().unwrap_or(1);
        if idx >= 1 && idx <= sessions.len().min(10) {
            let picked: &SessionSummary = &sessions[idx - 1];
            let new_id = picked.session_id.clone();

            let new_session = match session_manager.load(&new_id).await {
                Some(s) => s,
                None => {
                    output::print_error(&format!("session {} not found on disk", new_id));
                    return None;
                }
            };
            agent.swap_session(new_session.clone()).await;

            let msg_count = agent.messages().await.len();
            output::print_session_resumed(&new_id, msg_count, &picked.preview);
            return Some((new_id, new_session));
        }
    }
    None
}

fn handle_sessions(session_manager: &JsonFileSessionManager, current_session_id: &str) {
    let sessions = session_manager.list();
    if sessions.is_empty() {
        eprintln!("No sessions.");
    } else {
        for s in sessions.iter().take(20) {
            let date = chrono::DateTime::from_timestamp_millis(s.updated_at)
                .map(|d| d.format("%m-%d %H:%M").to_string())
                .unwrap_or_default();
            let marker = if s.session_id == current_session_id {
                " ◀"
            } else {
                ""
            };
            eprintln!(
                "  {} | {} events | {}{}",
                date, s.event_count, s.preview, marker,
            );
        }
    }
}

fn handle_shell(cmd: &str, workspace: &std::path::Path) {
    let shell_cmd = cmd[1..].trim();
    if shell_cmd.is_empty() {
        output::print_error("Usage: !<command>");
        return;
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
