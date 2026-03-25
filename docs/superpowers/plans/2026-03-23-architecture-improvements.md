# Architecture Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement 7 architecture improvements: ToolContext genericization, async Middleware system, alva-app-core split (alva-tools/alva-security/alva-memory/alva-runtime), unified init API, context compression, CI dependency firewall, and examples.

**Architecture:** Bottom-up approach — start with foundation layer changes (alva-types), then engine layer (alva-core middleware), then structural split (alva-app-core → 4 crates), then convenience APIs and examples on top.

**Tech Stack:** Rust, async-trait, tokio, serde, rusqlite, chromiumoxide

---

## File Structure Overview

### New crates to create:
```
crates/alva-tools/          ← 16 Tool implementations extracted from alva-app-core
crates/alva-security/       ← SecurityGuard + PermissionManager + sandbox
crates/alva-memory/         ← FTS + vector + persistence (SQLite)
crates/alva-runtime/        ← Thin integration/orchestration layer
```

### Modified crates:
```
crates/alva-types/src/tool.rs         ← ToolContext genericization
crates/alva-core/src/middleware.rs    ← NEW: async Middleware trait + chain
crates/alva-core/src/types.rs        ← AgentConfig hooks → middleware
crates/alva-core/src/agent.rs        ← Use middleware chain
crates/alva-core/src/agent_loop.rs   ← Use middleware chain
crates/alva-core/src/tool_executor.rs ← Use middleware chain
crates/alva-app-core/                     ← Slim down to facade + skills + mcp + environment
```

### New files:
```
crates/alva-core/src/middleware.rs              ← Middleware trait + MiddlewareChain
crates/alva-core/examples/middleware_basic.rs   ← Basic middleware example
crates/alva-runtime/examples/runtime_basic.rs   ← Basic runtime example
scripts/ci-check-deps.sh                         ← Dependency firewall script
```

---

## Task 1: Genericize ToolContext in alva-types

**Rationale:** Current `ToolContext` hardcodes `workspace() -> &Path`, binding all tools to local filesystem. Split into base + extension traits for future remote/container support.

**Files:**
- Modify: `crates/alva-types/src/tool.rs:49-75`
- Modify: `crates/alva-types/src/lib.rs` (update re-exports)

- [ ] **Step 1.1: Refactor ToolContext trait into base + extension**

In `crates/alva-types/src/tool.rs`, replace the current `ToolContext` trait with a layered design:

```rust
use std::any::Any;

/// Base runtime context for tools — generic, no filesystem assumptions.
pub trait ToolContext: Send + Sync {
    /// Current session identifier.
    fn session_id(&self) -> &str;

    /// Read a configuration value by key.
    /// Returns `None` if the key is not set.
    fn get_config(&self, key: &str) -> Option<String>;

    /// Downcast support — allows middleware/tools to access extended contexts.
    fn as_any(&self) -> &dyn Any;
}

/// Extension trait for tools that operate on a local filesystem.
pub trait LocalToolContext: ToolContext {
    /// Current workspace / project root path.
    fn workspace(&self) -> &std::path::Path;

    /// Whether the tool is allowed to perform dangerous operations.
    fn allow_dangerous(&self) -> bool;
}

/// Blanket: any LocalToolContext is also a ToolContext, so callers that
/// only need the base trait work seamlessly.
/// (This is automatic via trait inheritance — no blanket impl needed.)
```

Update `EmptyToolContext`:
```rust
pub struct EmptyToolContext;

impl ToolContext for EmptyToolContext {
    fn session_id(&self) -> &str { "" }
    fn get_config(&self, _key: &str) -> Option<String> { None }
    fn as_any(&self) -> &dyn Any { self }
}

impl LocalToolContext for EmptyToolContext {
    fn workspace(&self) -> &std::path::Path { std::path::Path::new(".") }
    fn allow_dangerous(&self) -> bool { false }
}
```

- [ ] **Step 1.2: Update Tool trait execute signature**

Change `Tool::execute` to accept `&dyn ToolContext` (base trait). Tools that need filesystem access downcast to `LocalToolContext`:

```rust
async fn execute(
    &self,
    input: serde_json::Value,
    cancel: &CancellationToken,
    ctx: &dyn ToolContext,
) -> Result<ToolResult, AgentError>;
```

The signature stays the same — the change is that `ToolContext` is now the base trait without `workspace()`.

- [ ] **Step 1.3: Update lib.rs re-exports**

In `crates/alva-types/src/lib.rs`, add:
```rust
pub use tool::{LocalToolContext, EmptyToolContext, Tool, ToolCall, ToolContext, ToolDefinition, ToolRegistry, ToolResult};
```

- [ ] **Step 1.4: Update alva-core to use base ToolContext**

In `crates/alva-core/src/types.rs:66`, `AgentState.tool_context` type stays `Arc<dyn ToolContext>`.

In `crates/alva-core/src/tool_executor.rs`, the `tool_context` parameter stays `&Arc<dyn ToolContext>` — tools that need local access downcast internally.

- [ ] **Step 1.5: Update all 16 tools to downcast**

Each tool that uses `ctx.workspace()` needs to downcast. Helper pattern:

```rust
use alva_types::{LocalToolContext, ToolContext};

fn local_ctx(ctx: &dyn ToolContext) -> Result<&dyn LocalToolContext, AgentError> {
    ctx.as_any()
        .downcast_ref::<SrowToolContext>()
        .map(|c| c as &dyn LocalToolContext)
        .ok_or_else(|| AgentError::ToolError {
            tool_name: "tool_name".into(),
            message: "LocalToolContext required".into(),
        })
}
```

Add a method to the base trait:

```rust
pub trait ToolContext: Send + Sync {
    fn session_id(&self) -> &str;
    fn get_config(&self, key: &str) -> Option<String>;
    fn as_any(&self) -> &dyn Any;

    /// Try to get local filesystem context. Returns None for remote contexts.
    fn local(&self) -> Option<&dyn LocalToolContext> { None }
}
```

And implementors override it:
```rust
impl ToolContext for SrowToolContext {
    fn local(&self) -> Option<&dyn LocalToolContext> { Some(self) }
    // ...
}
```

Tools use: `let local = ctx.local().ok_or_else(|| ...)?;`

- [ ] **Step 1.6: Update SrowToolContext in alva-app-core**

`crates/alva-app-core/src/ports/tool.rs`:
```rust
pub use alva_types::{LocalToolContext, Tool, ToolContext, ToolDefinition, ToolRegistry, ToolResult};

#[derive(Debug, Clone)]
pub struct SrowToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
}

impl alva_types::ToolContext for SrowToolContext {
    fn session_id(&self) -> &str { &self.session_id }
    fn get_config(&self, _key: &str) -> Option<String> { None }
    fn as_any(&self) -> &dyn std::any::Any { self }
    fn local(&self) -> Option<&dyn alva_types::LocalToolContext> { Some(self) }
}

impl alva_types::LocalToolContext for SrowToolContext {
    fn workspace(&self) -> &std::path::Path { &self.workspace }
    fn allow_dangerous(&self) -> bool { self.allow_dangerous }
}
```

- [ ] **Step 1.7: Verify compilation**

Run: `cargo check -p alva-types -p alva-core`
Expected: compiles with no errors.

- [ ] **Step 1.8: Run existing tests**

Run: `cargo test -p alva-types -p alva-core`
Expected: all tests pass.

- [ ] **Step 1.9: Commit**

```bash
git add crates/alva-types/ crates/alva-core/
git commit -m "refactor(alva-types): genericize ToolContext with base + LocalToolContext extension"
```

---

## Task 2: Async Middleware System in alva-core

**Rationale:** Replace sync `Vec<Arc<dyn Fn>>` hooks with async middleware chain. Middleware can short-circuit, share context, and wrap execution (before + after). This is the core architectural upgrade.

**Files:**
- Create: `crates/alva-core/src/middleware.rs`
- Modify: `crates/alva-core/src/types.rs` (keep hooks for backward compat, add middleware)
- Modify: `crates/alva-core/src/agent.rs`
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-core/src/tool_executor.rs`
- Modify: `crates/alva-core/src/lib.rs`
- Create: `crates/alva-core/examples/middleware_basic.rs`
- Modify: `crates/alva-core/Cargo.toml`

- [ ] **Step 2.1: Create middleware.rs with Middleware trait**

Create `crates/alva-core/src/middleware.rs`:

```rust
//! Async middleware chain for the agent execution loop.
//!
//! Middleware intercepts agent operations (tool calls, LLM calls, message handling)
//! and can transform, block, or observe them.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use alva_types::{ToolCall, ToolResult, ToolContext, Message};
use async_trait::async_trait;

use crate::event::AgentEvent;
use crate::types::AgentMessage;

/// Errors that middleware can raise to short-circuit execution.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MiddlewareError {
    #[error("blocked: {reason}")]
    Blocked { reason: String },
    #[error("middleware error: {0}")]
    Other(String),
}

/// Context passed through the middleware chain — middleware can read and mutate this.
pub struct MiddlewareContext {
    /// Current session ID.
    pub session_id: String,
    /// Current system prompt.
    pub system_prompt: String,
    /// Current message history (mutable — middleware can inject/remove messages).
    pub messages: Vec<AgentMessage>,
    /// Arbitrary key-value store for inter-middleware communication.
    pub extensions: Extensions,
}

/// Type-safe key-value store for middleware to share data.
///
/// Uses `TypeId`-keyed map so each middleware can store its own state
/// without coupling to other middleware.
pub struct Extensions {
    map: std::collections::HashMap<std::any::TypeId, Box<dyn std::any::Any + Send + Sync>>,
}

impl Extensions {
    pub fn new() -> Self {
        Self {
            map: std::collections::HashMap::new(),
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.map.insert(std::any::TypeId::of::<T>(), Box::new(val));
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&std::any::TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&std::any::TypeId::of::<T>())
            .and_then(|b| b.downcast_mut())
    }
}

impl Default for Extensions {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Middleware trait
// ---------------------------------------------------------------------------

/// Async middleware for intercepting agent operations.
///
/// Each method has a default no-op implementation, so middleware only needs to
/// override the hooks it cares about.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before each LLM call. Can modify messages or short-circuit.
    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, messages);
        Ok(())
    }

    /// Called after each LLM response. Can inspect/modify the response.
    async fn after_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, response);
        Ok(())
    }

    /// Called before each tool execution. Return Err to block the call.
    async fn before_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, tool_call, tool_context);
        Ok(())
    }

    /// Called after each tool execution. Can modify the result.
    async fn after_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, tool_call, result);
        Ok(())
    }

    /// Called at the start of each agent run.
    async fn on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        let _ = ctx;
        Ok(())
    }

    /// Called at the end of each agent run.
    async fn on_agent_end(
        &self,
        ctx: &mut MiddlewareContext,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, error);
        Ok(())
    }

    /// Human-readable name for logging.
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

// ---------------------------------------------------------------------------
// MiddlewareStack — ordered list of middleware
// ---------------------------------------------------------------------------

/// An ordered stack of middleware. Middleware is executed in insertion order
/// for `before_*` hooks and reverse order for `after_*` hooks (onion model).
pub struct MiddlewareStack {
    layers: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    /// Add a middleware to the end of the stack.
    pub fn push(&mut self, middleware: Arc<dyn Middleware>) {
        self.layers.push(middleware);
    }

    /// Run all `before_llm_call` hooks in order. Short-circuits on first error.
    pub async fn run_before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.before_llm_call(ctx, messages).await?;
        }
        Ok(())
    }

    /// Run all `after_llm_call` hooks in reverse order.
    pub async fn run_after_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            layer.after_llm_call(ctx, response).await?;
        }
        Ok(())
    }

    /// Run all `before_tool_call` hooks in order. Short-circuits on first error.
    pub async fn run_before_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.before_tool_call(ctx, tool_call, tool_context).await?;
        }
        Ok(())
    }

    /// Run all `after_tool_call` hooks in reverse order.
    pub async fn run_after_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            layer.after_tool_call(ctx, tool_call, result).await?;
        }
        Ok(())
    }

    /// Run all `on_agent_start` hooks in order.
    pub async fn run_on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.on_agent_start(ctx).await?;
        }
        Ok(())
    }

    /// Run all `on_agent_end` hooks in reverse order.
    pub async fn run_on_agent_end(
        &self,
        ctx: &mut MiddlewareContext,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            layer.on_agent_end(ctx, error).await?;
        }
        Ok(())
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 2.2: Update AgentConfig to include MiddlewareStack**

In `crates/alva-core/src/types.rs`, add middleware to AgentConfig:

```rust
use crate::middleware::MiddlewareStack;

pub struct AgentConfig {
    // ... existing hook fields stay for backward compatibility ...

    /// Async middleware stack (preferred over sync hooks).
    /// When middleware is present, it takes priority over the equivalent hooks.
    pub middleware: MiddlewareStack,

    // ... tool_execution, max_iterations stay ...
}
```

Update `AgentConfig::new()`:
```rust
pub fn new(convert_to_llm: ConvertToLlmFn) -> Self {
    Self {
        convert_to_llm,
        transform_context: None,
        before_tool_call: Vec::new(),
        after_tool_call: Vec::new(),
        get_steering_messages: Vec::new(),
        get_follow_up_messages: Vec::new(),
        middleware: MiddlewareStack::new(),
        tool_execution: ToolExecutionMode::Parallel,
        max_iterations: 100,
    }
}
```

- [ ] **Step 2.3: Update agent_loop.rs to call middleware**

In `run_agent_loop_inner`, add middleware calls at the appropriate points. The middleware runs alongside existing hooks (not replacing them yet) for backward compatibility:

Before LLM call (after building llm_messages):
```rust
// Middleware: before_llm_call
if !config.middleware.is_empty() {
    let mut mw_ctx = MiddlewareContext {
        session_id: state.tool_context.session_id().to_string(),
        system_prompt: state.system_prompt.clone(),
        messages: state.messages.clone(),
        extensions: Extensions::new(),
    };
    if let Err(e) = config.middleware.run_before_llm_call(&mut mw_ctx, &mut llm_messages).await {
        tracing::warn!(error = %e, "middleware blocked LLM call");
        let _ = event_tx.send(AgentEvent::TurnEnd);
        break 'inner;
    }
}
```

After LLM response:
```rust
// Middleware: after_llm_call
if !config.middleware.is_empty() {
    let mut mw_ctx = MiddlewareContext { /* ... */ };
    let _ = config.middleware.run_after_llm_call(&mut mw_ctx, &mut assistant_message).await;
}
```

- [ ] **Step 2.4: Update tool_executor.rs to call middleware**

In both `execute_parallel` and `execute_sequential`, add middleware calls:

Before tool execution:
```rust
// If middleware is present, run before_tool_call
if !config.middleware.is_empty() {
    let mut mw_ctx = MiddlewareContext { /* ... */ };
    if let Err(e) = config.middleware.run_before_tool_call(&mut mw_ctx, tc, tool_context.as_ref()).await {
        // Middleware blocked this tool call
        let blocked_result = ToolResult {
            content: format!("Blocked by middleware: {}", e),
            is_error: true,
            details: None,
        };
        // ... emit events and continue
    }
}
```

After tool execution:
```rust
if !config.middleware.is_empty() {
    let mut mw_ctx = MiddlewareContext { /* ... */ };
    let _ = config.middleware.run_after_tool_call(&mut mw_ctx, tc, &mut result).await;
}
```

- [ ] **Step 2.5: Update lib.rs exports**

In `crates/alva-core/src/lib.rs`:
```rust
pub mod middleware;

pub use middleware::{Middleware, MiddlewareStack, MiddlewareContext, MiddlewareError, Extensions};
```

Also add `ConvertToLlmFn` to top-level exports (needed by alva-runtime's builder):
```rust
pub use types::{
    AgentMessage, AgentConfig, AgentState, AgentContext, ToolCallDecision,
    ToolExecutionMode, HookFuture, ConvertToLlmFn,
};
```

- [ ] **Step 2.6: Update Cargo.toml**

Add `thiserror` to alva-core dependencies (for MiddlewareError):
```toml
thiserror = "2"
```

- [ ] **Step 2.7: Write middleware tests**

Add tests in `crates/alva-core/src/middleware.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    struct CountingMiddleware {
        before_count: Arc<AtomicU32>,
        after_count: Arc<AtomicU32>,
    }

    #[async_trait]
    impl Middleware for CountingMiddleware {
        async fn before_tool_call(
            &self, _ctx: &mut MiddlewareContext, _tc: &ToolCall, _tool_ctx: &dyn ToolContext,
        ) -> Result<(), MiddlewareError> {
            self.before_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        async fn after_tool_call(
            &self, _ctx: &mut MiddlewareContext, _tc: &ToolCall, _result: &mut ToolResult,
        ) -> Result<(), MiddlewareError> {
            self.after_count.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
        fn name(&self) -> &str { "counting" }
    }

    struct BlockingMiddleware;

    #[async_trait]
    impl Middleware for BlockingMiddleware {
        async fn before_tool_call(
            &self, _ctx: &mut MiddlewareContext, _tc: &ToolCall, _tool_ctx: &dyn ToolContext,
        ) -> Result<(), MiddlewareError> {
            Err(MiddlewareError::Blocked { reason: "test block".into() })
        }
        fn name(&self) -> &str { "blocker" }
    }

    #[tokio::test]
    async fn middleware_stack_executes_in_order() {
        let before = Arc::new(AtomicU32::new(0));
        let after = Arc::new(AtomicU32::new(0));
        let mw = CountingMiddleware {
            before_count: before.clone(),
            after_count: after.clone(),
        };
        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(mw));

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let tc = ToolCall { id: "1".into(), name: "test".into(), arguments: serde_json::json!({}) };
        let tool_ctx = alva_types::EmptyToolContext;

        stack.run_before_tool_call(&mut ctx, &tc, &tool_ctx).await.unwrap();
        assert_eq!(before.load(Ordering::SeqCst), 1);

        let mut result = ToolResult { content: "ok".into(), is_error: false, details: None };
        stack.run_after_tool_call(&mut ctx, &tc, &mut result).await.unwrap();
        assert_eq!(after.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn middleware_short_circuits_on_block() {
        let before = Arc::new(AtomicU32::new(0));
        let after = Arc::new(AtomicU32::new(0));
        let counter = CountingMiddleware {
            before_count: before.clone(),
            after_count: after.clone(),
        };

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(BlockingMiddleware));
        stack.push(Arc::new(counter)); // should never execute

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let tc = ToolCall { id: "1".into(), name: "test".into(), arguments: serde_json::json!({}) };
        let tool_ctx = alva_types::EmptyToolContext;

        let result = stack.run_before_tool_call(&mut ctx, &tc, &tool_ctx).await;
        assert!(result.is_err());
        assert_eq!(before.load(Ordering::SeqCst), 0); // counter never reached
    }

    #[tokio::test]
    async fn extensions_type_safe_store() {
        let mut ext = Extensions::new();
        ext.insert::<String>("hello".to_string());
        ext.insert::<u32>(42u32);

        assert_eq!(ext.get::<String>(), Some(&"hello".to_string()));
        assert_eq!(ext.get::<u32>(), Some(&42u32));
        assert_eq!(ext.get::<bool>(), None);
    }
}
```

- [ ] **Step 2.8: Create middleware example**

Create `crates/alva-core/examples/middleware_basic.rs`:

```rust
//! Basic middleware example — demonstrates logging, security, and token counting middleware.
//!
//! Run: `cargo run --example middleware_basic -p alva-core`

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use alva_core::middleware::{
    Middleware, MiddlewareContext, MiddlewareError, MiddlewareStack,
};
use alva_types::{ToolCall, ToolContext, ToolResult, Message};
use async_trait::async_trait;

// ---------------------------------------------------------------------------
// Example 1: Logging middleware — logs every LLM call and tool execution
// ---------------------------------------------------------------------------

struct LoggingMiddleware;

#[async_trait]
impl Middleware for LoggingMiddleware {
    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        println!("[LOG] LLM call with {} messages", messages.len());
        Ok(())
    }

    async fn after_llm_call(
        &self,
        _ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        println!("[LOG] LLM responded with {} content blocks", response.content.len());
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        _tool_ctx: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        println!("[LOG] Executing tool: {} (id: {})", tool_call.name, tool_call.id);
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        println!(
            "[LOG] Tool {} finished (error: {})",
            tool_call.name, result.is_error
        );
        Ok(())
    }

    fn name(&self) -> &str {
        "logging"
    }
}

// ---------------------------------------------------------------------------
// Example 2: Security middleware — blocks dangerous tools unless allowed
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
        _tool_ctx: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        if self.blocked_tools.contains(&tool_call.name) {
            return Err(MiddlewareError::Blocked {
                reason: format!("tool '{}' is blocked by security policy", tool_call.name),
            });
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "security"
    }
}

// ---------------------------------------------------------------------------
// Example 3: Token counting middleware — tracks total tokens via Extensions
// ---------------------------------------------------------------------------

/// Stored in MiddlewareContext.extensions for cross-middleware access.
struct TokenCounter {
    total_input: AtomicU32,
    total_output: AtomicU32,
}

struct TokenCountingMiddleware;

#[async_trait]
impl Middleware for TokenCountingMiddleware {
    async fn on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        ctx.extensions.insert(TokenCounter {
            total_input: AtomicU32::new(0),
            total_output: AtomicU32::new(0),
        });
        Ok(())
    }

    async fn after_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        if let Some(counter) = ctx.extensions.get::<TokenCounter>() {
            if let Some(usage) = &response.usage {
                counter.total_input.fetch_add(usage.input_tokens, Ordering::Relaxed);
                counter.total_output.fetch_add(usage.output_tokens, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        ctx: &mut MiddlewareContext,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if let Some(counter) = ctx.extensions.get::<TokenCounter>() {
            println!(
                "[TOKENS] Total — input: {}, output: {}",
                counter.total_input.load(Ordering::Relaxed),
                counter.total_output.load(Ordering::Relaxed),
            );
        }
        Ok(())
    }

    fn name(&self) -> &str {
        "token_counter"
    }
}

// ---------------------------------------------------------------------------
// main — compose middleware into a stack
// ---------------------------------------------------------------------------

fn main() {
    // Build the middleware stack (order matters!)
    let mut stack = MiddlewareStack::new();

    // 1. Logging first — sees everything
    stack.push(Arc::new(LoggingMiddleware));

    // 2. Security second — blocks before reaching further middleware
    stack.push(Arc::new(SecurityMiddleware {
        blocked_tools: vec!["dangerous_tool".to_string()],
    }));

    // 3. Token counting last — observes final results
    stack.push(Arc::new(TokenCountingMiddleware));

    println!("Middleware stack created with {} layers:", stack.len());
    println!("  1. LoggingMiddleware — observes all operations");
    println!("  2. SecurityMiddleware — blocks dangerous tools");
    println!("  3. TokenCountingMiddleware — tracks token usage via Extensions");
    println!();
    println!("Before hooks run top-to-bottom (logging → security → token)");
    println!("After hooks run bottom-to-top (token → security → logging)");
    println!("If security blocks, token counting never sees the call.");
    println!();
    println!("Usage with Agent:");
    println!("  let mut config = AgentConfig::new(convert_fn);");
    println!("  config.middleware = stack;");
    println!("  let agent = Agent::new(model, \"system prompt\", config);");
}
```

Update `crates/alva-core/Cargo.toml` to add example:
```toml
[[example]]
name = "middleware_basic"
```

- [ ] **Step 2.9: Verify compilation and tests**

Run: `cargo check -p alva-core && cargo test -p alva-core`
Expected: all pass.

Run: `cargo run --example middleware_basic -p alva-core`
Expected: prints middleware composition info.

- [ ] **Step 2.10: Commit**

```bash
git add crates/alva-core/
git commit -m "feat(alva-core): add async Middleware system with MiddlewareStack, Extensions, and examples"
```

---

## Task 3: Split alva-app-core into alva-tools

**Rationale:** Extract all 16 tool implementations into a standalone crate so tools can be reused independently of alva-app-core's runtime.

**Files:**
- Create: `crates/alva-tools/Cargo.toml`
- Create: `crates/alva-tools/src/lib.rs`
- Move: `crates/alva-app-core/src/agent/runtime/tools/*.rs` → `crates/alva-tools/src/`
- Move: `crates/alva-app-core/src/agent/runtime/tools/browser/` → `crates/alva-tools/src/browser/`
- Modify: `crates/alva-app-core/Cargo.toml` (add alva-tools dep, remove tool-specific deps)
- Modify: `crates/alva-app-core/src/lib.rs` (re-export from alva-tools)
- Modify: `Cargo.toml` (add workspace member)

- [ ] **Step 3.1: Create alva-tools Cargo.toml**

```toml
[package]
name = "alva-tools"
version = "0.1.0"
edition = "2021"
description = "Built-in tool implementations for the agent framework"

[dependencies]
alva-types = { path = "../alva-types" }

# Async
async-trait = "0.1"
tokio = { version = "1", features = ["process", "time", "fs", "sync", "io-util"] }
futures = "0.3"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

# Tool helpers
walkdir = "2"
regex = "1"
glob = "0.3"

# HTTP (internet_search, read_url)
reqwest = { version = "0.12", features = ["json", "stream"] }

# Browser automation
chromiumoxide = { version = "0.9", default-features = false }
base64 = "0.22"

# Logging
tracing = "0.1"

[features]
browser = ["chromiumoxide", "base64"]
default = ["browser"]
```

- [ ] **Step 3.2: Move tool files**

```bash
mkdir -p crates/alva-tools/src/browser

# Standard tools
for f in ask_human create_file execute_shell file_edit grep_search internet_search list_files read_url view_image; do
    cp crates/alva-app-core/src/agent/runtime/tools/${f}.rs crates/alva-tools/src/${f}.rs
done

# Browser tools
for f in browser_manager browser_start browser_stop browser_navigate browser_action browser_snapshot browser_screenshot browser_status; do
    cp crates/alva-app-core/src/agent/runtime/tools/browser/${f}.rs crates/alva-tools/src/browser/${f}.rs
done
cp crates/alva-app-core/src/agent/runtime/tools/browser/mod.rs crates/alva-tools/src/browser/mod.rs
```

- [ ] **Step 3.3: Create alva-tools/src/lib.rs**

```rust
//! Built-in tool implementations for the agent framework.
//!
//! Standard tools: execute_shell, create_file, file_edit, grep_search,
//! list_files, ask_human, internet_search, read_url, view_image.
//!
//! Browser tools (feature-gated): browser_start, browser_stop,
//! browser_navigate, browser_action, browser_snapshot, browser_screenshot,
//! browser_status.

pub mod ask_human;
pub mod create_file;
pub mod execute_shell;
pub mod file_edit;
pub mod grep_search;
pub mod internet_search;
pub mod list_files;
pub mod read_url;
pub mod view_image;

#[cfg(feature = "browser")]
pub mod browser;

use alva_types::ToolRegistry;

/// Register the 9 standard tools.
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    registry.register(Box::new(execute_shell::ExecuteShellTool));
    registry.register(Box::new(create_file::CreateFileTool));
    registry.register(Box::new(file_edit::FileEditTool));
    registry.register(Box::new(grep_search::GrepSearchTool));
    registry.register(Box::new(list_files::ListFilesTool));
    registry.register(Box::new(ask_human::AskHumanTool));
    registry.register(Box::new(internet_search::InternetSearchTool));
    registry.register(Box::new(read_url::ReadUrlTool));
    registry.register(Box::new(view_image::ViewImageTool));
}

/// Register all tools (9 standard + 7 browser).
#[cfg(feature = "browser")]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);

    let manager = browser::browser_manager::shared_browser_manager();
    registry.register(Box::new(browser::BrowserStartTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserStopTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserNavigateTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserActionTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserSnapshotTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserScreenshotTool { manager: manager.clone() }));
    registry.register(Box::new(browser::BrowserStatusTool { manager: manager.clone() }));
}

/// Without browser feature, register_all_tools == register_builtin_tools.
#[cfg(not(feature = "browser"))]
pub fn register_all_tools(registry: &mut ToolRegistry) {
    register_builtin_tools(registry);
}
```

- [ ] **Step 3.4: Update tool imports**

Each tool file uses `alva_types::{...}` directly — no changes needed since alva-tools depends on alva-types.

For tools that use `ctx.workspace()`, update to use the new `LocalToolContext` pattern:
```rust
let local = ctx.local().ok_or_else(|| AgentError::ToolError {
    tool_name: self.name().into(),
    message: "local filesystem context required".into(),
})?;
let cwd = local.workspace().to_path_buf();
```

- [ ] **Step 3.5: Add alva-tools to workspace**

In root `Cargo.toml`:
```toml
members = [
    # ... existing members ...
    "crates/alva-tools",
]
```

- [ ] **Step 3.6: Update alva-app-core to depend on alva-tools**

In `crates/alva-app-core/Cargo.toml`:
```toml
alva-tools = { path = "../alva-tools" }
```

Remove from alva-app-core's deps: `walkdir`, `glob`, `chromiumoxide`, `base64`, `reqwest` (now in alva-tools).

**Note:** Keep `regex` in alva-app-core if other modules still use it (e.g., security's sensitive_paths — which will move in Task 4).

- [ ] **Step 3.7: Update alva-app-core to re-export from alva-tools**

Replace `crates/alva-app-core/src/agent/runtime/tools/` module with a thin re-export:

In `crates/alva-app-core/src/lib.rs`, change:
```rust
// Old
pub use agent::runtime::tools::register_all_tools;
pub use agent::runtime::tools::browser::BrowserManager;

// New
pub use alva_tools::{register_all_tools, register_builtin_tools};
pub use alva_tools::browser::{BrowserManager, browser_manager::{SharedBrowserManager, shared_browser_manager}};
```

Remove the `crates/alva-app-core/src/agent/runtime/tools/` directory (replaced by alva-tools crate).

- [ ] **Step 3.8: Verify compilation**

Run: `cargo check -p alva-tools -p alva-app-core`
Expected: compiles.

- [ ] **Step 3.9: Run tests**

Run: `cargo test -p alva-tools -p alva-app-core`
Expected: all pass.

- [ ] **Step 3.10: Commit**

```bash
git add crates/alva-tools/ crates/alva-app-core/ Cargo.toml
git commit -m "refactor: extract 16 tool implementations into alva-tools crate"
```

---

## Task 4: Split alva-app-core into alva-security

**Rationale:** Extract security subsystem so it can be reused by any agent runtime, not just alva-app-core.

**Files:**
- Create: `crates/alva-security/Cargo.toml`
- Create: `crates/alva-security/src/lib.rs`
- Move: `crates/alva-app-core/src/agent/runtime/security/*.rs` → `crates/alva-security/src/`
- Modify: `crates/alva-app-core/Cargo.toml`
- Modify: `crates/alva-app-core/src/lib.rs`
- Modify: `Cargo.toml`

- [ ] **Step 4.1: Create alva-security Cargo.toml**

```toml
[package]
name = "alva-security"
version = "0.1.0"
edition = "2021"
description = "Security subsystem for the agent framework — permission management, sandbox, path filtering"

[dependencies]
alva-types = { path = "../alva-types" }

serde = { version = "1", features = ["derive"] }
serde_json = "1"
uuid = { version = "1", features = ["v4"] }
dirs = "5"
regex = "1"
tracing = "0.1"
tokio = { version = "1", features = ["sync"] }
```

- [ ] **Step 4.2: Move security files**

```bash
mkdir -p crates/alva-security/src

for f in guard permission sensitive_paths authorized_roots sandbox; do
    cp crates/alva-app-core/src/agent/runtime/security/${f}.rs crates/alva-security/src/${f}.rs
done
```

- [ ] **Step 4.3: Create alva-security/src/lib.rs**

```rust
//! Agent security subsystem — permission management, sandbox, path filtering.

pub mod guard;
pub mod permission;
pub mod sensitive_paths;
pub mod authorized_roots;
pub mod sandbox;

pub use guard::{SecurityGuard, SecurityDecision};
pub use permission::{PermissionManager, PermissionDecision};
pub use sensitive_paths::SensitivePathFilter;
pub use authorized_roots::AuthorizedRoots;
pub use sandbox::{SandboxConfig, SandboxMode};
```

- [ ] **Step 4.4: Fix imports in security files**

In `guard.rs`, replace:
```rust
// Old
use crate::ports::tool::ToolContext;
#[cfg(test)]
use crate::ports::tool::SrowToolContext;
// New
use alva_types::ToolContext;
```

For tests in `guard.rs`, replace `SrowToolContext` with a local test helper:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Local test context (replaces SrowToolContext from alva-app-core)
    struct TestToolContext {
        workspace: std::path::PathBuf,
    }

    impl alva_types::ToolContext for TestToolContext {
        fn session_id(&self) -> &str { "test-session" }
        fn get_config(&self, _key: &str) -> Option<String> { None }
        fn as_any(&self) -> &dyn std::any::Any { self }
        fn local(&self) -> Option<&dyn alva_types::LocalToolContext> { Some(self) }
    }

    impl alva_types::LocalToolContext for TestToolContext {
        fn workspace(&self) -> &std::path::Path { &self.workspace }
        fn allow_dangerous(&self) -> bool { false }
    }

    fn test_ctx() -> TestToolContext {
        TestToolContext { workspace: std::path::PathBuf::from("/projects/myapp") }
    }

    // ... rest of tests use test_ctx() instead of SrowToolContext ...
}
```

- [ ] **Step 4.5: Add to workspace and alva-app-core**

Root `Cargo.toml`: add `"crates/alva-security"` to members.

`crates/alva-app-core/Cargo.toml`: add `alva-security = { path = "../alva-security" }`.

- [ ] **Step 4.6: Update alva-app-core re-exports**

In `crates/alva-app-core/src/lib.rs`:
```rust
// Old
pub use agent::runtime::security::guard::{SecurityGuard, SecurityDecision};
// ...

// New
pub use alva_security::{
    SecurityGuard, SecurityDecision,
    PermissionManager, PermissionDecision,
    SensitivePathFilter, AuthorizedRoots,
    SandboxConfig, SandboxMode,
};
```

Remove `crates/alva-app-core/src/agent/runtime/security/` directory.

- [ ] **Step 4.7: Verify and commit**

Run: `cargo check -p alva-security -p alva-app-core && cargo test -p alva-security -p alva-app-core`

```bash
git add crates/alva-security/ crates/alva-app-core/ Cargo.toml
git commit -m "refactor: extract security subsystem into alva-security crate"
```

---

## Task 5: Split alva-app-core into alva-memory

**Rationale:** Extract the memory subsystem (FTS + vector search) into a standalone crate. **Note:** `agent/persistence/` (SqliteStorage for sessions) stays in alva-app-core because it has deep coupling to alva-app-core's domain types (`Session`, `SessionStatus`, `SessionStorage` trait). Only the memory-specific storage moves.

**Files:**
- Create: `crates/alva-memory/Cargo.toml`
- Create: `crates/alva-memory/src/lib.rs`
- Create: `crates/alva-memory/src/error.rs` (new MemoryError, replaces EngineError usage)
- Move: `crates/alva-app-core/src/agent/memory/` → `crates/alva-memory/src/`
- Modify: `crates/alva-app-core/Cargo.toml`
- Modify: `crates/alva-app-core/src/lib.rs`
- Modify: `Cargo.toml`

**Important:** `agent/persistence/` stays in alva-app-core — it depends on `domain::session::Session` and `ports::storage::SessionStorage` which are srow-specific.

- [ ] **Step 5.1: Create alva-memory Cargo.toml**

```toml
[package]
name = "alva-memory"
version = "0.1.0"
edition = "2021"
description = "Agent memory — FTS + vector hybrid search, embedding support"

[dependencies]
alva-types = { path = "../alva-types" }

async-trait = "0.1"
tokio = { version = "1", features = ["sync", "fs"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
chrono = { version = "0.4", features = ["serde"] }
tracing = "0.1"

# SQLite (for memory-specific FTS + vector storage, NOT session persistence)
rusqlite = { version = "0.31", features = ["bundled"] }
tokio-rusqlite = "0.5"
```

- [ ] **Step 5.2: Create alva-memory/src/error.rs**

The memory files currently use `crate::error::EngineError`. Create a dedicated error type:

```rust
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    #[error("storage error: {0}")]
    Storage(String),
    #[error("embedding error: {0}")]
    Embedding(String),
    #[error("sync error: {0}")]
    Sync(String),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 5.3: Move memory files (NOT persistence)**

```bash
mkdir -p crates/alva-memory/src

# Memory subsystem only
for f in types service sqlite embedding sync; do
    cp crates/alva-app-core/src/agent/memory/${f}.rs crates/alva-memory/src/${f}.rs
done
```

- [ ] **Step 5.4: Create alva-memory/src/lib.rs**

```rust
//! Agent memory — FTS + vector hybrid search, file sync, embedding support.
//!
//! Note: Session persistence (SqliteStorage for sessions/messages) stays in
//! alva-app-core because it depends on alva-app-core's domain model.

pub mod error;
pub mod types;
pub mod sqlite;
pub mod embedding;
pub mod sync;
pub mod service;

pub use error::MemoryError;
pub use service::MemoryService;
pub use types::{MemoryEntry, MemoryChunk, MemoryFile, SyncReport};
```

- [ ] **Step 5.5: Replace EngineError with MemoryError in all memory files**

In each moved file (`service.rs`, `sqlite.rs`, `embedding.rs`, `sync.rs`):
```rust
// Old
use crate::error::EngineError;
// New
use crate::error::MemoryError;
```

Replace all `EngineError::Storage(...)` with `MemoryError::Storage(...)`, etc.

- [ ] **Step 5.6: Add to workspace, update alva-app-core**

Add to workspace members. Update alva-app-core to depend on alva-memory.

In alva-app-core, add conversion: `impl From<MemoryError> for EngineError`.

Remove `crates/alva-app-core/src/agent/memory/` directory only.
**Keep** `crates/alva-app-core/src/agent/persistence/` in alva-app-core.

`rusqlite` and `tokio-rusqlite` stay in alva-app-core for persistence. Agent-memory has its own copy for memory-specific FTS storage.

- [ ] **Step 5.6: Verify and commit**

Run: `cargo check -p alva-memory -p alva-app-core && cargo test -p alva-memory -p alva-app-core`

```bash
git add crates/alva-memory/ crates/alva-app-core/ Cargo.toml
git commit -m "refactor: extract memory and persistence into alva-memory crate"
```

---

## Task 6: Create alva-runtime (thin orchestration layer)

**Rationale:** Provide a batteries-included runtime that composes alva-core + alva-tools + alva-security + alva-memory into a ready-to-use agent. This is where the unified init API lives.

**Files:**
- Create: `crates/alva-runtime/Cargo.toml`
- Create: `crates/alva-runtime/src/lib.rs`
- Create: `crates/alva-runtime/src/builder.rs`
- Create: `crates/alva-runtime/src/init.rs`
- Create: `crates/alva-runtime/examples/runtime_basic.rs`
- Modify: `Cargo.toml`

- [ ] **Step 6.1: Create alva-runtime Cargo.toml**

```toml
[package]
name = "alva-runtime"
version = "0.1.0"
edition = "2021"
description = "Batteries-included agent runtime — composes core, tools, security, and memory"

[dependencies]
alva-types = { path = "../alva-types" }
alva-core = { path = "../alva-core" }
alva-tools = { path = "../alva-tools" }
alva-security = { path = "../alva-security" }
alva-memory = { path = "../alva-memory" }

async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"

[features]
browser = ["alva-tools/browser"]
default = ["browser"]

[[example]]
name = "runtime_basic"
```

- [ ] **Step 6.2: Create alva-runtime/src/lib.rs**

```rust
//! Batteries-included agent runtime.
//!
//! Composes `alva-core` (execution engine) + `alva-tools` (built-in tools) +
//! `alva-security` (permission & sandbox) + `alva-memory` (persistence & search)
//! into a ready-to-use agent with a builder API.
//!
//! # Quick Start
//!
//! ```rust,no_run
//! use alva_runtime::AgentRuntime;
//!
//! let runtime = AgentRuntime::builder()
//!     .system_prompt("You are a helpful assistant.")
//!     .workspace("/path/to/project")
//!     .with_builtin_tools()
//!     .with_security()
//!     .build(model);
//! ```

pub mod builder;
pub mod init;

pub use builder::AgentRuntime;
pub use init::model;

// Re-export key types for convenience
pub use alva_core::{Agent, AgentEvent, AgentMessage, AgentConfig};
pub use alva_core::middleware::{Middleware, MiddlewareStack};
pub use alva_types::{Tool, ToolContext, ToolRegistry, LanguageModel, Provider, ProviderRegistry};
pub use alva_tools::{register_builtin_tools, register_all_tools};
pub use alva_security::{SecurityGuard, SandboxMode};
pub use alva_memory::MemoryService;
```

- [ ] **Step 6.3: Create alva-runtime/src/init.rs (unified init API)**

```rust
//! Unified model initialization — `model("provider/model_id")`.

use std::sync::Arc;
use alva_types::{LanguageModel, ProviderRegistry, ProviderError};

/// Parse a `provider/model_id` string and resolve it from the registry.
///
/// # Examples
///
/// ```rust,no_run
/// let registry = setup_providers(); // user sets up providers
/// let llm = alva_runtime::model("anthropic/claude-sonnet-4-20250514", &registry)?;
/// ```
pub fn model(
    spec: &str,
    registry: &ProviderRegistry,
) -> Result<Arc<dyn LanguageModel>, ProviderError> {
    let (provider_id, model_id) = spec.split_once('/').ok_or_else(|| {
        ProviderError::InvalidArgument {
            argument: "spec".to_string(),
            message: format!(
                "expected 'provider/model_id' format, got '{}'",
                spec
            ),
        }
    })?;
    registry.language_model(provider_id, model_id)
}
```

- [ ] **Step 6.4: Create alva-runtime/src/builder.rs**

```rust
//! Builder pattern for constructing a fully-configured agent runtime.

use std::path::PathBuf;
use std::sync::Arc;

use alva_core::middleware::MiddlewareStack;
use alva_core::{Agent, AgentConfig};
use alva_types::{
    LanguageModel, Message, ModelConfig, Tool, ToolRegistry,
};

use crate::init;

/// A fully-configured agent runtime with tools, security, and memory.
pub struct AgentRuntime {
    pub agent: Agent,
    pub tool_registry: ToolRegistry,
}

/// Builder for AgentRuntime.
pub struct AgentRuntimeBuilder {
    system_prompt: String,
    workspace: Option<PathBuf>,
    model_config: ModelConfig,
    middleware: MiddlewareStack,
    register_builtin: bool,
    register_browser: bool,
    custom_tools: Vec<Box<dyn Tool>>,
    convert_to_llm: Option<alva_core::types::ConvertToLlmFn>,
}

impl AgentRuntimeBuilder {
    pub fn new() -> Self {
        Self {
            system_prompt: String::new(),
            workspace: None,
            model_config: ModelConfig::default(),
            middleware: MiddlewareStack::new(),
            register_builtin: false,
            register_browser: false,
            custom_tools: Vec::new(),
            convert_to_llm: None,
        }
    }

    pub fn system_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.system_prompt = prompt.into();
        self
    }

    pub fn workspace(mut self, path: impl Into<PathBuf>) -> Self {
        self.workspace = Some(path.into());
        self
    }

    pub fn model_config(mut self, config: ModelConfig) -> Self {
        self.model_config = config;
        self
    }

    pub fn middleware(mut self, mw: Arc<dyn alva_core::middleware::Middleware>) -> Self {
        self.middleware.push(mw);
        self
    }

    pub fn with_builtin_tools(mut self) -> Self {
        self.register_builtin = true;
        self
    }

    pub fn with_browser_tools(mut self) -> Self {
        self.register_browser = true;
        self
    }

    pub fn tool(mut self, tool: Box<dyn Tool>) -> Self {
        self.custom_tools.push(tool);
        self
    }

    pub fn convert_to_llm(mut self, f: alva_core::types::ConvertToLlmFn) -> Self {
        self.convert_to_llm = Some(f);
        self
    }

    /// Build the runtime with the given language model.
    pub fn build(self, model: Arc<dyn LanguageModel>) -> AgentRuntime {
        let mut registry = ToolRegistry::new();

        // Register built-in tools
        if self.register_builtin || self.register_browser {
            if self.register_browser {
                alva_tools::register_all_tools(&mut registry);
            } else {
                alva_tools::register_builtin_tools(&mut registry);
            }
        }

        // Register custom tools
        for tool in self.custom_tools {
            registry.register(tool);
        }

        // Default convert_to_llm: extract Standard messages
        let convert_fn = self.convert_to_llm.unwrap_or_else(|| {
            Arc::new(|ctx: &alva_core::types::AgentContext<'_>| {
                let mut result = vec![Message::system(ctx.system_prompt)];
                for m in ctx.messages {
                    if let alva_core::AgentMessage::Standard(msg) = m {
                        result.push(msg.clone());
                    }
                }
                result
            })
        });

        let mut config = AgentConfig::new(convert_fn);
        config.middleware = self.middleware;

        // Build agent with tools
        let agent = Agent::new(model, self.system_prompt, config);

        AgentRuntime {
            agent,
            tool_registry: registry,
        }
    }
}

impl AgentRuntime {
    pub fn builder() -> AgentRuntimeBuilder {
        AgentRuntimeBuilder::new()
    }
}

impl Default for AgentRuntimeBuilder {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 6.5: Create runtime example**

Create `crates/alva-runtime/examples/runtime_basic.rs`:

```rust
//! Basic alva-runtime example — demonstrates the builder API.
//!
//! Run: `cargo run --example runtime_basic -p alva-runtime`
//!
//! Note: This example doesn't connect to a real LLM. It shows how to
//! compose the runtime with tools, middleware, and configuration.

use std::sync::Arc;

use alva_runtime::AgentRuntime;
use alva_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_types::{ToolCall, ToolContext, Message};
use async_trait::async_trait;

// A simple logging middleware
struct LogMiddleware;

#[async_trait]
impl Middleware for LogMiddleware {
    async fn before_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        _tool_ctx: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        println!("[runtime] executing tool: {}", tool_call.name);
        Ok(())
    }

    fn name(&self) -> &str { "log" }
}

fn main() {
    println!("=== Agent Runtime Builder Example ===\n");

    // In a real app, you'd get `model` from a ProviderRegistry:
    //   let registry = ProviderRegistry::new();
    //   registry.register(Arc::new(AnthropicProvider::new(api_key)));
    //   let model = alva_runtime::model("anthropic/claude-sonnet-4-20250514", &registry).unwrap();

    println!("Usage:");
    println!();
    println!("  // 1. Set up providers");
    println!("  let mut registry = ProviderRegistry::new();");
    println!("  registry.register(Arc::new(my_provider));");
    println!();
    println!("  // 2. Resolve model with unified API");
    println!("  let model = alva_runtime::model(\"anthropic/claude-sonnet-4-20250514\", &registry)?;");
    println!();
    println!("  // 3. Build runtime");
    println!("  let runtime = AgentRuntime::builder()");
    println!("      .system_prompt(\"You are a helpful coding assistant.\")");
    println!("      .workspace(\"/path/to/project\")");
    println!("      .with_builtin_tools()");
    println!("      .with_browser_tools()");
    println!("      .middleware(Arc::new(LogMiddleware))");
    println!("      .middleware(Arc::new(SecurityMiddleware::new(...)))");
    println!("      .build(model);");
    println!();
    println!("  // 4. Use the agent");
    println!("  let events = runtime.agent.prompt(vec![...]);");
    println!();
    println!("Tool registry contains:");
    println!("  - 9 standard tools (execute_shell, create_file, file_edit, ...)");
    println!("  - 7 browser tools (browser_start, browser_stop, ...)");
    println!("  - Custom tools added via .tool()");
}
```

- [ ] **Step 6.6: Add to workspace**

Root `Cargo.toml`: add `"crates/alva-runtime"`.

- [ ] **Step 6.7: Update alva-app-core to depend on alva-runtime**

In `crates/alva-app-core/Cargo.toml`:
```toml
alva-runtime = { path = "../alva-runtime" }
```

Update `alva-app-core/src/lib.rs` re-exports to delegate to alva-runtime where appropriate.

- [ ] **Step 6.8: Verify and commit**

Run: `cargo check -p alva-runtime && cargo run --example runtime_basic -p alva-runtime`
Expected: compiles and runs.

```bash
git add crates/alva-runtime/ crates/alva-app-core/ Cargo.toml
git commit -m "feat: add alva-runtime crate with builder API, unified model init, and examples"
```

---

## Task 7: Context Compression Middleware

**Rationale:** Long conversations exhaust context windows. A middleware that automatically summarizes/compresses old messages when token count exceeds a threshold.

**Files:**
- Create: `crates/alva-core/src/middleware/compression.rs`
- Modify: `crates/alva-core/src/middleware.rs` (re-export)

Note: alva-graph already has a `CompactionConfig` in `compaction.rs`. This middleware provides a more general approach that works at the alva-core level without requiring the graph layer.

- [ ] **Step 7.1: Create compression middleware**

Create `crates/alva-core/src/middleware/compression.rs`:

```rust
//! Context compression middleware — automatically compresses conversation
//! history when it exceeds a configurable token threshold.

use async_trait::async_trait;
use alva_types::Message;

use crate::middleware::{Middleware, MiddlewareContext, MiddlewareError};

/// Configuration for context compression.
pub struct CompressionConfig {
    /// Estimated max tokens before compression triggers.
    pub token_threshold: u32,
    /// Number of recent messages to always keep uncompressed.
    pub keep_recent: usize,
    /// Approximate tokens per character (for estimation).
    pub tokens_per_char: f32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            token_threshold: 100_000,
            keep_recent: 20,
            tokens_per_char: 0.25,
        }
    }
}

/// Middleware that compresses old messages by truncating their content
/// and replacing with a summary marker.
///
/// This is a simple token-budget approach. For LLM-powered summarization,
/// extend this middleware to call the model during `before_llm_call`.
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(CompressionConfig::default())
    }

    fn estimate_tokens(&self, messages: &[Message]) -> u32 {
        let total_chars: usize = messages
            .iter()
            .flat_map(|m| &m.content)
            .map(|block| match block {
                alva_types::ContentBlock::Text { text } => text.len(),
                alva_types::ContentBlock::Reasoning { text } => text.len(),
                alva_types::ContentBlock::ToolResult { content, .. } => content.len(),
                alva_types::ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                alva_types::ContentBlock::Image { data, .. } => data.len(),
            })
            .sum();
        (total_chars as f32 * self.config.tokens_per_char) as u32
    }
}

#[async_trait]
impl Middleware for CompressionMiddleware {
    async fn before_llm_call(
        &self,
        _ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let estimated = self.estimate_tokens(messages);
        if estimated <= self.config.token_threshold {
            return Ok(());
        }

        let total = messages.len();
        if total <= self.config.keep_recent + 1 {
            // Not enough messages to compress (keep system + recent)
            return Ok(());
        }

        // Keep first message (system prompt) + last N messages.
        // Replace everything in between with a summary marker.
        let compress_end = total - self.config.keep_recent;

        // Count compressed messages for the summary
        let compressed_count = compress_end - 1; // exclude system prompt

        // Build summary message
        let summary = Message::system(&format!(
            "[Context compressed: {} earlier messages were summarized to save tokens. \
             The conversation continues below with the most recent {} messages.]",
            compressed_count, self.config.keep_recent
        ));

        // Rebuild: system + summary + recent
        let mut new_messages = Vec::with_capacity(self.config.keep_recent + 2);
        new_messages.push(messages[0].clone()); // system prompt
        new_messages.push(summary);
        new_messages.extend_from_slice(&messages[compress_end..]);

        *messages = new_messages;
        tracing::info!(
            compressed = compressed_count,
            remaining = messages.len(),
            "context compressed"
        );

        Ok(())
    }

    fn name(&self) -> &str {
        "compression"
    }
}
```

- [ ] **Step 7.2: Convert middleware to directory module**

**Decision:** Use directory module structure for future extensibility.

```bash
mkdir -p crates/alva-core/src/middleware
mv crates/alva-core/src/middleware.rs crates/alva-core/src/middleware/mod.rs
# compression.rs was already created in Step 7.1
```

At the end of `middleware/mod.rs`, add:
```rust
pub mod compression;
pub use compression::{CompressionMiddleware, CompressionConfig};
```

- [ ] **Step 7.3: Add compression tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn compression_triggers_above_threshold() {
        let config = CompressionConfig {
            token_threshold: 10,
            keep_recent: 2,
            tokens_per_char: 1.0, // 1 token per char for easy math
        };
        let mw = CompressionMiddleware::new(config);

        let mut messages = vec![
            Message::system("system"),
            Message::user("msg1 with some content"),
            Message::user("msg2 with some content"),
            Message::user("msg3 with some content"),
            Message::user("msg4 recent"),
            Message::user("msg5 recent"),
        ];

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: "system".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();

        // Should have: system + summary + 2 recent = 4
        assert_eq!(messages.len(), 4);
    }

    #[tokio::test]
    async fn compression_skips_below_threshold() {
        let mw = CompressionMiddleware::new(CompressionConfig {
            token_threshold: 1_000_000,
            ..Default::default()
        });

        let mut messages = vec![
            Message::system("system"),
            Message::user("hello"),
        ];
        let original_len = messages.len();

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: "system".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();
        assert_eq!(messages.len(), original_len);
    }
}
```

- [ ] **Step 7.4: Verify and commit**

Run: `cargo test -p alva-core`

```bash
git add crates/alva-core/
git commit -m "feat(alva-core): add CompressionMiddleware for automatic context window management"
```

---

## Task 8: CI Dependency Firewall

**Rationale:** Automatically enforce that foundation crates stay dependency-free from application crates.

**Files:**
- Create: `scripts/ci-check-deps.sh`

- [ ] **Step 8.1: Create CI dependency check script**

Create `scripts/ci-check-deps.sh`:

```bash
#!/usr/bin/env bash
# CI Dependency Firewall — ensures foundation crates don't depend on application crates.
#
# Usage: ./scripts/ci-check-deps.sh
# Returns non-zero if any violation is found.

set -euo pipefail

RED='\033[0;31m'
GREEN='\033[0;32m'
NC='\033[0m'

VIOLATIONS=0

# Rule 1: alva-types must have ZERO workspace dependencies
echo "Checking alva-types has no workspace dependencies..."
AGENT_TYPES_DEPS=$(cargo tree -p alva-types --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^alva-types" || true)
if [ -n "$AGENT_TYPES_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-types depends on workspace crates:${NC}"
    echo "$AGENT_TYPES_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-types is dependency-free${NC}"
fi

# Rule 2: alva-core must only depend on alva-types
echo "Checking alva-core dependencies..."
AGENT_CORE_DEPS=$(cargo tree -p alva-core --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^alva-core" | grep -v "^alva-types" || true)
if [ -n "$AGENT_CORE_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-core has unexpected workspace deps:${NC}"
    echo "$AGENT_CORE_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-core only depends on alva-types${NC}"
fi

# Rule 3: alva-tools must only depend on alva-types
echo "Checking alva-tools dependencies..."
AGENT_TOOLS_DEPS=$(cargo tree -p alva-tools --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^alva-tools" | grep -v "^alva-types" || true)
if [ -n "$AGENT_TOOLS_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-tools has unexpected workspace deps:${NC}"
    echo "$AGENT_TOOLS_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-tools only depends on alva-types${NC}"
fi

# Rule 4: alva-security must only depend on alva-types
echo "Checking alva-security dependencies..."
AGENT_SEC_DEPS=$(cargo tree -p alva-security --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^alva-security" | grep -v "^alva-types" || true)
if [ -n "$AGENT_SEC_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-security has unexpected workspace deps:${NC}"
    echo "$AGENT_SEC_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-security only depends on alva-types${NC}"
fi

# Rule 5: alva-memory must only depend on alva-types
echo "Checking alva-memory dependencies..."
AGENT_MEM_DEPS=$(cargo tree -p alva-memory --depth 1 --prefix none 2>/dev/null | grep -E "^(agent-|srow-|protocol-)" | grep -v "^alva-memory" | grep -v "^alva-types" || true)
if [ -n "$AGENT_MEM_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-memory has unexpected workspace deps:${NC}"
    echo "$AGENT_MEM_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-memory only depends on alva-types${NC}"
fi

# Rule 6: protocol crates must not depend on srow-* crates
echo "Checking protocol crates..."
for PROTO in alva-skill alva-mcp alva-acp; do
    PROTO_DEPS=$(cargo tree -p $PROTO --depth 1 --prefix none 2>/dev/null | grep -E "^srow-" || true)
    if [ -n "$PROTO_DEPS" ]; then
        echo -e "${RED}VIOLATION: $PROTO depends on srow crates:${NC}"
        echo "$PROTO_DEPS"
        VIOLATIONS=$((VIOLATIONS + 1))
    else
        echo -e "${GREEN}OK: $PROTO does not depend on srow crates${NC}"
    fi
done

# Rule 7: alva-app must NOT directly depend on alva-types, alva-core, alva-graph
echo "Checking alva-app facade boundary..."
APP_DIRECT_DEPS=$(cargo tree -p alva-app --depth 1 --prefix none 2>/dev/null | grep -E "^(alva-types|alva-core|alva-graph)" || true)
if [ -n "$APP_DIRECT_DEPS" ]; then
    echo -e "${RED}VIOLATION: alva-app directly depends on internal crates (should use alva-app-core facade):${NC}"
    echo "$APP_DIRECT_DEPS"
    VIOLATIONS=$((VIOLATIONS + 1))
else
    echo -e "${GREEN}OK: alva-app only uses alva-app-core facade${NC}"
fi

echo ""
if [ $VIOLATIONS -gt 0 ]; then
    echo -e "${RED}FAILED: $VIOLATIONS dependency violation(s) found${NC}"
    exit 1
else
    echo -e "${GREEN}PASSED: All dependency boundaries are clean${NC}"
fi
```

- [ ] **Step 8.2: Make executable**

```bash
chmod +x scripts/ci-check-deps.sh
```

- [ ] **Step 8.3: Test the script**

Run: `./scripts/ci-check-deps.sh`
Expected: All checks pass (or expected violations for crates not yet created).

- [ ] **Step 8.4: Commit**

```bash
git add scripts/ci-check-deps.sh
git commit -m "ci: add dependency firewall script to enforce crate boundary rules"
```

---

## Task 9: Update alva-app-core facade and alva-app

**Rationale:** After the split, alva-app-core becomes a thin facade that re-exports from the new crates. alva-app continues to import only through alva-app-core.

**Files:**
- Modify: `crates/alva-app-core/Cargo.toml`
- Modify: `crates/alva-app-core/src/lib.rs`
- Modify: `crates/alva-app-core/src/agent/mod.rs`
- Modify: `crates/alva-app-core/src/agent/runtime/mod.rs`

- [ ] **Step 9.1: Clean up alva-app-core Cargo.toml**

Remove dependencies that moved to sub-crates:
- Remove: `walkdir`, `regex`, `glob` (in alva-tools)
- Remove: `chromiumoxide`, `base64` (in alva-tools)
- Remove: `rusqlite`, `tokio-rusqlite` (in alva-memory)

Add new deps:
```toml
alva-tools = { path = "../alva-tools" }
alva-security = { path = "../alva-security" }
alva-memory = { path = "../alva-memory" }
alva-runtime = { path = "../alva-runtime" }
```

- [ ] **Step 9.2: Update alva-app-core/src/lib.rs**

Replace direct module paths with re-exports from new crates:

```rust
// Re-export from new crates
pub use alva_tools;
pub use alva_security;
pub use alva_memory;
pub use alva_runtime;

// Keep existing re-exports for backward compatibility
pub use alva_tools::{register_all_tools, register_builtin_tools};
pub use alva_security::{SecurityGuard, SecurityDecision, PermissionManager, PermissionDecision,
                         SensitivePathFilter, AuthorizedRoots, SandboxConfig, SandboxMode};
pub use alva_memory::{MemoryService, MemoryEntry, MemoryChunk, MemoryFile, SyncReport};
```

- [ ] **Step 9.3: Remove migrated directories from alva-app-core**

Delete:
- `crates/alva-app-core/src/agent/runtime/tools/` (now in alva-tools)
- `crates/alva-app-core/src/agent/runtime/security/` (now in alva-security)
- `crates/alva-app-core/src/agent/memory/` (now in alva-memory)
- `crates/alva-app-core/src/agent/persistence/` (now in alva-memory)

Update `crates/alva-app-core/src/agent/runtime/mod.rs` to remove `pub mod tools;` and `pub mod security;`.
Update `crates/alva-app-core/src/agent/mod.rs` to remove `pub mod memory;` and `pub mod persistence;`.

- [ ] **Step 9.4: Verify full workspace compilation**

Run: `cargo check --workspace`
Expected: all crates compile.

- [ ] **Step 9.5: Run all tests**

Run: `cargo test --workspace`
Expected: all tests pass.

- [ ] **Step 9.6: Run dependency firewall**

Run: `./scripts/ci-check-deps.sh`
Expected: all checks pass.

- [ ] **Step 9.7: Commit**

```bash
git add -A
git commit -m "refactor: slim down alva-app-core to facade, delegate to alva-tools/security/memory/runtime"
```

---

## Task 10: Rename AgentConfig hooks (cleanup)

**Rationale:** AgentConfig in alva-core should be renamed to `AgentHooks` or the hooks struct should be clarified, eliminating the need for the awkward `AgentHookConfig` alias in alva-app-core.

**Files:**
- Modify: `crates/alva-core/src/types.rs`
- Modify: `crates/alva-core/src/lib.rs`
- Modify: `crates/alva-core/src/agent.rs`
- Modify: `crates/alva-core/src/agent_loop.rs`
- Modify: `crates/alva-app-core/src/lib.rs` (remove alias)

- [ ] **Step 10.1: Rename AgentConfig → AgentHooks in alva-core**

In `crates/alva-core/src/types.rs`:
```rust
// Old
pub struct AgentConfig { ... }
// New
pub struct AgentHooks { ... }
```

Update all references in alva-core (agent.rs, agent_loop.rs, tool_executor.rs).

- [ ] **Step 10.2: Update lib.rs exports**

```rust
pub use types::{AgentHooks, AgentMessage, AgentState, AgentContext, ...};
```

- [ ] **Step 10.3: Remove alias in alva-app-core**

In `crates/alva-app-core/src/lib.rs`:
```rust
// Old
pub use alva_core::types::{AgentConfig as AgentHookConfig, AgentContext};
// New
pub use alva_core::{AgentHooks, AgentContext};
```

- [ ] **Step 10.4: Verify and commit**

Run: `cargo check --workspace && cargo test --workspace`

```bash
git add crates/alva-core/ crates/alva-app-core/
git commit -m "refactor(alva-core): rename AgentConfig to AgentHooks for clarity"
```

---

## Execution Order Summary

| Task | Name | Depends On | Crate(s) |
|------|------|------------|----------|
| 1 | Genericize ToolContext | — | alva-types |
| 2 | Async Middleware | Task 1 | alva-core |
| 3 | Extract alva-tools | Task 1 | alva-tools, alva-app-core |
| 4 | Extract alva-security | Task 1 | alva-security, alva-app-core |
| 5 | Extract alva-memory | — | alva-memory, alva-app-core |
| 6 | Create alva-runtime | Tasks 2,3,4,5 | alva-runtime |
| 7 | Context Compression | Task 2 | alva-core |
| 8 | CI Dependency Firewall | Tasks 3,4,5 | scripts/ |
| 9 | Update Facade | Tasks 3,4,5,6 | alva-app-core |
| 10 | Rename AgentConfig | Task 2 | alva-core, alva-app-core |

**Parallelization opportunities:**
- Tasks 3, 4, 5 can run in parallel (independent crate extractions)
- Tasks 7 and 8 can run in parallel
- Task 6 must wait for 3, 4, 5
- Task 9 must wait for all splits
- Task 10 can run after Task 2

**Dual-path phase note:** During Tasks 3-5, alva-app-core will temporarily have both the old module paths AND new crate dependencies. Each extraction task immediately removes the old module directory from alva-app-core and replaces with re-exports from the new crate. Task 9 is for final cleanup and verification only — there should be no duplicated code between tasks.

---

## Post-Implementation Architecture

```
┌──────────────────────────────────────────────────────────────┐
│  alva-app (GPUI Desktop UI)                                  │ Layer 6
├──────────────────────────────────────────────────────────────┤
│  alva-app-core (Facade: skills + mcp + environment + re-exports) │ Layer 5
├──────────────────────────────────────────────────────────────┤
│  alva-runtime (Builder + unified init + composition)        │ Layer 4
├──────────┬───────────────┬───────────────────────────────────┤
│ agent-   │ agent-        │ agent-                            │ Layer 3
│ tools    │ security      │ memory                            │
├──────────┴───────────────┴───────────────────────────────────┤
│  alva-core (Agent loop + async Middleware + events)         │ Layer 2
├──────────────────────────────────────────────────────────────┤
│  alva-types (ToolContext base + LocalToolContext + traits)   │ Layer 1
├──────────┬───────────────┬───────────────────────────────────┤
│ protocol │ protocol      │ protocol                          │ Standalone
│ -skill   │ -mcp          │ -acp                              │
└──────────┴───────────────┴───────────────────────────────────┘
```

**Key improvements:**
- `alva-types::ToolContext` is generic (no filesystem assumption)
- `alva-core` has async middleware (replaces sync hooks)
- `alva-app-core` is 60% lighter (tools, security, memory extracted)
- `alva-runtime` provides LangChain-like builder API
- CI enforces dependency boundaries automatically
- Middleware enables context compression, logging, security as composable layers
