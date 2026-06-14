//! HooksPlugin — runs shell-script hooks as middleware at PreToolUse,
//! PostToolUse, SessionStart, and SessionEnd events.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use alva_kernel_core::middleware::Middleware;
use alva_kernel_abi::ToolCall;
use alva_kernel_abi::tool::execution::ToolOutput;

use crate::extension::{Plugin, Registrar};
use crate::extension::hooks::{HookEvent, HookExecutor, HookInput};
use crate::settings::HooksSettings;

/// Lifecycle hooks as middleware — runs shell scripts at PreToolUse, PostToolUse,
/// SessionStart, and SessionEnd events.
pub struct HooksPlugin {
    settings: HooksSettings,
}

impl HooksPlugin {
    pub fn new(settings: HooksSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl Plugin for HooksPlugin {
    fn name(&self) -> &str { "hooks" }
    fn description(&self) -> &str { "Lifecycle hooks (shell scripts at tool/session events)" }

    async fn register(&self, r: &Registrar) {
        r.middleware(Arc::new(HooksMiddleware {
            settings: self.settings.clone(),
            workspace: OnceLock::new(),
        }));
    }
}

/// Internal middleware that delegates to HookExecutor.
struct HooksMiddleware {
    settings: HooksSettings,
    workspace: OnceLock<PathBuf>,
}

#[async_trait]
impl Middleware for HooksMiddleware {
    fn name(&self) -> &str { "hooks" }

    fn configure(&self, ctx: &alva_kernel_core::middleware::MiddlewareContext) {
        if let Some(ref ws) = ctx.workspace {
            let _ = self.workspace.set(ws.clone());
        }
    }

    fn priority(&self) -> i32 {
        // Run after security but before guardrails and most other middleware
        alva_kernel_core::shared::MiddlewarePriority::HOOKS
    }

    async fn on_agent_start(&self, state: &mut alva_kernel_core::state::AgentState) -> Result<(), alva_kernel_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let session_id = state.session.session_id();
            let executor = HookExecutor::new(ws, session_id);
            let input = HookInput::lifecycle(HookEvent::SessionStart, session_id, ws);
            let result = executor.run(&self.settings, HookEvent::SessionStart, None, input).await;
            if result.is_blocked() {
                return Err(alva_kernel_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        state: &mut alva_kernel_core::state::AgentState,
        _error: Option<&str>,
    ) -> Result<(), alva_kernel_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let session_id = state.session.session_id();
            let executor = HookExecutor::new(ws, session_id);
            let input = HookInput::lifecycle(HookEvent::SessionEnd, session_id, ws);
            let _ = executor.run(&self.settings, HookEvent::SessionEnd, None, input).await;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        state: &mut alva_kernel_core::state::AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), alva_kernel_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let session_id = state.session.session_id();
            let executor = HookExecutor::new(ws, session_id);
            let input = HookInput::pre_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                session_id,
                ws,
            );
            let result = executor.run(
                &self.settings,
                HookEvent::PreToolUse,
                Some(&tool_call.name),
                input,
            ).await;
            if result.is_blocked() {
                return Err(alva_kernel_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        state: &mut alva_kernel_core::state::AgentState,
        tool_call: &ToolCall,
        tool_output: &mut ToolOutput,
    ) -> Result<(), alva_kernel_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let session_id = state.session.session_id();
            let executor = HookExecutor::new(ws, session_id);
            let response_text = tool_output.model_text();
            let input = HookInput::post_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                &response_text,
                session_id,
                ws,
            );
            let _ = executor.run(
                &self.settings,
                HookEvent::PostToolUse,
                Some(&tool_call.name),
                input,
            ).await;
        }
        Ok(())
    }
}
