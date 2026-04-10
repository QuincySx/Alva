// INPUT:  std::sync::Arc, async_trait, alva_types::{AgentError, Message, ToolCall, ToolOutput}, crate::state::AgentState
// OUTPUT: LlmCallFn, ToolCallFn, Middleware (trait), MiddlewareStack
// POS:    Middleware trait and stack — receives &mut AgentState directly, with wrap hooks for interceptor pattern.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use alva_types::{AgentError, Message, ToolCall, ToolOutput};
use async_trait::async_trait;

use crate::state::AgentState;

// Re-export shared types so downstream users can access them via `middleware::` if needed.
pub use crate::shared::{Extensions, MiddlewareError, MiddlewarePriority};

// ---------------------------------------------------------------------------
// Callback traits for wrap hooks
// ---------------------------------------------------------------------------

/// Callback for the "next" step in the LLM wrapping chain.
#[async_trait]
pub trait LlmCallFn: Send + Sync {
    async fn call(
        &self,
        state: &mut AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError>;
}

/// Callback for the "next" step in the tool wrapping chain.
#[async_trait]
pub trait ToolCallFn: Send + Sync {
    async fn call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError>;
}

// ---------------------------------------------------------------------------
// Middleware trait
// ---------------------------------------------------------------------------

/// V2 Middleware trait — receives `&mut AgentState` directly.
///
/// Key differences from v1:
/// - `on_agent_start` / `on_agent_end` lifecycle hooks
/// - `before_*` / `after_*` inspection hooks receive `&mut AgentState`
/// - `wrap_*` interceptor hooks receive `&mut AgentState` (mutable, state is threaded through the chain)
/// - All methods have default no-op implementations
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called when the agent run starts.
    async fn on_agent_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called when the agent run ends (with optional error description).
    async fn on_agent_end(
        &self,
        _state: &mut AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called before messages are sent to the LLM.
    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called after the LLM returns a response.
    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Wrap the entire LLM call — can modify request, response, retry, or skip.
    /// Default: just calls `next`.
    async fn wrap_llm_call(
        &self,
        state: &mut AgentState,
        messages: Vec<Message>,
        next: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        next.call(state, messages)
            .await
            .map_err(MiddlewareError::from)
    }

    /// Called before a tool is executed.
    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called after a tool has finished executing.
    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
        _result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Wrap a single tool execution — can modify args, result, retry, or skip.
    /// Default: just calls `next`.
    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        next.call(state, tool_call)
            .await
            .map_err(MiddlewareError::from)
    }

    /// Called once after the agent is fully constructed, before any run.
    /// Middleware that needs shared infrastructure (bus, workspace) can grab it here.
    fn configure(&self, _ctx: &MiddlewareContext) {}

    /// Execution priority (lower values run first in before-hooks).
    /// Default is `MiddlewarePriority::DEFAULT` (3000).
    fn priority(&self) -> i32 {
        MiddlewarePriority::DEFAULT
    }

    /// Human-readable name for this middleware (defaults to type name).
    fn name(&self) -> &str {
        std::any::type_name::<Self>()
    }
}

/// Context passed to middleware during [`Middleware::configure`].
/// Contains shared infrastructure created during agent construction.
pub struct MiddlewareContext {
    pub bus: Option<alva_types::BusHandle>,
    pub workspace: Option<std::path::PathBuf>,
}

// ---------------------------------------------------------------------------
// MiddlewareStack — ordered middleware layers (onion model)
// ---------------------------------------------------------------------------

/// Ordered V2 middleware stack.
///
/// **Before** hooks run top-to-bottom (in insertion order).
/// **After** hooks run bottom-to-top (reverse order) — the onion model.
pub struct MiddlewareStack {
    layers: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
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

    /// Call `configure()` on all middleware with the given context.
    pub fn configure_all(&self, ctx: &MiddlewareContext) {
        for layer in &self.layers {
            layer.configure(ctx);
        }
    }

    /// Iterate over all middleware layers in order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Middleware>> {
        self.layers.iter()
    }

    // -- lifecycle hooks ---------------------------------------------------

    /// Run `on_agent_start` top-to-bottom.
    pub async fn run_on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.on_agent_start(state).await?;
        }
        Ok(())
    }

    /// Run `on_agent_end` bottom-to-top.
    ///
    /// Unlike other hooks, this always runs **all** layers even if some fail,
    /// so that every middleware gets a chance to clean up. The first error
    /// encountered is returned after all layers have run.
    pub async fn run_on_agent_end(
        &self,
        state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let mut first_error: Option<MiddlewareError> = None;
        for layer in self.layers.iter().rev() {
            if let Err(e) = layer.on_agent_end(state, error).await {
                tracing::warn!(
                    error = %e,
                    middleware = layer.name(),
                    "on_agent_end failed"
                );
                if first_error.is_none() {
                    first_error = Some(e);
                }
            }
        }
        first_error.map_or(Ok(()), Err)
    }

    // -- before hooks: top-to-bottom ---------------------------------------

    /// Run `before_llm_call` top-to-bottom.
    pub async fn run_before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = std::time::Instant::now();
            layer.before_llm_call(state, messages).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::info!(middleware = layer.name(), hook = "before_llm_call", duration_ms = elapsed, "middleware hook");
        }
        Ok(())
    }

    /// Run `before_tool_call` top-to-bottom.
    pub async fn run_before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = std::time::Instant::now();
            layer.before_tool_call(state, tool_call).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::info!(middleware = layer.name(), hook = "before_tool_call", duration_ms = elapsed, "middleware hook");
        }
        Ok(())
    }

    // -- after hooks: bottom-to-top ----------------------------------------

    /// Run `after_llm_call` bottom-to-top.
    pub async fn run_after_llm_call(
        &self,
        state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            let start = std::time::Instant::now();
            layer.after_llm_call(state, response).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::info!(middleware = layer.name(), hook = "after_llm_call", duration_ms = elapsed, "middleware hook");
        }
        Ok(())
    }

    /// Run `after_tool_call` bottom-to-top.
    pub async fn run_after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            let start = std::time::Instant::now();
            layer.after_tool_call(state, tool_call, result).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::info!(middleware = layer.name(), hook = "after_tool_call", duration_ms = elapsed, "middleware hook");
        }
        Ok(())
    }

    // -- wrap hooks -----------------------------------------------------------

    /// Run `wrap_llm_call` through the full middleware chain.
    ///
    /// Builds a nested chain: `mw[0].wrap(mw[1].wrap(... mw[n].wrap(actual)))`.
    /// Each middleware's `next` is a closure that calls the remaining chain.
    pub async fn run_wrap_llm_call(
        &self,
        state: &mut AgentState,
        messages: Vec<Message>,
        actual_call: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        self.call_wrap_llm_chain(state, messages, actual_call, 0)
            .await
    }

    /// Recursive helper for LLM wrap chain (using `Box::pin` for async recursion).
    fn call_wrap_llm_chain<'a>(
        &'a self,
        state: &'a mut AgentState,
        messages: Vec<Message>,
        actual_call: &'a dyn LlmCallFn,
        index: usize,
    ) -> Pin<Box<dyn Future<Output = Result<Message, MiddlewareError>> + Send + 'a>> {
        Box::pin(async move {
            if index >= self.layers.len() {
                // No more middleware -- call actual LLM
                actual_call
                    .call(state, messages)
                    .await
                    .map_err(MiddlewareError::from)
            } else {
                // Create a "next" that calls the rest of the chain
                let next = ChainedLlmCall {
                    stack: self,
                    actual_call,
                    next_index: index + 1,
                };
                self.layers[index]
                    .wrap_llm_call(state, messages, &next)
                    .await
            }
        })
    }

    /// Run `wrap_tool_call` through the full middleware chain.
    ///
    /// Builds a nested chain: `mw[0].wrap(mw[1].wrap(... mw[n].wrap(actual)))`.
    pub async fn run_wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        actual_call: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        self.call_wrap_tool_chain(state, tool_call, actual_call, 0)
            .await
    }

    /// Recursive helper for tool wrap chain (using `Box::pin` for async recursion).
    fn call_wrap_tool_chain<'a>(
        &'a self,
        state: &'a mut AgentState,
        tool_call: &'a ToolCall,
        actual_call: &'a dyn ToolCallFn,
        index: usize,
    ) -> Pin<Box<dyn Future<Output = Result<ToolOutput, MiddlewareError>> + Send + 'a>> {
        Box::pin(async move {
            if index >= self.layers.len() {
                // No more middleware -- call actual tool
                actual_call
                    .call(state, tool_call)
                    .await
                    .map_err(MiddlewareError::from)
            } else {
                // Create a "next" that calls the rest of the chain
                let next = ChainedToolCall {
                    stack: self,
                    actual_call,
                    next_index: index + 1,
                };
                self.layers[index]
                    .wrap_tool_call(state, tool_call, &next)
                    .await
            }
        })
    }
}

// ---------------------------------------------------------------------------
// Helper structs for chained wrap calls
// ---------------------------------------------------------------------------

/// Wraps the remaining LLM middleware chain as a `LlmCallFn` so each
/// middleware layer receives a proper `next` callback.
///
/// State is NOT held here — it is passed through `call(state, messages)` at each level,
/// allowing `&mut AgentState` to flow through the chain without borrow conflicts.
struct ChainedLlmCall<'a> {
    stack: &'a MiddlewareStack,
    actual_call: &'a dyn LlmCallFn,
    next_index: usize,
}

#[async_trait]
impl<'a> LlmCallFn for ChainedLlmCall<'a> {
    async fn call(
        &self,
        state: &mut AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError> {
        self.stack
            .call_wrap_llm_chain(state, messages, self.actual_call, self.next_index)
            .await
            .map_err(MiddlewareError::into_agent_error)
    }
}

/// Wraps the remaining tool middleware chain as a `ToolCallFn` so each
/// middleware layer receives a proper `next` callback.
///
/// State is NOT held here — it is passed through `call(state, tool_call)` at each level.
struct ChainedToolCall<'a> {
    stack: &'a MiddlewareStack,
    actual_call: &'a dyn ToolCallFn,
    next_index: usize,
}

#[async_trait]
impl<'a> ToolCallFn for ChainedToolCall<'a> {
    async fn call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError> {
        self.stack
            .call_wrap_tool_chain(state, tool_call, self.actual_call, self.next_index)
            .await
            .map_err(MiddlewareError::into_agent_error)
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

    #[test]
    fn empty_stack() {
        let stack = MiddlewareStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
    }

    #[test]
    fn push_sorted_by_priority() {
        struct PrioMiddleware {
            prio: i32,
            label: String,
        }

        #[async_trait]
        impl Middleware for PrioMiddleware {
            fn priority(&self) -> i32 {
                self.prio
            }
            fn name(&self) -> &str {
                &self.label
            }
        }

        let mut stack = MiddlewareStack::new();

        // Insert out of order
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: MiddlewarePriority::OBSERVATION, // 5000
            label: "obs".to_string(),
        }));
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: MiddlewarePriority::SECURITY, // 1000
            label: "sec".to_string(),
        }));
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: MiddlewarePriority::CONTEXT, // 3000
            label: "ctx".to_string(),
        }));

        let names: Vec<&str> = stack.iter().map(|m| m.name()).collect();
        assert_eq!(names, vec!["sec", "ctx", "obs"]);
    }

    #[test]
    fn push_sorted_stable_for_equal_priority() {
        struct PrioMiddleware {
            prio: i32,
            label: String,
        }

        #[async_trait]
        impl Middleware for PrioMiddleware {
            fn priority(&self) -> i32 {
                self.prio
            }
            fn name(&self) -> &str {
                &self.label
            }
        }

        let mut stack = MiddlewareStack::new();

        // All same priority — insertion order preserved
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: 3000,
            label: "A".to_string(),
        }));
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: 3000,
            label: "B".to_string(),
        }));
        stack.push_sorted(Arc::new(PrioMiddleware {
            prio: 3000,
            label: "C".to_string(),
        }));

        let names: Vec<&str> = stack.iter().map(|m| m.name()).collect();
        assert_eq!(names, vec!["A", "B", "C"]);
    }
}
