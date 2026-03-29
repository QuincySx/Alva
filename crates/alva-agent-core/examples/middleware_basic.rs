// INPUT:  alva_agent_core (V2), alva_types, async_trait
// OUTPUT: (none — example binary)
// POS:    Example demonstrating V2 middleware composition (logging, security, token counting)
//! Example: composing V2 middleware into an agent config.
//!
//! Demonstrates:
//! - LoggingMiddleware  — logs before/after tool calls
//! - SecurityMiddleware — blocks specific tools
//! - TokenCountingMiddleware — uses Extensions for cross-middleware state
//! - Composing them into a V2 MiddlewareStack and attaching to AgentConfig

use std::sync::Arc;

use alva_agent_core::v2::middleware::{Middleware, MiddlewareError, MiddlewareStack};
use alva_agent_core::v2::state::{AgentConfig, AgentState};
use alva_agent_core::middleware::Extensions;
use alva_types::{Message, ToolCall, ToolResult};
use alva_types::session::InMemorySession;
use async_trait::async_trait;

// ---------------------------------------------------------------------------
// LoggingMiddleware — prints before/after tool calls
// ---------------------------------------------------------------------------

struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        println!("[LoggingMiddleware] before_tool_call: {}", tool_call.name);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        println!(
            "[LoggingMiddleware] after_tool_call: {} -> is_error={}",
            tool_call.name, result.is_error
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "LoggingMiddleware"
    }
}

// ---------------------------------------------------------------------------
// SecurityMiddleware — blocks specific tools by name
// ---------------------------------------------------------------------------

struct SecurityMiddleware {
    blocked_tools: Vec<String>,
}

#[async_trait]
impl Middleware for SecurityMiddleware {
    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.blocked_tools.contains(&tool_call.name) {
            println!(
                "[SecurityMiddleware] BLOCKED tool: {}",
                tool_call.name
            );
            return Err(MiddlewareError::Blocked {
                reason: format!("tool '{}' is not allowed", tool_call.name),
            });
        }
        println!(
            "[SecurityMiddleware] ALLOWED tool: {}",
            tool_call.name
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "SecurityMiddleware"
    }
}

// ---------------------------------------------------------------------------
// TokenCountingMiddleware — uses Extensions to share state
// ---------------------------------------------------------------------------

/// Shared state placed in Extensions so other middleware can read it.
#[derive(Debug, Default)]
struct TokenStats {
    llm_calls: u32,
    tool_calls: u32,
}

struct TokenCountingMiddleware;

#[async_trait]
impl Middleware for TokenCountingMiddleware {
    async fn on_agent_start(
        &self,
        state: &mut AgentState,
    ) -> Result<(), MiddlewareError> {
        state.extensions.insert(TokenStats::default());
        println!("[TokenCountingMiddleware] initialized stats in Extensions");
        Ok(())
    }

    async fn before_llm_call(
        &self,
        state: &mut AgentState,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = state.extensions.get_mut::<TokenStats>() {
            stats.llm_calls += 1;
            println!(
                "[TokenCountingMiddleware] LLM call #{}", stats.llm_calls
            );
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = state.extensions.get_mut::<TokenStats>() {
            stats.tool_calls += 1;
            println!(
                "[TokenCountingMiddleware] tool call #{} ({})",
                stats.tool_calls, tool_call.name
            );
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = state.extensions.get::<TokenStats>() {
            println!(
                "[TokenCountingMiddleware] agent ended — LLM calls: {}, tool calls: {}, error: {:?}",
                stats.llm_calls, stats.tool_calls, error
            );
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "TokenCountingMiddleware"
    }
}

// ---------------------------------------------------------------------------
// Stub model for the example
// ---------------------------------------------------------------------------

struct StubModel;

#[async_trait]
impl alva_types::LanguageModel for StubModel {
    async fn complete(
        &self,
        _: &[Message],
        _: &[&dyn alva_types::Tool],
        _: &alva_types::ModelConfig,
    ) -> Result<Message, alva_types::AgentError> {
        Ok(Message {
            id: "stub".to_string(),
            role: alva_types::MessageRole::Assistant,
            content: vec![alva_types::ContentBlock::Text {
                text: "stub response".to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        })
    }
    fn stream(
        &self,
        _: &[Message],
        _: &[&dyn alva_types::Tool],
        _: &alva_types::ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = alva_types::StreamEvent> + Send>> {
        Box::pin(tokio_stream::empty())
    }
    fn model_id(&self) -> &str {
        "stub"
    }
}

// ---------------------------------------------------------------------------
// main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== Middleware Basic Example (V2) ===\n");

    // 1. Build the middleware stack (order matters — onion model).
    let mut stack = MiddlewareStack::new();
    stack.push(Arc::new(LoggingMiddleware));
    stack.push(Arc::new(SecurityMiddleware {
        blocked_tools: vec!["dangerous_tool".to_string()],
    }));
    stack.push(Arc::new(TokenCountingMiddleware));

    println!(
        "MiddlewareStack created with {} layer(s):\n  - LoggingMiddleware\n  - SecurityMiddleware\n  - TokenCountingMiddleware\n",
        stack.len()
    );

    // 2. Create V2 AgentState + AgentConfig
    let session: Arc<dyn alva_types::session::AgentSession> =
        Arc::new(InMemorySession::new());
    let mut state = AgentState {
        model: Arc::new(StubModel),
        tools: vec![],
        session,
        extensions: Extensions::new(),
    };

    let config = AgentConfig {
        middleware: stack,
        system_prompt: "You are a demo assistant.".to_string(),
    };

    println!(
        "AgentConfig.middleware has {} layer(s)",
        config.middleware.len()
    );
    println!("AgentConfig.middleware.is_empty() = {}", config.middleware.is_empty());

    // 3. Demonstrate a quick round-trip through the stack.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        println!("\n--- Running on_agent_start ---");
        let _ = config.middleware.run_on_agent_start(&mut state).await;

        println!("\n--- Running before_llm_call ---");
        let mut msgs = vec![Message::user("Hello!")];
        let _ = config.middleware.run_before_llm_call(&mut state, &mut msgs).await;

        println!("\n--- Running after_llm_call ---");
        let mut response = Message {
            id: "msg-1".to_string(),
            role: alva_types::MessageRole::Assistant,
            content: vec![alva_types::ContentBlock::Text {
                text: "Hi there!".to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let _ = config.middleware.run_after_llm_call(&mut state, &mut response).await;

        println!("\n--- Running before_tool_call (allowed tool) ---");
        let allowed_call = ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let result: Result<(), MiddlewareError> = config
            .middleware
            .run_before_tool_call(&mut state, &allowed_call)
            .await;
        println!("  result: {:?}", result.map(|_| "ok"));

        println!("\n--- Running before_tool_call (blocked tool) ---");
        let blocked_call = ToolCall {
            id: "call-2".to_string(),
            name: "dangerous_tool".to_string(),
            arguments: serde_json::json!({}),
        };
        let result: Result<(), MiddlewareError> = config
            .middleware
            .run_before_tool_call(&mut state, &blocked_call)
            .await;
        println!("  result: {:?}", result.map(|_| "ok"));

        println!("\n--- Running on_agent_end ---");
        let _ = config.middleware.run_on_agent_end(&mut state, None).await;
    });

    println!("\n=== Done ===");
}
