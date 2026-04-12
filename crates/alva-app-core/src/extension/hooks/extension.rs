//! HooksExtension — runs shell-script hooks as middleware at PreToolUse,
//! PostToolUse, SessionStart, and SessionEnd events.

use std::path::PathBuf;
use std::sync::{Arc, OnceLock};

use async_trait::async_trait;

use alva_agent_core::middleware::Middleware;
use alva_types::ToolCall;
use alva_types::tool::execution::ToolOutput;

use crate::extension::{Extension, HostAPI};
use crate::extension::hooks::{HookEvent, HookExecutor, HookInput};
use crate::settings::HooksSettings;

/// Lifecycle hooks as middleware — runs shell scripts at PreToolUse, PostToolUse,
/// SessionStart, and SessionEnd events.
pub struct HooksExtension {
    settings: HooksSettings,
}

impl HooksExtension {
    pub fn new(settings: HooksSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl Extension for HooksExtension {
    fn name(&self) -> &str { "hooks" }
    fn description(&self) -> &str { "Lifecycle hooks (shell scripts at tool/session events)" }

    fn activate(&self, api: &HostAPI) {
        api.middleware(Arc::new(HooksMiddleware {
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

    fn configure(&self, ctx: &alva_agent_core::middleware::MiddlewareContext) {
        if let Some(ref ws) = ctx.workspace {
            let _ = self.workspace.set(ws.clone());
        }
    }

    fn priority(&self) -> i32 {
        // Run after security but before guardrails and most other middleware
        alva_agent_core::shared::MiddlewarePriority::HOOKS
    }

    async fn on_agent_start(&self, _state: &mut alva_agent_core::state::AgentState) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session"); // TODO: real session_id
            let input = HookInput::lifecycle(HookEvent::SessionStart, "session", ws);
            let result = executor.run(&self.settings, HookEvent::SessionStart, None, input).await;
            if result.is_blocked() {
                return Err(alva_agent_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        _error: Option<&str>,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let input = HookInput::lifecycle(HookEvent::SessionEnd, "session", ws);
            let _ = executor.run(&self.settings, HookEvent::SessionEnd, None, input).await;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let input = HookInput::pre_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                "session",
                ws,
            );
            let result = executor.run(
                &self.settings,
                HookEvent::PreToolUse,
                Some(&tool_call.name),
                input,
            ).await;
            if result.is_blocked() {
                return Err(alva_agent_core::shared::MiddlewareError::Blocked {
                    reason: result.blocking_messages().join("; "),
                });
            }
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        _state: &mut alva_agent_core::state::AgentState,
        tool_call: &ToolCall,
        tool_output: &mut ToolOutput,
    ) -> Result<(), alva_agent_core::shared::MiddlewareError> {
        if let Some(ws) = self.workspace.get() {
            let executor = HookExecutor::new(ws, "session");
            let response_text = tool_output.model_text();
            let input = HookInput::post_tool_use(
                &tool_call.name,
                tool_call.arguments.clone(),
                &response_text,
                "session",
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
