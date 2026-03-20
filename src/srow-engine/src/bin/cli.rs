//! srow-cli — Simple CLI to test the Agent engine
//!
//! Usage:
//!   OPENAI_API_KEY=sk-xxx cargo run -p srow-engine --bin srow-cli
//!
//! Or with a custom base URL (for DeepSeek/Qwen):
//!   OPENAI_API_KEY=sk-xxx OPENAI_BASE_URL=https://api.deepseek.com/v1 OPENAI_MODEL=deepseek-chat cargo run -p srow-engine --bin srow-cli

use srow_engine::{
    adapters::{
        llm::openai_compat::OpenAICompatProvider,
        storage::memory::MemoryStorage,
        tools,
    },
    application::engine::{AgentEngine, EngineEvent},
    application::session_service::SessionService,
    domain::agent::{AgentConfig, LLMConfig, LLMProviderKind},
    domain::message::LLMMessage,
    ports::tool::ToolRegistry,
};
use std::sync::Arc;
use tokio::sync::{mpsc, watch};

const DEFAULT_SYSTEM_PROMPT: &str = r#"You are Srow, a helpful AI assistant with access to tools.
You can execute shell commands, create/edit files, search code, and list directories.
Always use tools when the task requires interacting with the filesystem or running commands.
Be concise and direct in your responses."#;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    // Read configuration from environment
    let api_key = std::env::var("OPENAI_API_KEY").expect("OPENAI_API_KEY must be set");
    let base_url = std::env::var("OPENAI_BASE_URL").ok();
    let model = std::env::var("OPENAI_MODEL").unwrap_or_else(|_| "gpt-4o".to_string());
    let workspace = std::env::current_dir()?;

    println!("=== Srow Agent CLI ===");
    println!("Model: {}", model);
    if let Some(ref url) = base_url {
        println!("Base URL: {}", url);
    }
    println!("Workspace: {}", workspace.display());
    println!("Type 'exit' or 'quit' to stop.\n");

    // Create LLM provider
    let llm: Arc<dyn srow_engine::LLMProvider> = if let Some(ref url) = base_url {
        Arc::new(OpenAICompatProvider::with_base_url(&api_key, url, &model))
    } else {
        Arc::new(OpenAICompatProvider::new(&api_key, &model))
    };

    // Create tool registry with builtin tools
    let mut registry = ToolRegistry::new();
    tools::register_builtin_tools(&mut registry);
    let tools = Arc::new(registry);

    // Create in-memory storage
    let storage: Arc<dyn srow_engine::SessionStorage> = Arc::new(MemoryStorage::new());

    // Create session service and session
    let session_svc = SessionService::new(storage.clone());
    let config = AgentConfig {
        name: "srow-cli".to_string(),
        system_prompt: DEFAULT_SYSTEM_PROMPT.to_string(),
        llm: LLMConfig {
            provider: LLMProviderKind::OpenAI,
            model: model.clone(),
            api_key: api_key.clone(),
            base_url: base_url.clone(),
            max_tokens: 8192,
            temperature: None,
        },
        workspace: workspace.clone(),
        max_iterations: 25,
        compaction_threshold: 0,
        ..Default::default()
    };

    let session = session_svc.create(&config).await?;
    println!("Session: {}\n", session.id);

    // Event channel
    let (event_tx, mut event_rx) = mpsc::channel::<EngineEvent>(256);
    let (_cancel_tx, cancel_rx) = watch::channel(false);

    // Spawn event printer
    tokio::spawn(async move {
        while let Some(event) = event_rx.recv().await {
            match event {
                EngineEvent::TextDelta { text, .. } => {
                    print!("{}", text);
                    use std::io::Write;
                    std::io::stdout().flush().ok();
                }
                EngineEvent::ToolCallStarted { tool_name, .. } => {
                    eprintln!("\n[tool] calling: {}", tool_name);
                }
                EngineEvent::ToolCallCompleted {
                    output,
                    is_error,
                    ..
                } => {
                    if is_error {
                        eprintln!("[tool] error: {}", truncate(&output, 200));
                    } else {
                        eprintln!("[tool] done ({})", truncate(&output, 100));
                    }
                }
                EngineEvent::Completed { .. } => {
                    println!("\n");
                }
                EngineEvent::Error { error, .. } => {
                    eprintln!("\n[error] {}", error);
                }
                EngineEvent::TokenUsage {
                    input, output, total, ..
                } => {
                    eprintln!("[tokens] in={} out={} total={}", input, output, total);
                }
                EngineEvent::WaitingForHuman { .. } => {}
            }
        }
    });

    // REPL loop
    loop {
        eprint!("you> ");
        use std::io::Write;
        std::io::stderr().flush().ok();

        let result = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            std::io::stdin().read_line(&mut buf).map(|n| (n, buf))
        })
        .await??;

        let (n, line) = result;
        if n == 0 {
            break; // EOF
        }

        let input = line.trim().to_string();
        if input.is_empty() {
            continue;
        }
        if input == "exit" || input == "quit" {
            break;
        }

        // Create engine for this turn
        let mut engine = AgentEngine::new(
            config.clone(),
            llm.clone(),
            tools.clone(),
            storage.clone(),
            event_tx.clone(),
            cancel_rx.clone(),
        );

        let user_msg = LLMMessage::user(&input);
        let sid = session.id.clone();

        if let Err(e) = engine.run(&sid, user_msg).await {
            eprintln!("[engine error] {}", e);
        }
    }

    println!("Goodbye!");
    Ok(())
}

fn truncate(s: &str, max: usize) -> &str {
    if s.len() <= max {
        s
    } else {
        &s[..max]
    }
}
