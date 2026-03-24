// INPUT:  alva_types (Message, ToolCall, ToolContext, ToolResult), async_trait, thiserror, crate::types::AgentMessage
// OUTPUT: Middleware (trait), MiddlewareStack, MiddlewareContext, MiddlewareError, Extensions, CompressionMiddleware, CompressionConfig
// POS:    Async middleware subsystem — defines the Middleware trait (onion model), type-safe Extensions store, and the ordered MiddlewareStack.
pub mod compression;

use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use alva_types::{Message, ToolCall, ToolContext, ToolResult};
use async_trait::async_trait;

use crate::types::AgentMessage;

pub use compression::{CompressionConfig, CompressionMiddleware};

// ---------------------------------------------------------------------------
// MiddlewareError
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, thiserror::Error)]
pub enum MiddlewareError {
    #[error("blocked: {reason}")]
    Blocked { reason: String },
    #[error("middleware error: {0}")]
    Other(String),
}

// ---------------------------------------------------------------------------
// Extensions — type-safe key-value store for inter-middleware communication
// ---------------------------------------------------------------------------

pub struct Extensions {
    map: HashMap<TypeId, Box<dyn Any + Send + Sync>>,
}

impl Extensions {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    pub fn insert<T: Send + Sync + 'static>(&mut self, val: T) {
        self.map.insert(TypeId::of::<T>(), Box::new(val));
    }

    pub fn get<T: Send + Sync + 'static>(&self) -> Option<&T> {
        self.map
            .get(&TypeId::of::<T>())
            .and_then(|b| b.downcast_ref())
    }

    pub fn get_mut<T: Send + Sync + 'static>(&mut self) -> Option<&mut T> {
        self.map
            .get_mut(&TypeId::of::<T>())
            .and_then(|b| b.downcast_mut())
    }
}

impl Default for Extensions {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// MiddlewareContext
// ---------------------------------------------------------------------------

/// Context passed through the middleware chain.
pub struct MiddlewareContext {
    pub session_id: String,
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub extensions: Extensions,
}

// ---------------------------------------------------------------------------
// Middleware trait
// ---------------------------------------------------------------------------

#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called before messages are sent to the LLM.
    async fn before_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, messages);
        Ok(())
    }

    /// Called after the LLM returns a response.
    async fn after_llm_call(
        &self,
        ctx: &mut MiddlewareContext,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, response);
        Ok(())
    }

    /// Called before a tool is executed.
    async fn before_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, tool_call, tool_context);
        Ok(())
    }

    /// Called after a tool has finished executing.
    async fn after_tool_call(
        &self,
        ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, tool_call, result);
        Ok(())
    }

    /// Called when the agent loop starts.
    async fn on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        let _ = ctx;
        Ok(())
    }

    /// Called when the agent loop ends.
    async fn on_agent_end(
        &self,
        ctx: &mut MiddlewareContext,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let _ = (ctx, error);
        Ok(())
    }

    /// Human-readable name for this middleware (defaults to type name).
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }

    /// Execution priority (lower values run first in before-hooks).
    /// Default is 100. Use lower values (e.g. 10) for early middleware like security,
    /// higher values (e.g. 200) for late middleware like compression.
    fn priority(&self) -> i32 {
        100
    }
}

// ---------------------------------------------------------------------------
// MiddlewareStack — ordered middleware layers (onion model)
// ---------------------------------------------------------------------------

/// Ordered middleware stack.
///
/// **Before** hooks run top-to-bottom (in insertion order).
/// **After** hooks run bottom-to-top (reverse order) — the onion model.
pub struct MiddlewareStack {
    layers: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self {
            layers: Vec::new(),
        }
    }

    /// Append a middleware layer to the stack (insertion order).
    pub fn push(&mut self, middleware: Arc<dyn Middleware>) {
        self.layers.push(middleware);
    }

    /// Insert a middleware layer sorted by priority (lower values first).
    /// Stable: middleware with equal priority preserves insertion order.
    pub fn push_sorted(&mut self, middleware: Arc<dyn Middleware>) {
        let prio = middleware.priority();
        let pos = self
            .layers
            .iter()
            .position(|m| m.priority() > prio)
            .unwrap_or(self.layers.len());
        self.layers.insert(pos, middleware);
    }

    pub fn is_empty(&self) -> bool {
        self.layers.is_empty()
    }

    pub fn len(&self) -> usize {
        self.layers.len()
    }

    // -- before hooks: top-to-bottom ----------------------------------------

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

    pub async fn run_on_agent_start(
        &self,
        ctx: &mut MiddlewareContext,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.on_agent_start(ctx).await?;
        }
        Ok(())
    }

    // -- after hooks: bottom-to-top -----------------------------------------

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
}

impl Default for MiddlewareStack {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Helper: build a minimal MiddlewareContext
    // -----------------------------------------------------------------------
    fn test_ctx() -> MiddlewareContext {
        MiddlewareContext {
            session_id: "test-session".to_string(),
            system_prompt: "test prompt".to_string(),
            messages: Vec::new(),
            extensions: Extensions::new(),
        }
    }

    // -----------------------------------------------------------------------
    // Test: Extensions type-safe store
    // -----------------------------------------------------------------------
    #[test]
    fn test_extensions_insert_get() {
        let mut ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct TokenCount(u32);

        #[derive(Debug, PartialEq)]
        struct RequestId(String);

        ext.insert(TokenCount(42));
        ext.insert(RequestId("req-123".to_string()));

        assert_eq!(ext.get::<TokenCount>(), Some(&TokenCount(42)));
        assert_eq!(
            ext.get::<RequestId>(),
            Some(&RequestId("req-123".to_string()))
        );
        assert_eq!(ext.get::<String>(), None);
    }

    #[test]
    fn test_extensions_get_mut() {
        let mut ext = Extensions::new();

        #[derive(Debug, PartialEq)]
        struct Counter(u32);

        ext.insert(Counter(0));
        if let Some(c) = ext.get_mut::<Counter>() {
            c.0 += 10;
        }
        assert_eq!(ext.get::<Counter>(), Some(&Counter(10)));
    }

    // -----------------------------------------------------------------------
    // Test: middleware execution order (onion model)
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_middleware_execution_order() {
        // Track the order in which before/after hooks fire.
        let order = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));

        struct OrderMiddleware {
            label: String,
            order: Arc<parking_lot::Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl Middleware for OrderMiddleware {
            async fn before_llm_call(
                &self,
                _ctx: &mut MiddlewareContext,
                _messages: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                self.order
                    .lock()
                    .push(format!("before:{}", self.label));
                Ok(())
            }
            async fn after_llm_call(
                &self,
                _ctx: &mut MiddlewareContext,
                _response: &mut Message,
            ) -> Result<(), MiddlewareError> {
                self.order
                    .lock()
                    .push(format!("after:{}", self.label));
                Ok(())
            }
            fn name(&self) -> &str {
                &self.label
            }
        }

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(OrderMiddleware {
            label: "A".to_string(),
            order: order.clone(),
        }));
        stack.push(Arc::new(OrderMiddleware {
            label: "B".to_string(),
            order: order.clone(),
        }));
        stack.push(Arc::new(OrderMiddleware {
            label: "C".to_string(),
            order: order.clone(),
        }));

        let mut ctx = test_ctx();
        let mut msgs = Vec::new();
        stack.run_before_llm_call(&mut ctx, &mut msgs).await.unwrap();

        let mut response = Message {
            id: "msg-1".to_string(),
            role: alva_types::MessageRole::Assistant,
            content: vec![],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        stack.run_after_llm_call(&mut ctx, &mut response).await.unwrap();

        let trace = order.lock().clone();
        // Before: top-to-bottom (A, B, C)
        // After: bottom-to-top (C, B, A)
        assert_eq!(
            trace,
            vec![
                "before:A",
                "before:B",
                "before:C",
                "after:C",
                "after:B",
                "after:A",
            ]
        );
    }

    // -----------------------------------------------------------------------
    // Test: short-circuit on Blocked error
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_short_circuit_on_block() {
        let call_count = Arc::new(AtomicUsize::new(0));

        struct BlockingMiddleware;

        #[async_trait]
        impl Middleware for BlockingMiddleware {
            async fn before_llm_call(
                &self,
                _ctx: &mut MiddlewareContext,
                _messages: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                Err(MiddlewareError::Blocked {
                    reason: "forbidden".to_string(),
                })
            }
        }

        struct CountingMiddleware {
            count: Arc<AtomicUsize>,
        }

        #[async_trait]
        impl Middleware for CountingMiddleware {
            async fn before_llm_call(
                &self,
                _ctx: &mut MiddlewareContext,
                _messages: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                self.count.fetch_add(1, Ordering::SeqCst);
                Ok(())
            }
        }

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(BlockingMiddleware));
        stack.push(Arc::new(CountingMiddleware {
            count: call_count.clone(),
        }));

        let mut ctx = test_ctx();
        let mut msgs = Vec::new();
        let result = stack.run_before_llm_call(&mut ctx, &mut msgs).await;

        assert!(result.is_err());
        assert!(
            matches!(result, Err(MiddlewareError::Blocked { reason }) if reason == "forbidden")
        );
        // CountingMiddleware should never have been called.
        assert_eq!(call_count.load(Ordering::SeqCst), 0);
    }

    // -----------------------------------------------------------------------
    // Test: on_agent_start / on_agent_end ordering
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_agent_lifecycle_order() {
        let order = Arc::new(parking_lot::Mutex::new(Vec::<String>::new()));

        struct LifecycleMiddleware {
            label: String,
            order: Arc<parking_lot::Mutex<Vec<String>>>,
        }

        #[async_trait]
        impl Middleware for LifecycleMiddleware {
            async fn on_agent_start(
                &self,
                _ctx: &mut MiddlewareContext,
            ) -> Result<(), MiddlewareError> {
                self.order
                    .lock()
                    .push(format!("start:{}", self.label));
                Ok(())
            }
            async fn on_agent_end(
                &self,
                _ctx: &mut MiddlewareContext,
                _error: Option<&str>,
            ) -> Result<(), MiddlewareError> {
                self.order
                    .lock()
                    .push(format!("end:{}", self.label));
                Ok(())
            }
        }

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(LifecycleMiddleware {
            label: "X".to_string(),
            order: order.clone(),
        }));
        stack.push(Arc::new(LifecycleMiddleware {
            label: "Y".to_string(),
            order: order.clone(),
        }));

        let mut ctx = test_ctx();
        stack.run_on_agent_start(&mut ctx).await.unwrap();
        stack.run_on_agent_end(&mut ctx, None).await.unwrap();

        let trace = order.lock().clone();
        assert_eq!(trace, vec!["start:X", "start:Y", "end:Y", "end:X"]);
    }

    // -----------------------------------------------------------------------
    // Test: empty stack is a no-op
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_empty_stack() {
        let stack = MiddlewareStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);

        let mut ctx = test_ctx();
        let mut msgs = Vec::new();
        assert!(stack.run_before_llm_call(&mut ctx, &mut msgs).await.is_ok());
    }

    // -----------------------------------------------------------------------
    // Test: middleware can mutate messages
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_middleware_mutates_messages() {
        struct InjectSystemNote;

        #[async_trait]
        impl Middleware for InjectSystemNote {
            async fn before_llm_call(
                &self,
                _ctx: &mut MiddlewareContext,
                messages: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                messages.push(Message::system("Injected by middleware"));
                Ok(())
            }
        }

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(InjectSystemNote));

        let mut ctx = test_ctx();
        let mut msgs = vec![Message::user("Hello")];
        stack.run_before_llm_call(&mut ctx, &mut msgs).await.unwrap();

        assert_eq!(msgs.len(), 2);
        assert_eq!(msgs[1].text_content(), "Injected by middleware");
    }

    // -----------------------------------------------------------------------
    // Test: cross-middleware communication via Extensions
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_extensions_cross_middleware() {
        #[derive(Debug)]
        struct TokenBudget(u32);

        struct BudgetSetter;

        #[async_trait]
        impl Middleware for BudgetSetter {
            async fn on_agent_start(
                &self,
                ctx: &mut MiddlewareContext,
            ) -> Result<(), MiddlewareError> {
                ctx.extensions.insert(TokenBudget(1000));
                Ok(())
            }
        }

        struct BudgetReader {
            observed: Arc<parking_lot::Mutex<Option<u32>>>,
        }

        #[async_trait]
        impl Middleware for BudgetReader {
            async fn on_agent_start(
                &self,
                ctx: &mut MiddlewareContext,
            ) -> Result<(), MiddlewareError> {
                if let Some(budget) = ctx.extensions.get::<TokenBudget>() {
                    *self.observed.lock() = Some(budget.0);
                }
                Ok(())
            }
        }

        let observed = Arc::new(parking_lot::Mutex::new(None));

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(BudgetSetter));
        stack.push(Arc::new(BudgetReader {
            observed: observed.clone(),
        }));

        let mut ctx = test_ctx();
        stack.run_on_agent_start(&mut ctx).await.unwrap();

        assert_eq!(*observed.lock(), Some(1000));
    }
}
