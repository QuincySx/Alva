// INPUT:  std::time::Duration, crate::middleware, crate::state::AgentState, alva_types
// OUTPUT: ToolTimeoutMiddleware
// POS:    Middleware that wraps tool execution with a configurable timeout.

use std::time::Duration;

use alva_types::{ToolCall, ToolOutput};
use async_trait::async_trait;

use crate::middleware::{Middleware, MiddlewareError, ToolCallFn};
use crate::shared::MiddlewarePriority;
use crate::state::AgentState;

/// Middleware that enforces a timeout on tool execution.
///
/// If a tool call does not complete within the configured duration,
/// it returns an error result to the LLM instead of blocking forever.
///
/// ```rust,ignore
/// let timeout_mw = ToolTimeoutMiddleware::new(Duration::from_secs(120));
/// middleware_stack.push_sorted(Arc::new(timeout_mw));
/// ```
pub struct ToolTimeoutMiddleware {
    timeout: Duration,
}

impl ToolTimeoutMiddleware {
    pub fn new(timeout: Duration) -> Self {
        Self { timeout }
    }
}

impl Default for ToolTimeoutMiddleware {
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
        }
    }
}

#[async_trait]
impl Middleware for ToolTimeoutMiddleware {
    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        match tokio::time::timeout(self.timeout, next.call(state, tool_call)).await {
            Ok(result) => result.map_err(|e| MiddlewareError::Other(e.to_string())),
            Err(_) => Ok(ToolOutput::error(format!(
                "Tool '{}' timed out after {:?}. Consider breaking the task into smaller steps.",
                tool_call.name, self.timeout
            ))),
        }
    }

    fn name(&self) -> &str {
        "tool_timeout"
    }

    fn priority(&self) -> i32 {
        // Run early in the chain — timeout wraps everything inside it.
        MiddlewarePriority::DEFAULT - 100
    }
}
