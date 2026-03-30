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

mod checkpoint;
mod output;
mod session_store;

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AgentEvent, AgentMessage, BaseAgent, BaseAgentBuilder, PermissionDecision, PermissionMode};
use alva_agent_runtime::middleware::checkpoint::CheckpointCallback;
use alva_agent_runtime::middleware::security::ApprovalRequest;
use alva_provider::{OpenAIProvider, ProviderConfig};
use tokio::sync::mpsc;

use session_store::SessionStore;

/// CLI checkpoint callback — bridges CheckpointMiddleware to CheckpointManager.
struct CliCheckpointCallback {
    manager: checkpoint::CheckpointManager,
}

impl CheckpointCallback for CliCheckpointCallback {
    fn create_checkpoint(&self, description: &str, file_paths: &[std::path::PathBuf]) {
        match self.manager.create(description, file_paths) {
            Ok(id) => {
                tracing::debug!(id = %id, "auto-checkpoint created");
            }
            Err(e) => {
                tracing::warn!(error = %e, "auto-checkpoint failed");
            }
        }
    }
}

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

/// Load project context from well-known files (AGENTS.md, CLAUDE.md, .alva/context.md).
fn load_project_context(workspace: &std::path::Path) -> String {
    let mut context = String::new();
    for name in &["AGENTS.md", "CLAUDE.md", ".alva/context.md"] {
        let path = workspace.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                context.push_str(&format!(
                    "\n\n# Project Context (from {})\n\n{}",
                    name, content
                ));
            }
        }
    }
    context
}

async fn run() {
    // 1. Config
    let config = match ProviderConfig::from_file_with_env("alva.json") {
        Ok(c) => c,
        Err(e) => {
            output::print_error(&format!("Config error: {}", e));
            eprintln!();
            eprintln!("Quick start:");
            eprintln!("  export ALVA_API_KEY=sk-...");
            eprintln!("  export ALVA_MODEL=gpt-4o");
            eprintln!("  cargo run -p alva-app-core --bin alva-cli");
            std::process::exit(1);
        }
    };

    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let store = SessionStore::for_workspace(&workspace);

    // 2. Build system prompt with project context
    let project_context = load_project_context(&workspace);
    let system_prompt = format!(
        "You are a helpful coding assistant. You have access to tools for \
         running shell commands, reading/writing files, and searching code. \
         Use tools when needed to accomplish the user's task. \
         Be concise in your responses.{}",
        project_context
    );

    // 3. Provider + Agent (with approval channel)
    let model = Arc::new(OpenAIProvider::new(config.clone()));
    let mut builder = BaseAgentBuilder::new()
        .workspace(&workspace)
        .system_prompt(&system_prompt)
        .without_browser()
        .with_sub_agents()
        .sub_agent_max_depth(3);
    let mut approval_rx = builder.with_approval_channel();
    let agent = builder.build(model).await.expect("failed to build agent");

    // 3b. Register checkpoint callback
    let checkpoint_mgr_for_rewind = checkpoint::CheckpointManager::new(&workspace);
    agent
        .set_checkpoint_callback(Arc::new(CliCheckpointCallback {
            manager: checkpoint::CheckpointManager::new(&workspace),
        }))
        .await;

    // 4. Print banner
    output::print_banner(&config.model, &workspace.display().to_string());
    output::print_git_status(&workspace);
    output::print_banner_end();

    // 5. Check for single-shot mode
    if let Some(prompt) = std::env::args().nth(1) {
        let session_id = store.create(&prompt);
        run_prompt(&agent, &prompt, &mut approval_rx).await;
        let messages = agent.messages().await;
        store.save_messages(&session_id, &messages);
        return;
    }

    // 6. Interactive REPL — auto-resume latest or start new
    let mut session_id = match store.latest() {
        Some(id) => {
            let sessions = store.list();
            let meta = sessions.iter().find(|m| m.id == id).unwrap();
            output::print_session_resumed(&id, meta.message_count, &meta.summary);

            // Restore messages — clear first to avoid stale data
            agent.new_session().await;
            let saved = store.load_messages(&id);
            if !saved.is_empty() {
                restore_messages(&agent, saved).await;
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
                        output::print_git_status(&workspace);
                        output::print_banner_end();
                    }
                    "/config" => {
                        eprintln!("  Model:     {}", config.model);
                        eprintln!("  Base URL:  {}", config.base_url);
                        eprintln!("  Workspace: {}", workspace.display());
                        eprintln!("  Session:   {}", session_id);
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
                    "/rewind" => {
                        let checkpoints = checkpoint_mgr_for_rewind.list();
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
                                        match checkpoint_mgr_for_rewind.rewind(&cp.id) {
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
                                restore_messages(&agent, saved).await;

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
                            .current_dir(&workspace)
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
                        run_prompt(&agent, trimmed, &mut approval_rx).await;

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

/// Restore saved messages into the agent's state.
async fn restore_messages(agent: &BaseAgent, messages: Vec<AgentMessage>) {
    if !messages.is_empty() {
        agent.restore_messages(messages).await;
    }
}

/// Run a single prompt, handling both agent events and approval requests concurrently.
async fn run_prompt(
    agent: &BaseAgent,
    prompt: &str,
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) {
    let mut event_rx = agent.prompt_text(prompt);

    loop {
        tokio::select! {
            event = event_rx.recv() => {
                match event {
                    Some(AgentEvent::MessageStart { .. }) => {}
                    Some(AgentEvent::MessageUpdate { delta, .. }) => {
                        if let alva_types::StreamEvent::TextDelta { text } = &delta {
                            output::print_assistant_text(text);
                        }
                    }
                    Some(AgentEvent::MessageEnd { .. }) => {
                        // Text already streamed via MessageUpdate deltas; just print newline.
                        println!();
                    }
                    Some(AgentEvent::ToolExecutionStart { tool_call }) => {
                        output::print_tool_start(&tool_call.name);
                    }
                    Some(AgentEvent::ToolExecutionEnd { tool_call, result }) => {
                        output::print_tool_end(&tool_call.name, result.is_error, &result.content);
                    }
                    Some(AgentEvent::AgentEnd { error }) => {
                        if let Some(e) = error {
                            output::print_error(&e);
                        }
                        break;
                    }
                    Some(_) => {}
                    None => break,
                }
            }
            approval = approval_rx.recv() => {
                if let Some(req) = approval {
                    handle_approval(agent, req).await;
                }
            }
        }
    }
}

/// Handle a single approval request: prompt the user and resolve the permission.
async fn handle_approval(agent: &BaseAgent, req: ApprovalRequest) {
    output::print_approval_prompt(&req.tool_name, &req.arguments);

    let mut input = String::new();
    let _ = io::stdin().lock().read_line(&mut input);

    let decision = match input.trim().to_lowercase().as_str() {
        "y" | "yes" | "" => PermissionDecision::AllowOnce,
        "a" | "always" => PermissionDecision::AllowAlways,
        "d" | "deny" => PermissionDecision::RejectAlways,
        _ => PermissionDecision::RejectOnce,
    };

    agent
        .resolve_permission(&req.request_id, &req.tool_name, decision)
        .await;
}
