// INPUT:  std::sync::Arc, std::time::Duration, alva_kernel_abi::{Sleeper, NoopSleeper, timeout, ToolCall, ToolOutput}, crate::middleware, crate::state::AgentState
// OUTPUT: ToolTimeoutMiddleware
// POS:    Middleware that wraps tool execution with a configurable timeout, runtime-agnostic via Sleeper.

use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::{NoopSleeper, Sleeper, ToolCall, ToolOutput};
use async_trait::async_trait;

use crate::middleware::{Middleware, MiddlewareError, ToolCallFn};
use crate::shared::MiddlewarePriority;
use crate::state::AgentState;

/// Middleware that enforces a timeout on tool execution.
///
/// If a tool call does not complete within the configured duration,
/// it returns an error result to the LLM instead of blocking forever.
///
/// The middleware is runtime-agnostic: it delegates the actual wait
/// to an injected `Arc<dyn Sleeper>`. Construct with [`with_sleeper`]
/// (production) or [`Default::default`] (no real timeout — useful in
/// tests where no runtime is available).
///
/// ```rust,ignore
/// let timeout_mw = ToolTimeoutMiddleware::with_sleeper(
///     Duration::from_secs(120),
///     Arc::new(TokioSleeper),
/// );
/// ```
pub struct ToolTimeoutMiddleware {
    timeout: Duration,
    sleeper: Arc<dyn Sleeper>,
}

impl ToolTimeoutMiddleware {
    /// Construct with an explicit sleeper. This is the production path —
    /// host装配层 should pass a real sleeper (e.g., `TokioSleeper`).
    pub fn with_sleeper(timeout: Duration, sleeper: Arc<dyn Sleeper>) -> Self {
        Self { timeout, sleeper }
    }
}

impl Default for ToolTimeoutMiddleware {
    /// Default constructor uses `NoopSleeper`, which means **no timeout
    /// is actually enforced** — the user future always wins. This keeps
    /// the kernel runtime-independent and unit tests building. For real
    /// timeout enforcement, use [`with_sleeper`].
    fn default() -> Self {
        Self {
            timeout: Duration::from_secs(120),
            sleeper: Arc::new(NoopSleeper),
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
        // Honor the `Tool::manages_own_timeout` contract: tools that opt in
        // (e.g. sub-agent spawning, long-running stream tools) bound their
        // own runtime, so wrapping them again with the generic timeout
        // would only ever shrink their budget.
        let self_managed = state
            .tools
            .iter()
            .find(|t| t.name() == tool_call.name)
            .is_some_and(|t| t.manages_own_timeout());
        if self_managed {
            return next.call(state, tool_call).await.map_err(MiddlewareError::from);
        }

        match alva_kernel_abi::timeout(
            self.sleeper.as_ref(),
            self.timeout,
            next.call(state, tool_call),
        )
        .await
        {
            Ok(result) => result.map_err(MiddlewareError::from),
            Err(_) => Ok(ToolOutput::error(format!(
                "Tool '{}' timed out after {:?}. Consider breaking the task into smaller steps.",
                tool_call.name, self.timeout
            ))),
        }
    }

    fn name(&self) -> &str {
        "builtins_tool_timeout"
    }

    fn priority(&self) -> i32 {
        // Run early in the chain — timeout wraps everything inside it.
        MiddlewarePriority::DEFAULT - 100
    }
}
