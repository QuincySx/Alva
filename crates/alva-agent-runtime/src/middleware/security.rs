// INPUT:  alva_agent_core::v2::middleware, alva_agent_security, alva_types, async_trait, tokio::sync::Mutex
// OUTPUT: SecurityMiddleware
// POS:    Wraps SecurityGuard as V2 async Middleware — blocks tool calls on Deny/NeedHumanApproval.

use std::sync::Arc;

use alva_agent_core::v2::middleware::{Middleware, MiddlewareError};
use alva_agent_core::v2::state::AgentState;
use alva_agent_core::middleware::MiddlewarePriority;
use alva_agent_security::{SandboxMode, SecurityDecision, SecurityGuard};
use alva_types::{EmptyToolContext, ToolCall};
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
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        let mut guard = self.guard.lock().await;
        let tool_context = EmptyToolContext;
        match guard.check_tool_call(&tool_call.name, &tool_call.arguments, &tool_context) {
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

    fn priority(&self) -> i32 {
        MiddlewarePriority::SECURITY
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_agent_core::middleware::Extensions;
    use alva_types::session::InMemorySession;

    fn make_state() -> AgentState {
        use alva_types::base::error::AgentError;
        use alva_types::base::message::Message;
        use alva_types::base::stream::StreamEvent;
        use alva_types::model::LanguageModel;
        use alva_types::tool::Tool;
        use alva_types::ModelConfig;

        struct StubModel;
        #[async_trait]
        impl LanguageModel for StubModel {
            async fn complete(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> Result<Message, AgentError> {
                unreachable!()
            }
            fn stream(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::empty())
            }
            fn model_id(&self) -> &str {
                "stub"
            }
        }

        AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        }
    }

    #[tokio::test]
    async fn blocks_outside_workspace_path() {
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let mut state = make_state();
        let tc = ToolCall {
            id: "1".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/etc/passwd" }),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn allows_workspace_path() {
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let mut state = make_state();
        let tc = ToolCall {
            id: "2".into(),
            name: "read_file".into(),
            arguments: serde_json::json!({ "path": "/projects/test/src/main.rs" }),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok());
    }
}
