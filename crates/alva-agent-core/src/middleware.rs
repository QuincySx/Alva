// INPUT:  std::sync::Arc, async_trait, alva_types::{AgentError, Message, ToolCall, ToolResult}, crate::state::AgentState
// OUTPUT: LlmCallFn, ToolCallFn, Middleware (trait), MiddlewareStack
// POS:    Middleware trait and stack — receives &mut AgentState directly, with wrap hooks for interceptor pattern.

use std::sync::Arc;

use alva_types::{AgentError, Message, ToolCall, ToolResult};
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
    async fn call(&self, messages: Vec<Message>) -> Result<Message, AgentError>;
}

/// Callback for the "next" step in the tool wrapping chain.
#[async_trait]
pub trait ToolCallFn: Send + Sync {
    async fn call(&self, tool_call: &ToolCall) -> Result<ToolResult, AgentError>;
}

// ---------------------------------------------------------------------------
// Middleware trait
// ---------------------------------------------------------------------------

/// V2 Middleware trait — receives `&mut AgentState` directly.
///
/// Key differences from v1:
/// - `on_agent_start` / `on_agent_end` lifecycle hooks
/// - `before_*` / `after_*` inspection hooks receive `&mut AgentState`
/// - `wrap_*` interceptor hooks receive `&AgentState` (immutable, since the next callback owns the mutable flow)
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
        _state: &AgentState,
        messages: Vec<Message>,
        next: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        next.call(messages)
            .await
            .map_err(|e| MiddlewareError::Other(e.to_string()))
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
        _result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    /// Wrap a single tool execution — can modify args, result, retry, or skip.
    /// Default: just calls `next`.
    async fn wrap_tool_call(
        &self,
        _state: &AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolResult, MiddlewareError> {
        next.call(tool_call)
            .await
            .map_err(|e| MiddlewareError::Other(e.to_string()))
    }

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

    /// Iterate over all middleware layers in order.
    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Middleware>> {
        self.layers.iter()
    }

    // -- lifecycle hooks ---------------------------------------------------

    /// Run `on_agent_start` top-to-bottom.
    pub async fn run_on_agent_start(
        &self,
        state: &mut AgentState,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.on_agent_start(state).await?;
        }
        Ok(())
    }

    /// Run `on_agent_end` bottom-to-top.
    pub async fn run_on_agent_end(
        &self,
        state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            layer.on_agent_end(state, error).await?;
        }
        Ok(())
    }

    // -- before hooks: top-to-bottom ---------------------------------------

    /// Run `before_llm_call` top-to-bottom.
    pub async fn run_before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        for layer in &self.layers {
            layer.before_llm_call(state, messages).await?;
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
            layer.before_tool_call(state, tool_call).await?;
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
            layer.after_llm_call(state, response).await?;
        }
        Ok(())
    }

    /// Run `after_tool_call` bottom-to-top.
    pub async fn run_after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolResult,
    ) -> Result<(), MiddlewareError> {
        for layer in self.layers.iter().rev() {
            layer.after_tool_call(state, tool_call, result).await?;
        }
        Ok(())
    }

    // -- wrap hooks -----------------------------------------------------------

    /// Run `wrap_llm_call` — delegates to the first middleware layer's wrap,
    /// which by default calls `next` (the actual LLM call).
    ///
    /// TODO: proper nesting for multiple wrapping middleware (chain of `next` callbacks).
    pub async fn run_wrap_llm_call(
        &self,
        state: &AgentState,
        messages: Vec<Message>,
        actual_call: &dyn LlmCallFn,
    ) -> Result<Message, MiddlewareError> {
        if let Some(first) = self.layers.first() {
            first.wrap_llm_call(state, messages, actual_call).await
        } else {
            actual_call
                .call(messages)
                .await
                .map_err(|e| MiddlewareError::Other(e.to_string()))
        }
    }

    /// Run `wrap_tool_call` — delegates to the first middleware layer's wrap,
    /// which by default calls `next` (the actual tool execution).
    ///
    /// TODO: proper nesting for multiple wrapping middleware (chain of `next` callbacks).
    pub async fn run_wrap_tool_call(
        &self,
        state: &AgentState,
        tool_call: &ToolCall,
        actual_call: &dyn ToolCallFn,
    ) -> Result<ToolResult, MiddlewareError> {
        if let Some(first) = self.layers.first() {
            first.wrap_tool_call(state, tool_call, actual_call).await
        } else {
            actual_call
                .call(tool_call)
                .await
                .map_err(|e| MiddlewareError::Other(e.to_string()))
        }
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
