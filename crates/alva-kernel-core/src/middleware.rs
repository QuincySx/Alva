// INPUT:  std::sync::Arc, async_trait, alva_kernel_abi::{AgentError, Message, ToolCall, ToolOutput}, crate::state::AgentState
// OUTPUT: LlmCallFn, ToolCallFn, Middleware (trait), MiddlewareStack
// POS:    Middleware trait and stack — receives &AgentState directly, with wrap hooks for interceptor pattern.

use std::sync::Arc;

use alva_kernel_abi::{AgentError, AgentMessage, Message, ToolCall, ToolOutput};
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
        state: &AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError>;
}

/// Callback for the "next" step in the tool wrapping chain.
#[async_trait]
pub trait ToolCallFn: Send + Sync {
    async fn call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError>;
}

// ---------------------------------------------------------------------------
// Middleware trait
// ---------------------------------------------------------------------------

/// Middleware trait — receives `&AgentState` directly.
///
/// Key differences from the old design:
/// - `on_agent_start` / `on_agent_end` lifecycle hooks
/// - `before_*` / `after_*` inspection hooks receive `&AgentState`
/// - `wrap_*` interceptor hooks receive `&AgentState` (shared, threaded through the chain via reborrow)
/// - All methods have default no-op implementations
///
/// Why `&AgentState` rather than `&mut AgentState`: the onion composition of
/// `wrap_llm_call` / `wrap_tool_call` re-enters the chain through a `&dyn
/// LlmCallFn` / `ToolCallFn` continuation. An exclusive borrow cannot thread
/// cleanly across those nested async continuations. This is a *borrow* matter,
/// not an object-safety one — `#[async_trait] fn drive(&self, state: &mut
/// AgentState, …)` would itself be object-safe. Shared `&AgentState` plus
/// interior-mutable `Extensions` is how mutability is now expressed when a
/// middleware genuinely needs to store state mid-run.
#[async_trait]
pub trait Middleware: Send + Sync {
    /// Called when the agent run starts.
    async fn on_agent_start(&self, _state: &AgentState) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called after an input message has been appended to the session.
    async fn input_committed(
        &self,
        _state: &AgentState,
        _message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called when the agent run ends (with optional error description).
    async fn on_agent_end(
        &self,
        _state: &AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called before messages are sent to the LLM.
    async fn before_llm_call(
        &self,
        _state: &AgentState,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called after the LLM returns a response.
    async fn after_llm_call(
        &self,
        _state: &AgentState,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Wrap the entire LLM call — can modify request, response, retry, or skip.
    /// Default: just calls `next`.
    async fn wrap_llm_call(
        &self,
        state: &AgentState,
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
        _state: &AgentState,
        _tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Called after a tool has finished executing.
    async fn after_tool_call(
        &self,
        _state: &AgentState,
        _tool_call: &ToolCall,
        _result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Wrap a single tool execution — can modify args, result, retry, or skip.
    /// Default: just calls `next`.
    async fn wrap_tool_call(
        &self,
        state: &AgentState,
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
    pub bus: Option<alva_kernel_abi::BusHandle>,
    pub workspace: Option<std::path::PathBuf>,
    /// Scoped session for this middleware. None only in test setups that
    /// do not wire a session (e.g. unit tests of middleware hooks in isolation).
    /// Middleware that emits events should call `ctx.session.as_ref()?.append(...)`.
    pub session: Option<alva_kernel_abi::agent_session::ScopedSession>,
}

// ---------------------------------------------------------------------------
// MiddlewareStack — ordered middleware layers (onion model)
// ---------------------------------------------------------------------------

/// Ordered middleware stack.
///
/// **Before** hooks run top-to-bottom (in insertion order).
/// **After** hooks run bottom-to-top (reverse order) — the onion model.
#[derive(Clone)]
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

    /// Return the names of all middleware in the stack, in insertion order.
    pub fn names(&self) -> Vec<String> {
        self.layers.iter().map(|m| m.name().to_string()).collect()
    }

    // -- lifecycle hooks ---------------------------------------------------

    /// Run `on_agent_start` top-to-bottom.
    pub async fn run_on_agent_start(&self, state: &AgentState) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = web_time::Instant::now();
            layer.on_agent_start(state).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "on_agent_start",
                duration_ms = elapsed,
                "middleware hook"
            );
        }
        Ok(())
    }

    /// Run `input_committed` top-to-bottom.
    pub async fn run_input_committed(
        &self,
        state: &AgentState,
        message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = web_time::Instant::now();
            layer.input_committed(state, message).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "input_committed",
                duration_ms = elapsed,
                "middleware hook"
            );
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
        state: &AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        let mut first_error: Option<MiddlewareError> = None;
        for layer in self.layers.iter().rev() {
            let start = web_time::Instant::now();
            let result = layer.on_agent_end(state, error).await;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "on_agent_end",
                duration_ms = elapsed,
                "middleware hook"
            );
            if let Err(e) = result {
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
        state: &AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = web_time::Instant::now();
            layer.before_llm_call(state, messages).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "before_llm_call",
                duration_ms = elapsed,
                "middleware hook"
            );
        }
        Ok(())
    }

    /// Run `before_tool_call` top-to-bottom.
    pub async fn run_before_tool_call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            let start = web_time::Instant::now();
            layer.before_tool_call(state, tool_call).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "before_tool_call",
                duration_ms = elapsed,
                "middleware hook"
            );
        }
        Ok(())
    }

    // -- after hooks: bottom-to-top ----------------------------------------

    /// Run `after_llm_call` bottom-to-top.
    pub async fn run_after_llm_call(
        &self,
        state: &AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            let start = web_time::Instant::now();
            layer.after_llm_call(state, response).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "after_llm_call",
                duration_ms = elapsed,
                "middleware hook"
            );
        }
        Ok(())
    }

    /// Run `after_tool_call` bottom-to-top.
    pub async fn run_after_tool_call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            let start = web_time::Instant::now();
            layer.after_tool_call(state, tool_call, result).await?;
            let elapsed = start.elapsed().as_millis() as u64;
            tracing::debug!(
                middleware = layer.name(),
                hook = "after_tool_call",
                duration_ms = elapsed,
                "middleware hook"
            );
        }
        Ok(())
    }

    // -- wrap hooks -----------------------------------------------------------

    /// Run `wrap_llm_call` through the full middleware chain.
    ///
    /// Builds a nested chain: `mw[0].wrap(mw[1].wrap(... mw[n].wrap(actual)))`.
    /// The chain is constructed as an owned `Arc<dyn LlmCallFn>` fold from the
    /// innermost layer outward; each `next` holds its inner layer, so no
    /// function-level async recursion (and no per-layer `Box::pin`) is needed.
    pub async fn run_wrap_llm_call(
        &self,
        state: &AgentState,
        messages: Vec<Message>,
        actual_call: Arc<dyn LlmCallFn>,
    ) -> Result<Message, MiddlewareError> {
        // Fold from innermost (index = len) outward to index 0.
        let mut next: Arc<dyn LlmCallFn> = actual_call;
        for index in (0..self.layers.len()).rev() {
            let inner = next;
            next = Arc::new(ChainedLlmCall {
                stack: Arc::new(self.clone()),
                inner,
                index,
            });
        }
        // `next` is now the outermost wrapper; call it with the innermost being
        // `actual_call` that every layer reverts to once its own wrap runs.
        next.call(state, messages)
            .await
            .map_err(MiddlewareError::from)
    }

    /// Run `wrap_tool_call` through the full middleware chain.
    ///
    /// Same owned-fold construction as [`run_wrap_llm_call`].
    pub async fn run_wrap_tool_call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
        actual_call: Arc<dyn ToolCallFn>,
    ) -> Result<ToolOutput, MiddlewareError> {
        let mut next: Arc<dyn ToolCallFn> = actual_call;
        for index in (0..self.layers.len()).rev() {
            let inner = next;
            next = Arc::new(ChainedToolCall {
                stack: Arc::new(self.clone()),
                inner,
                index,
            });
        }
        next.call(state, tool_call)
            .await
            .map_err(MiddlewareError::from)
    }
}

// ---------------------------------------------------------------------------
// Helper structs for chained wrap calls
// ---------------------------------------------------------------------------

/// Wraps the next middleware layer as an owned `LlmCallFn`. Holds an owned
/// `Arc` to the stack and to its inner layer, so the onion can be folded
/// without borrowing and without function recursion.
///
/// State is NOT held here — it is passed through `call(state, messages)`.
struct ChainedLlmCall {
    stack: Arc<MiddlewareStack>,
    inner: Arc<dyn LlmCallFn>,
    index: usize,
}

#[async_trait]
impl LlmCallFn for ChainedLlmCall {
    async fn call(
        &self,
        state: &AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError> {
        self.stack.layers[self.index]
            .wrap_llm_call(state, messages, self.inner.as_ref())
            .await
            .map_err(MiddlewareError::into_agent_error)
    }
}

/// Owned `ToolCallFn` wrapper for the next tool middleware layer, mirroring
/// [`ChainedLlmCall`].
struct ChainedToolCall {
    stack: Arc<MiddlewareStack>,
    inner: Arc<dyn ToolCallFn>,
    index: usize,
}

#[async_trait]
impl ToolCallFn for ChainedToolCall {
    async fn call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError> {
        self.stack.layers[self.index]
            .wrap_tool_call(state, tool_call, self.inner.as_ref())
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
