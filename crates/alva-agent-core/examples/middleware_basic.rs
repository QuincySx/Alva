// INPUT:  alva_agent_core, alva_types, async_trait
// OUTPUT: (none — example binary)
// POS:    Example demonstrating middleware composition (logging, security, token counting) into an AgentHooks config
//! Example: composing middleware into an agent config.
//!
//! Demonstrates:
//! - LoggingMiddleware  — logs before/after tool calls
//! - SecurityMiddleware — blocks specific tools
//! - TokenCountingMiddleware — uses Extensions for cross-middleware state
//! - Composing them into a MiddlewareStack and attaching to AgentHooks

use std::sync::Arc;

use alva_agent_core::{
    AgentHooks, AgentMessage, Extensions, Middleware, MiddlewareContext, MiddlewareError,
    MiddlewareStack,
};
use alva_types::{Message, ToolCall, ToolContext, ToolResult};
use async_trait::async_trait;

// ---------------------------------------------------------------------------
// LoggingMiddleware — prints before/after tool calls
// ---------------------------------------------------------------------------

struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        _tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        println!("[LoggingMiddleware] before_tool_call: {}", tool_call.name);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
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
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        _tool_context: &dyn ToolContext,
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
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        ctx.extensions.insert(TokenStats::default());
        println!("[TokenCountingMiddleware] initialized stats in Extensions");
        Ok(())
    }

    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = ctx.extensions.get_mut::<TokenStats>() {
            stats.llm_calls += 1;
            println!(
                "[TokenCountingMiddleware] LLM call #{}", stats.llm_calls
            );
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        _tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = ctx.extensions.get_mut::<TokenStats>() {
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
        ctx: &mut MiddlewareContext,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if let Some(stats) = ctx.extensions.get::<TokenStats>() {
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
// main
// ---------------------------------------------------------------------------

fn main() {
    println!("=== Middleware Basic Example ===\n");

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

    // 2. Attach to AgentHooks.
    let convert_to_llm = Arc::new(|ctx: &alva_agent_core::AgentContext<'_>| -> Vec<Message> {
        let mut result = vec![Message::system(ctx.system_prompt)];
        for m in ctx.messages {
            if let AgentMessage::Standard(msg) = m {
                result.push(msg.clone());
            }
        }
        result
    });

    let mut config = AgentHooks::new(convert_to_llm);
    config.middleware = stack;

    println!(
        "AgentHooks.middleware has {} layer(s)",
        config.middleware.len()
    );
    println!("AgentHooks.middleware.is_empty() = {}", config.middleware.is_empty());

    // 3. Demonstrate a quick round-trip through the stack.
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();

    rt.block_on(async {
        println!("\n--- Running on_agent_start ---");
        let mut ctx = MiddlewareContext {
            session_id: "demo-session".to_string(),
            system_prompt: "You are a demo assistant.".to_string(),
            messages: Vec::new(),
            extensions: Extensions::new(),
        };
        let _ = config.middleware.run_on_agent_start(&mut ctx).await;

        println!("\n--- Running before_llm_call ---");
        let mut msgs = vec![Message::user("Hello!")];
        let _ = config.middleware.run_before_llm_call(&mut ctx, &mut msgs).await;

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
        let _ = config.middleware.run_after_llm_call(&mut ctx, &mut response).await;

        println!("\n--- Running before_tool_call (allowed tool) ---");
        let allowed_call = ToolCall {
            id: "call-1".to_string(),
            name: "read_file".to_string(),
            arguments: serde_json::json!({"path": "/tmp/test.txt"}),
        };
        let tool_ctx = alva_types::EmptyToolContext;
        let result = config
            .middleware
            .run_before_tool_call(&mut ctx, &allowed_call, &tool_ctx)
            .await;
        println!("  result: {:?}", result.map(|_| "ok"));

        println!("\n--- Running before_tool_call (blocked tool) ---");
        let blocked_call = ToolCall {
            id: "call-2".to_string(),
            name: "dangerous_tool".to_string(),
            arguments: serde_json::json!({}),
        };
        let result = config
            .middleware
            .run_before_tool_call(&mut ctx, &blocked_call, &tool_ctx)
            .await;
        println!("  result: {:?}", result.map(|_| "ok"));

        println!("\n--- Running on_agent_end ---");
        let _ = config.middleware.run_on_agent_end(&mut ctx, None).await;
    });

    println!("\n=== Done ===");
}
