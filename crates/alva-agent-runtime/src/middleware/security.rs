// INPUT:  alva_agent_core::middleware, alva_agent_security, alva_types, async_trait, tokio::sync::Mutex
// OUTPUT: SecurityMiddleware
// POS:    Wraps SecurityGuard as V2 async Middleware — blocks tool calls on Deny/NeedHumanApproval.

use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareError};
use alva_agent_core::state::AgentState;
use alva_agent_core::shared::MiddlewarePriority;
use alva_agent_security::{SandboxMode, SecurityDecision, SecurityGuard};
use alva_types::{MinimalExecutionContext, ToolCall};
use async_trait::async_trait;
use tokio::sync::Mutex;

/// Sent through the approval channel when a tool needs human approval.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// Wrapper type stored in Extensions — holds the sending half of the approval channel.
/// The CLI/UI receives ApprovalRequest on the other end of the channel.
#[derive(Clone)]
pub struct ApprovalNotifier {
    pub tx: tokio::sync::mpsc::UnboundedSender<ApprovalRequest>,
}

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

    /// Get a shared reference to the underlying SecurityGuard.
    /// Used by BaseAgent to resolve approvals from the UI layer.
    pub fn guard(&self) -> Arc<Mutex<SecurityGuard>> {
        self.guard.clone()
    }
}

#[async_trait]
impl Middleware for SecurityMiddleware {
    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        let tool_context = MinimalExecutionContext::new();

        // Lock guard, check, take receiver if needed, then drop lock BEFORE awaiting
        let (decision, pending_rx) = {
            let mut guard = self.guard.lock().await;
            let decision =
                guard.check_tool_call(&tool_call.name, &tool_call.arguments, &tool_context);
            let rx = if let SecurityDecision::NeedHumanApproval { ref request_id } = decision {
                guard.take_pending_receiver(request_id)
            } else {
                None
            };
            (decision, rx)
        };
        // Guard lock is dropped here — critical for avoiding deadlock during approval

        match decision {
            SecurityDecision::Allow => Ok(()),
            SecurityDecision::Deny { reason } => Err(MiddlewareError::Blocked { reason }),
            SecurityDecision::NeedHumanApproval { request_id } => {
                // Try to get the approval notifier from Extensions
                let notifier = state.extensions.get::<ApprovalNotifier>().cloned();

                match (notifier, pending_rx) {
                    (Some(notifier), Some(rx)) => {
                        // Notify UI that approval is needed
                        let _ = notifier.tx.send(ApprovalRequest {
                            request_id: request_id.clone(),
                            tool_name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                        });

                        // Wait for human decision
                        match rx.await {
                            Ok(perm) => {
                                use alva_agent_security::PermissionDecision;
                                match perm {
                                    PermissionDecision::AllowOnce
                                    | PermissionDecision::AllowAlways => Ok(()),
                                    PermissionDecision::RejectOnce
                                    | PermissionDecision::RejectAlways => {
                                        Err(MiddlewareError::Blocked {
                                            reason: format!(
                                                "tool '{}' denied by user",
                                                tool_call.name
                                            ),
                                        })
                                    }
                                }
                            }
                            Err(_) => Err(MiddlewareError::Blocked {
                                reason: format!(
                                    "approval for '{}' timed out or cancelled",
                                    tool_call.name
                                ),
                            }),
                        }
                    }
                    _ => {
                        // No approval handler configured — fall back to blocking
                        Err(MiddlewareError::Blocked {
                            reason: format!(
                                "tool '{}' requires human approval (request: {}) but no approval handler is configured",
                                tool_call.name, request_id
                            ),
                        })
                    }
                }
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
    use alva_agent_core::shared::Extensions;
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
    async fn middleware_exposes_guard() {
        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let guard = mw.guard();
        // Verify we can lock and access
        let _g = guard.lock().await;
        // Smoke test passes — guard is accessible
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

    #[tokio::test]
    async fn middleware_waits_for_approval_and_allows() {
        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let guard = mw.guard();
        let mut state = make_state();

        // Set up approval channel in Extensions
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        state
            .extensions
            .insert(ApprovalNotifier { tx: approval_tx });

        let tc = ToolCall {
            id: "1".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls /projects/test" }),
        };

        // Spawn a task that will resolve the approval
        let mw_guard = guard.clone();
        let approval_handle = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            assert_eq!(req.tool_name, "execute_shell");
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                alva_agent_security::PermissionDecision::AllowOnce,
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approval_handle.await.unwrap();
        assert!(
            result.is_ok(),
            "should be allowed after approval: {:?}",
            result
        );
    }

    #[tokio::test]
    async fn middleware_waits_for_approval_and_denies() {
        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
        let guard = mw.guard();
        let mut state = make_state();

        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        state
            .extensions
            .insert(ApprovalNotifier { tx: approval_tx });

        let tc = ToolCall {
            id: "2".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "rm -rf /projects/test" }),
        };

        let mw_guard = guard.clone();
        let approval_handle = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                alva_agent_security::PermissionDecision::RejectOnce,
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approval_handle.await.unwrap();
        assert!(result.is_err(), "should be denied after rejection");
    }
}
