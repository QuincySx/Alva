// INPUT:  alva_agent_runtime, alva_agent_core, alva_types, async_trait, futures_core
// OUTPUT: (none — example binary)
// POS:    Example demonstrating agent runtime builder API with a stub LLM provider
//! Example: building an agent runtime with the builder API.
//!
//! Demonstrates:
//! - Setting up a `ProviderRegistry` (with a stub provider)
//! - Resolving a model via `alva_agent_runtime::model("provider/model_id", &registry)`
//! - Using the builder API to compose tools and middleware
//! - Running the agent loop and consuming events
//!
//! This example uses a mock model so it runs without any real API keys.

use std::pin::Pin;
use std::sync::Arc;

use alva_agent_runtime::AgentRuntime;
use alva_agent_core::middleware::{Middleware, MiddlewareError};
use alva_agent_core::state::AgentState;
use alva_agent_core::run::run_agent;
use alva_types::{
    AgentError, CancellationToken, CompletionResponse, LanguageModel, Message, ModelConfig,
    Provider, ProviderError, ProviderRegistry, StreamEvent, Tool, AgentMessage,
};
use async_trait::async_trait;
use futures_core::Stream;

// ---------------------------------------------------------------------------
// Stub LLM — always returns a fixed response
// ---------------------------------------------------------------------------

struct StubModel;

#[async_trait]
impl LanguageModel for StubModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message {
            id: "stub-msg-1".to_string(),
            role: alva_types::MessageRole::Assistant,
            content: vec![alva_types::ContentBlock::Text {
                text: "Hello from StubModel! I'm a mock LLM.".to_string(),
            }],
            tool_call_id: None,
            usage: Some(alva_types::UsageMetadata {
                input_tokens: 10,
                output_tokens: 8,
                total_tokens: 18,
            }),
            timestamp: 0,
        }))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        Box::pin(tokio_stream::pending::<StreamEvent>())
    }

    fn model_id(&self) -> &str {
        "stub-model"
    }
}

// ---------------------------------------------------------------------------
// Stub Provider — produces StubModel instances
// ---------------------------------------------------------------------------

struct StubProvider;

impl Provider for StubProvider {
    fn id(&self) -> &str {
        "stub"
    }
    fn language_model(
        &self,
        _model_id: &str,
    ) -> Result<Arc<dyn LanguageModel>, ProviderError> {
        Ok(Arc::new(StubModel))
    }
}

// ---------------------------------------------------------------------------
// Example logging middleware
// ---------------------------------------------------------------------------

struct PrintMiddleware;

#[async_trait]
impl Middleware for PrintMiddleware {
    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        println!("[PrintMiddleware] before_llm_call — {} message(s)", messages.len());
        Ok(())
    }

    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        println!(
            "[PrintMiddleware] after_llm_call — response: {}",
            response.text_content()
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "PrintMiddleware"
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== alva-agent-runtime Basic Example ===\n");

    // 1. Set up a provider registry with the stub provider.
    let mut registry = ProviderRegistry::new();
    registry.register(Arc::new(StubProvider));

    // 2. Resolve a model via the unified init helper.
    let llm = alva_agent_runtime::model("stub/any-model-id", &registry)
        .expect("failed to resolve model");
    println!("Resolved model: {}\n", llm.model_id());

    // 3. Build the runtime using the builder API.
    let runtime = AgentRuntime::builder()
        .system_prompt("You are a helpful assistant.")
        .with_builtin_tools()
        .middleware(Arc::new(PrintMiddleware))
        .build(llm);

    println!(
        "Runtime created. Tool registry has {} tool(s):",
        runtime.tool_registry.list().len()
    );
    for tool in runtime.tool_registry.list() {
        println!("  - {}", tool.name());
    }

    // 4. Run agent loop and consume events.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        println!("\n--- Running agent ---");
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut state = runtime.state;
        let config = runtime.config;
        let user_msg = AgentMessage::Standard(Message::user("Hello, agent!"));

        // Run the agent loop
        let run_result = run_agent(
            &mut state,
            &config,
            cancel,
            vec![user_msg],
            event_tx,
        )
        .await;

        // Drain events
        while let Ok(event) = event_rx.try_recv() {
            match &event {
                alva_agent_core::AgentEvent::AgentStart => {
                    println!("[event] AgentStart");
                }
                alva_agent_core::AgentEvent::TurnStart => {
                    println!("[event] TurnStart");
                }
                alva_agent_core::AgentEvent::MessageEnd { message } => {
                    if let AgentMessage::Standard(msg) = message {
                        println!("[event] MessageEnd: {}", msg.text_content());
                    }
                }
                alva_agent_core::AgentEvent::AgentEnd { error } => {
                    println!("[event] AgentEnd (error: {:?})", error);
                }
                _ => {
                    println!("[event] {:?}", event);
                }
            }
        }

        if let Err(e) = run_result {
            println!("Agent run error: {}", e);
        }
    });

    println!("\n=== Done ===");
}
