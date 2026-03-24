// INPUT:  alva_agent_core::middleware, alva_agent_security, alva_types, async_trait, tokio::sync::Mutex
// OUTPUT: SecurityMiddleware
// POS:    Wraps SecurityGuard as async Middleware — blocks tool calls on Deny/NeedHumanApproval.

use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_agent_security::{SandboxMode, SecurityDecision, SecurityGuard};
use alva_types::{ToolCall, ToolContext};
use async_trait::async_trait;
use tokio::sync::Mutex;

pub struct SecurityMiddleware {
    guard: Arc<Mutex<SecurityGuard>>,
}

impl SecurityMiddleware {
    pub fn new(guard: SecurityGuard) -> Self {
        Self {
            guard: Arc::new(Mutex::new(guard)),
        }
    }

    pub fn for_workspace(workspace: impl Into<std::path::PathBuf>, mode: SandboxMode) -> Self {
        Self::new(SecurityGuard::new(workspace.into(), mode))
    }
}

#[async_trait]
impl Middleware for SecurityMiddleware {
    async fn before_tool_call(
        &self,
        _ctx: &mut MiddlewareContext,
        tool_call: &ToolCall,
        tool_context: &dyn ToolContext,
    ) -> Result<(), MiddlewareError> {
        let mut guard = self.guard.lock().await;
        match guard.check_tool_call(&tool_call.name, &tool_call.arguments, tool_context) {
            SecurityDecision::Allow => Ok(()),
            SecurityDecision::Deny { reason } => Err(MiddlewareError::Blocked { reason }),
            SecurityDecision::NeedHumanApproval { request_id } => {
                Err(MiddlewareError::Blocked {
                    reason: format!(
                        "tool '{}' requires human approval (request: {})",
                        tool_call.name, request_id
                    ),
                })
            }
        }
    }

    fn name(&self) -> &str {
        "security"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_agent_core::middleware::Extensions;

    #[tokio::test]
    async fn blocks_outside_workspace_path() {
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let tc = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/etc/passwd" }),
        };
        let tool_ctx = alva_types::EmptyToolContext;
        let result = mw.before_tool_call(&mut ctx, &tc, &tool_ctx).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn allows_workspace_path() {
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: String::new(),
            messages: vec![],
            extensions: Extensions::new(),
        };
        let tc = ToolCall {
            id: "2".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/projects/test/src/main.rs" }),
        };
        let tool_ctx = alva_types::EmptyToolContext;
        let result = mw.before_tool_call(&mut ctx, &tc, &tool_ctx).await;
        assert!(result.is_ok());
    }
}
