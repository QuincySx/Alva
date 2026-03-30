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
//!   /quit /exit      Exit
//!
//! Sessions are stored under `.alva/sessions/` in the working directory.

mod output;
mod session_store;

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AgentEvent, AgentMessage, BaseAgent, BaseAgentBuilder};
use alva_provider::{OpenAIProvider, ProviderConfig};

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
    // 1. Config
    let config = match ProviderConfig::from_file_with_env("alva.json") {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Config error: {}", e);
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

    eprintln!("Model: {} @ {}", config.model, config.base_url);
    eprintln!("Workspace: {}", workspace.display());

    // 2. Provider + Agent
    let model = Arc::new(OpenAIProvider::new(config));
    let agent = BaseAgentBuilder::new()
        .workspace(&workspace)
        .system_prompt(
            "You are a helpful coding assistant. You have access to tools for \
             running shell commands, reading/writing files, and searching code. \
             Use tools when needed to accomplish the user's task. \
             Be concise in your responses.",
        )
        .without_browser()
        .with_sub_agents()
        .sub_agent_max_depth(3)
        .build(model)
        .await
        .expect("failed to build agent");

    // 3. Check for single-shot mode
    if let Some(prompt) = std::env::args().nth(1) {
        let session_id = store.create(&prompt);
        run_prompt(&agent, &prompt).await;
        let messages = agent.messages().await;
        store.save_messages(&session_id, &messages);
        return;
    }

    // 4. Interactive REPL — auto-resume latest or start new
    let mut session_id = match store.latest() {
        Some(id) => {
            let sessions = store.list();
            let meta = sessions.iter().find(|m| m.id == id).unwrap();
            eprintln!(
                "Resuming session {} ({} messages) — \"{}\"",
                id, meta.message_count, meta.summary
            );
            eprintln!("Type /new for fresh session, /sessions to list all.");

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
            eprintln!("New session: {}", id);
            id
        }
    };

    eprintln!("---");

    loop {
        eprint!("> ");
        io::stderr().flush().ok();

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
                    "/new" => {
                        // Save current, start fresh
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);

                        agent.new_session().await;
                        session_id = store.create("");
                        eprintln!("New session: {}", session_id);
                        eprintln!("---");
                    }
                    "/resume" => {
                        // Save current first
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);

                        let sessions = store.list();
                        if sessions.is_empty() {
                            eprintln!("No sessions found.");
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

                                eprintln!(
                                    "Resumed: {} ({} messages)",
                                    session_id,
                                    agent.messages().await.len()
                                );
                            }
                        }
                        eprintln!("---");
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
                    cmd if cmd.starts_with('/') => {
                        eprintln!("Unknown command: {}", cmd);
                        eprintln!("Available: /new /resume /sessions /quit");
                    }
                    _ => {
                        // Regular prompt
                        run_prompt(&agent, trimmed).await;

                        // Auto-save after each prompt
                        let messages = agent.messages().await;
                        store.save_messages(&session_id, &messages);
                    }
                }
            }
            Err(e) => {
                eprintln!("stdin error: {}", e);
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
    // Use follow_up to inject the saved history into the agent.
    // The agent will see these messages in its context on the next prompt.
    if !messages.is_empty() {
        agent.restore_messages(messages).await;
    }
}

async fn run_prompt(agent: &BaseAgent, prompt: &str) {
    let mut rx = agent.prompt_text(prompt);
    let mut printed_end = false;

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::MessageStart { .. } => {}
            AgentEvent::MessageUpdate { delta, .. } => {
                if let alva_types::StreamEvent::TextDelta { text } = &delta {
                    print!("{}", text);
                    io::stdout().flush().ok();
                }
            }
            AgentEvent::MessageEnd { message } => {
                if let AgentMessage::Standard(msg) = &message {
                    let text = msg.text_content();
                    if !text.is_empty() && !printed_end {
                        println!("{}", text);
                        printed_end = true;
                    }
                }
            }
            AgentEvent::ToolExecutionStart { tool_call } => {
                eprintln!("  [tool] {} ...", tool_call.name);
            }
            AgentEvent::ToolExecutionEnd { tool_call, result } => {
                let status = if result.is_error { "ERROR" } else { "ok" };
                let preview = if result.content.len() > 100 {
                    format!("{}...", &result.content[..100])
                } else {
                    result.content.clone()
                };
                eprintln!(
                    "  [tool] {} → {} ({})",
                    tool_call.name,
                    status,
                    preview.replace('\n', " ")
                );
            }
            AgentEvent::AgentEnd { error } => {
                if let Some(e) = error {
                    eprintln!("Error: {}", e);
                }
                break;
            }
            _ => {}
        }
    }
}
