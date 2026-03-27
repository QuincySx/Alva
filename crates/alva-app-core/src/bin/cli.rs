//! alva-cli — Minimal CLI agent.
//!
//! Usage:
//!   export ALVA_API_KEY=sk-...
//!   export ALVA_MODEL=gpt-4o              # optional, default: gpt-4o
//!   export ALVA_BASE_URL=https://...      # optional, default: OpenAI
//!   cargo run -p alva-app-core --bin alva-cli
//!
//! Or pass prompt as argument:
//!   cargo run -p alva-app-core --bin alva-cli -- "list files in current directory"

use std::io::{self, BufRead, Write};
use std::sync::Arc;

use alva_app_core::{AgentEvent, AgentMessage, BaseAgentBuilder};
use alva_provider::{OpenAIProvider, ProviderConfig};

fn main() {
    // Init tracing (stderr, so stdout stays clean for agent output)
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
    // 1. Load config
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

    eprintln!("Model: {} @ {}", config.model, config.base_url);

    // 2. Create provider
    let model = Arc::new(OpenAIProvider::new(config));

    // 3. Build agent
    let workspace = std::env::current_dir().unwrap_or_else(|_| ".".into());
    let agent = BaseAgentBuilder::new()
        .workspace(&workspace)
        .system_prompt(
            "You are a helpful coding assistant. You have access to tools for \
             running shell commands, reading/writing files, and searching code. \
             Use tools when needed to accomplish the user's task. \
             Be concise in your responses.",
        )
        .without_browser()
        .build(model)
        .await
        .expect("failed to build agent");

    // 4. REPL loop
    let prompt_from_args = std::env::args().nth(1);

    if let Some(prompt) = prompt_from_args {
        // Single-shot mode
        run_prompt(&agent, &prompt).await;
    } else {
        // Interactive REPL
        eprintln!("Ready. Type your prompt (Ctrl+D to exit).");
        eprintln!("---");
        loop {
            eprint!("> ");
            io::stderr().flush().ok();

            let mut line = String::new();
            match io::stdin().lock().read_line(&mut line) {
                Ok(0) => break, // EOF
                Ok(_) => {
                    let trimmed = line.trim();
                    if trimmed.is_empty() {
                        continue;
                    }
                    if trimmed == "/quit" || trimmed == "/exit" {
                        break;
                    }
                    run_prompt(&agent, trimmed).await;
                    eprintln!("---");
                }
                Err(e) => {
                    eprintln!("stdin error: {}", e);
                    break;
                }
            }
        }
    }
}

async fn run_prompt(agent: &alva_app_core::BaseAgent, prompt: &str) {
    let mut rx = agent.prompt_text(prompt);

    while let Some(event) = rx.recv().await {
        match event {
            AgentEvent::MessageStart { .. } => {}
            AgentEvent::MessageUpdate { delta, .. } => {
                // Print streaming text deltas as they arrive
                if let alva_types::StreamEvent::TextDelta { text } = &delta {
                    print!("{}", text);
                    io::stdout().flush().ok();
                }
            }
            AgentEvent::MessageEnd { message } => {
                if let AgentMessage::Standard(msg) = &message {
                    let text = msg.text_content();
                    if !text.is_empty() {
                        println!("{}", text);
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
                eprintln!("  [tool] {} → {} ({})", tool_call.name, status, preview.replace('\n', " "));
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
