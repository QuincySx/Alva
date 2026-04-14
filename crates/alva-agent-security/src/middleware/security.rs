// INPUT:  alva_kernel_core::middleware, crate::{SecurityGuard, SandboxMode, SecurityDecision, PermissionDecision}, alva_kernel_abi::{BusHandle, ToolCall}, async_trait, tokio::sync::Mutex
// OUTPUT: SecurityMiddleware, ApprovalRequest, ApprovalNotifier
// POS:    Wraps SecurityGuard as async Middleware — reads ApprovalNotifier from bus to route interactive permission prompts.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use alva_kernel_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_kernel_core::state::AgentState;
use alva_kernel_core::shared::MiddlewarePriority;
use crate::{SandboxMode, SecurityDecision, SecurityGuard};
use alva_kernel_abi::{BusHandle, CancellationToken, MinimalExecutionContext, ToolCall};
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
    bus: OnceLock<BusHandle>,
    approval_timeout: Duration,
}

impl SecurityMiddleware {
    pub fn new(guard: SecurityGuard) -> Self {
        Self {
            guard: Arc::new(Mutex::new(guard)),
            bus: OnceLock::new(),
            approval_timeout: Duration::from_secs(300),
        }
    }

    pub fn for_workspace(workspace: impl Into<std::path::PathBuf>, mode: SandboxMode) -> Self {
        Self::new(SecurityGuard::new(workspace.into(), mode))
    }

    /// Attach a bus handle so the middleware can look up capabilities (e.g. ApprovalNotifier).
    ///
    /// Can also be wired lazily by `Middleware::configure()`; the first
    /// caller wins (subsequent calls are ignored).
    pub fn with_bus(self, bus: BusHandle) -> Self {
        let _ = self.bus.set(bus);
        self
    }

    pub fn with_approval_timeout(mut self, timeout: Duration) -> Self {
        self.approval_timeout = timeout;
        self
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
                // Try to get the approval notifier from the bus
                let notifier = self.bus.get()
                    .and_then(|b| b.get::<ApprovalNotifier>())
                    .map(|arc| (*arc).clone());

                match (notifier, pending_rx) {
                    (Some(notifier), Some(rx)) => {
                        // Notify UI that approval is needed
                        if notifier.tx.send(ApprovalRequest {
                            request_id: request_id.clone(),
                            tool_name: tool_call.name.clone(),
                            arguments: tool_call.arguments.clone(),
                        }).is_err() {
                            let mut guard = self.guard.lock().await;
                            guard.cancel_permission(&request_id);
                            return Err(MiddlewareError::Blocked {
                                reason: format!(
                                    "approval handler for '{}' disconnected before the request could be delivered",
                                    tool_call.name
                                ),
                            });
                        }

                        enum ApprovalWaitOutcome {
                            Decision(Result<crate::PermissionDecision, tokio::sync::oneshot::error::RecvError>),
                            Cancelled,
                            TimedOut,
                        }

                        let cancel = state.extensions.get::<CancellationToken>().cloned();
                        let timeout = tokio::time::sleep(self.approval_timeout);
                        tokio::pin!(timeout);

                        let wait_outcome = if let Some(mut cancel) = cancel {
                            tokio::select! {
                                result = rx => ApprovalWaitOutcome::Decision(result),
                                _ = cancel.cancelled() => ApprovalWaitOutcome::Cancelled,
                                _ = &mut timeout => ApprovalWaitOutcome::TimedOut,
                            }
                        } else {
                            tokio::select! {
                                result = rx => ApprovalWaitOutcome::Decision(result),
                                _ = &mut timeout => ApprovalWaitOutcome::TimedOut,
                            }
                        };

                        match wait_outcome {
                            ApprovalWaitOutcome::Decision(Ok(perm)) => {
                                use crate::PermissionDecision;
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
                            ApprovalWaitOutcome::Decision(Err(_)) => Err(MiddlewareError::Blocked {
                                reason: format!(
                                    "approval for '{}' timed out or cancelled",
                                    tool_call.name
                                ),
                            }),
                            ApprovalWaitOutcome::Cancelled => {
                                let mut guard = self.guard.lock().await;
                                guard.cancel_permission(&request_id);
                                Err(MiddlewareError::Blocked {
                                    reason: format!(
                                        "approval for '{}' was cancelled because the run was cancelled",
                                        tool_call.name
                                    ),
                                })
                            }
                            ApprovalWaitOutcome::TimedOut => {
                                let mut guard = self.guard.lock().await;
                                guard.cancel_permission(&request_id);
                                Err(MiddlewareError::Blocked {
                                    reason: format!(
                                        "approval for '{}' timed out",
                                        tool_call.name
                                    ),
                                })
                            }
                        }
                    }
                    _ => {
                        let mut guard = self.guard.lock().await;
                        guard.cancel_permission(&request_id);
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

    fn configure(&self, ctx: &MiddlewareContext) {
        if let Some(bus) = &ctx.bus {
            let _ = self.bus.set(bus.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_core::shared::Extensions;
    use alva_kernel_abi::agent_session::InMemoryAgentSession;
    use alva_kernel_abi::Bus;

    fn make_state() -> AgentState {
        use alva_kernel_abi::base::error::AgentError;
        use alva_kernel_abi::base::message::Message;
        use alva_kernel_abi::base::stream::StreamEvent;
        use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
        use alva_kernel_abi::tool::Tool;
        use alva_kernel_abi::ModelConfig;

        struct StubModel;
        #[async_trait]
        impl LanguageModel for StubModel {
            async fn complete(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> Result<CompletionResponse, AgentError> {
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
            session: Arc::new(InMemoryAgentSession::new()),
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
        // Set up approval channel on the bus
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
                .with_bus(bus_handle);
        let guard = mw.guard();
        let mut state = make_state();

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
                crate::PermissionDecision::AllowOnce,
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
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
                .with_bus(bus_handle);
        let guard = mw.guard();
        let mut state = make_state();

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
                crate::PermissionDecision::RejectOnce,
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approval_handle.await.unwrap();
        assert!(result.is_err(), "should be denied after rejection");
    }

    #[tokio::test]
    async fn middleware_cancellation_interrupts_pending_approval() {
        let (approval_tx, _approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
                .with_bus(bus_handle);
        let mut state = make_state();
        let cancel = alva_kernel_abi::CancellationToken::new();
        cancel.cancel();
        state.extensions.insert(cancel.clone());

        let tc = ToolCall {
            id: "3".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls /projects/test" }),
        };

        let result = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            mw.before_tool_call(&mut state, &tc),
        )
        .await;

        assert!(
            result.is_ok(),
            "pending approval should stop waiting once the run is cancelled"
        );
        assert!(result.unwrap().is_err());
    }

    #[tokio::test]
    async fn middleware_times_out_pending_approval() {
        let (approval_tx, _approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw =
            SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
                .with_bus(bus_handle)
                .with_approval_timeout(std::time::Duration::from_millis(20));
        let mut state = make_state();

        let tc = ToolCall {
            id: "4".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls /projects/test" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_err(), "timed out approvals should be blocked");
    }
}
