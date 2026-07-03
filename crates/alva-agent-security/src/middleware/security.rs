// INPUT:  alva_kernel_core::middleware, crate::{SecurityGuard, SandboxMode, SecurityDecision, PermissionDecision, pending_actions::*}, alva_kernel_abi::{BusHandle, ToolCall, agent_session::*}, async_trait, tokio::sync::Mutex
// OUTPUT: SecurityMiddleware, ApprovalRequest, ApprovalNotifier
// POS:    Wraps SecurityGuard as async Middleware — reads ApprovalNotifier from bus to route
//         interactive permission prompts. Mirrors every approval request/resolution into the
//         session event log (requires_action / requires_action_resolved) so subscribers see
//         pending HITL state through the same stream that carries everything else.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::pending_actions::{
    ResolveStatus, EVENT_REQUIRES_ACTION, EVENT_REQUIRES_ACTION_RESOLVED,
};
use crate::{SandboxMode, SecurityDecision, SecurityGuard};
use alva_kernel_abi::agent_session::{AgentSession, EmitterKind, EventEmitter, SessionEvent};
use alva_kernel_abi::{bus_cap, BusHandle, CancellationToken, MinimalExecutionContext, ToolCall};
use alva_kernel_core::middleware::{Middleware, MiddlewareContext, MiddlewareError};
use alva_kernel_core::shared::MiddlewarePriority;
use alva_kernel_core::state::AgentState;
use async_trait::async_trait;
use tokio::sync::Mutex;

/// Emitter stamped onto every session event written by this middleware.
fn security_emitter() -> EventEmitter {
    EventEmitter {
        kind: EmitterKind::Middleware,
        id: "security".to_string(),
        instance: None,
    }
}

/// Append a `requires_action` event to the session log, encoding the HITL
/// request payload that subscribers / `pending_actions()` will read back.
/// Returns the event uuid so the matching `requires_action_resolved` can
/// link back via `parent_uuid`.
async fn emit_requires_action(
    session: &Arc<dyn AgentSession>,
    request_id: &str,
    tool_call: &ToolCall,
) -> String {
    let mut event = SessionEvent::new_runtime(EVENT_REQUIRES_ACTION);
    event.emitter = security_emitter();
    event.data = Some(serde_json::json!({
        "action_type": "tool_confirmation",
        "request_id": request_id,
        "tool_name": tool_call.name,
        "tool_call_id": tool_call.id,
        "arguments": tool_call.arguments,
    }));
    let uuid = event.uuid.clone();
    session.append(event).await;
    uuid
}

/// Append the matching `requires_action_resolved` event so the pair shows up
/// as resolved to any subscriber and to `pending_actions()`.
async fn emit_requires_action_resolved(
    session: &Arc<dyn AgentSession>,
    parent_uuid: &str,
    request_id: &str,
    status: ResolveStatus,
) {
    let mut event = SessionEvent::new_runtime(EVENT_REQUIRES_ACTION_RESOLVED);
    event.emitter = security_emitter();
    event.parent_uuid = Some(parent_uuid.to_string());
    event.data = Some(serde_json::json!({
        "request_id": request_id,
        "status": status.as_str(),
    }));
    session.append(event).await;
}

/// Sent through the approval channel when a tool needs human approval.
#[derive(Debug, Clone)]
pub struct ApprovalRequest {
    pub request_id: String,
    pub tool_name: String,
    pub arguments: serde_json::Value,
}

/// Bus Capability: fire-and-forget channel that wakes the HITL UI when
/// a tool call needs human approval.
///
/// **Provider**: `ApprovalPlugin::register`
/// (`alva-app-core/src/extension/approval.rs`) for the opt-in flow.
/// The deprecated `AgentRuntimeBuilder::build` legacy path can also
/// install it when the standard agent stack is enabled.
/// **Consumers**: `SecurityMiddleware::before_tool_call` — looks up the
/// notifier and sends an `ApprovalRequest` into the channel, then
/// awaits a decision from the security guard.
/// **Why bus**: the sender lives in the outer app (owns the `mpsc`
/// receiver), the consumer is middleware in `alva-agent-security`.
/// Constructor injection would require every stack variant to thread
/// the notifier through; bus-based discovery keeps middleware opt-in.
#[bus_cap]
#[derive(Clone)]
pub struct ApprovalNotifier {
    pub tx: tokio::sync::mpsc::UnboundedSender<ApprovalRequest>,
}

pub struct SecurityMiddleware {
    guard: Arc<Mutex<SecurityGuard>>,
    bus: OnceLock<BusHandle>,
    approval_timeout: Duration,
    /// One-shot latch so the "no approval handler wired" warning is logged at
    /// most once per middleware, instead of on every approval-needing tool.
    no_handler_warned: std::sync::atomic::AtomicBool,
}

impl SecurityMiddleware {
    pub fn new(guard: SecurityGuard) -> Self {
        Self {
            guard: Arc::new(Mutex::new(guard)),
            bus: OnceLock::new(),
            approval_timeout: Duration::from_secs(300),
            no_handler_warned: std::sync::atomic::AtomicBool::new(false),
        }
    }

    pub fn for_workspace(workspace: impl Into<std::path::PathBuf>, mode: SandboxMode) -> Self {
        Self::new(SecurityGuard::new(workspace.into(), mode))
    }

    /// Build a middleware around an EXISTING shared guard (e.g. the one the
    /// parent agent published on the bus). Unlike [`Self::new`], decisions,
    /// mode flips, and allow-always grants stay in sync with every other
    /// middleware holding the same guard — this is how a sub-agent loop
    /// enforces the same HITL gate as its parent.
    pub fn from_shared(guard: Arc<Mutex<SecurityGuard>>) -> Self {
        Self {
            guard,
            bus: OnceLock::new(),
            approval_timeout: Duration::from_secs(300),
            no_handler_warned: std::sync::atomic::AtomicBool::new(false),
        }
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

        // Phase 1: existing dangerous-tool / sensitive-path check
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

        // Phase 2: URL-aware SSRF check, ONLY when Phase 1 said Allow.
        // We do not gate Deny / NeedHumanApproval through URL checks —
        // those already block / ask, and a URL secondary check would be
        // redundant noise. The async URL inspect is done outside the
        // guard lock to avoid holding it across DNS.
        let (decision, pending_rx) = match decision {
            SecurityDecision::Allow => {
                let url_decision = {
                    let mut guard = self.guard.lock().await;
                    guard
                        .check_url_in_tool_call(&tool_call.name, &tool_call.arguments)
                        .await
                };
                match url_decision {
                    Some(SecurityDecision::NeedHumanApproval { request_id }) => {
                        let rx = {
                            let mut guard = self.guard.lock().await;
                            guard.take_pending_receiver(&request_id)
                        };
                        (SecurityDecision::NeedHumanApproval { request_id }, rx)
                    }
                    Some(SecurityDecision::Deny { reason }) => {
                        (SecurityDecision::Deny { reason }, None)
                    }
                    Some(SecurityDecision::Allow) | None => (SecurityDecision::Allow, None),
                }
            }
            other => (other, pending_rx),
        };
        // Guard lock is dropped here — critical for avoiding deadlock during approval

        match decision {
            SecurityDecision::Allow => Ok(()),
            SecurityDecision::Deny { reason } => Err(MiddlewareError::Blocked { reason }),
            SecurityDecision::NeedHumanApproval { request_id } => {
                // Mirror the request into the session event log before any
                // side-channel notification fires. Subscribers see this event
                // appear synchronously with seq assignment; readers will find
                // it via `pending_actions()` until the matching resolved event
                // is appended.
                let action_uuid =
                    emit_requires_action(&state.session, &request_id, tool_call).await;

                // Try to get the approval notifier from the bus
                let notifier = self
                    .bus
                    .get()
                    .and_then(|b| b.get::<ApprovalNotifier>())
                    .map(|arc| (*arc).clone());

                match (notifier, pending_rx) {
                    (Some(notifier), Some(rx)) => {
                        // Notify UI that approval is needed
                        if notifier
                            .tx
                            .send(ApprovalRequest {
                                request_id: request_id.clone(),
                                tool_name: tool_call.name.clone(),
                                arguments: tool_call.arguments.clone(),
                            })
                            .is_err()
                        {
                            let mut guard = self.guard.lock().await;
                            guard.cancel_permission(&request_id);
                            drop(guard);
                            emit_requires_action_resolved(
                                &state.session,
                                &action_uuid,
                                &request_id,
                                ResolveStatus::Disconnected,
                            )
                            .await;
                            return Err(MiddlewareError::Blocked {
                                reason: format!(
                                    "approval handler for '{}' disconnected before the request could be delivered",
                                    tool_call.name
                                ),
                            });
                        }

                        enum ApprovalWaitOutcome {
                            Decision(
                                Result<
                                    crate::PermissionDecision,
                                    tokio::sync::oneshot::error::RecvError,
                                >,
                            ),
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
                                let status = match perm {
                                    PermissionDecision::AllowOnce => ResolveStatus::AllowOnce,
                                    PermissionDecision::AllowAlways => ResolveStatus::AllowAlways,
                                    PermissionDecision::RejectOnce => ResolveStatus::RejectOnce,
                                    PermissionDecision::RejectAlways => ResolveStatus::RejectAlways,
                                };
                                emit_requires_action_resolved(
                                    &state.session,
                                    &action_uuid,
                                    &request_id,
                                    status,
                                )
                                .await;
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
                            ApprovalWaitOutcome::Decision(Err(_)) => {
                                emit_requires_action_resolved(
                                    &state.session,
                                    &action_uuid,
                                    &request_id,
                                    ResolveStatus::Disconnected,
                                )
                                .await;
                                Err(MiddlewareError::Blocked {
                                    reason: format!(
                                        "approval for '{}' timed out or cancelled",
                                        tool_call.name
                                    ),
                                })
                            }
                            ApprovalWaitOutcome::Cancelled => {
                                let mut guard = self.guard.lock().await;
                                guard.cancel_permission(&request_id);
                                drop(guard);
                                emit_requires_action_resolved(
                                    &state.session,
                                    &action_uuid,
                                    &request_id,
                                    ResolveStatus::Cancelled,
                                )
                                .await;
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
                                drop(guard);
                                emit_requires_action_resolved(
                                    &state.session,
                                    &action_uuid,
                                    &request_id,
                                    ResolveStatus::TimedOut,
                                )
                                .await;
                                Err(MiddlewareError::Blocked {
                                    reason: format!("approval for '{}' timed out", tool_call.name),
                                })
                            }
                        }
                    }
                    _ => {
                        let mut guard = self.guard.lock().await;
                        guard.cancel_permission(&request_id);
                        drop(guard);
                        emit_requires_action_resolved(
                            &state.session,
                            &action_uuid,
                            &request_id,
                            ResolveStatus::NoHandler,
                        )
                        .await;
                        // No approval handler configured. The behavior is
                        // fail-closed (we block), which is safe — but a missing
                        // ApprovalNotifier means EVERY approval-needing tool
                        // will be blocked, which presents to the user as
                        // mysterious tool failures. Surface it once in the logs
                        // so the missing HITL wiring is diagnosable up front
                        // rather than discovered tool-by-tool.
                        if !self
                            .no_handler_warned
                            .swap(true, std::sync::atomic::Ordering::Relaxed)
                        {
                            tracing::warn!(
                                "tool requires human approval but no ApprovalNotifier is registered \
                                 on the bus; all approval-requiring tools will be blocked. Wire an \
                                 approval handler (provide ApprovalNotifier) or use a permission \
                                 mode that does not require approval."
                            );
                        }
                        // Fall back to blocking.
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
    use alva_kernel_abi::agent_session::InMemoryAgentSession;
    use alva_kernel_abi::Bus;
    use alva_kernel_core::shared::Extensions;

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
            ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>>
            {
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
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen);
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

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
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

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
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
        let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
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
        let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
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

    // -----------------------------------------------------------------------
    // requires_action event-log integration
    //
    // The middleware mirrors each HITL request through the session event log:
    //   - `requires_action` appended before the side-channel send
    //   - `requires_action_resolved` appended on each terminal outcome
    //     (decision / cancel / timeout / disconnected / no_handler)
    // Subscribers to `AgentSession::subscribe_events` therefore see the same
    // state through the same stream that carries everything else, and
    // `pending_actions()` is a O(n) projection over the log.
    // -----------------------------------------------------------------------

    use crate::pending_actions::{
        pending_actions, EVENT_REQUIRES_ACTION, EVENT_REQUIRES_ACTION_RESOLVED,
    };

    fn make_state_with_session(session: Arc<dyn AgentSession>) -> AgentState {
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
            ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>>
            {
                Box::pin(tokio_stream::empty())
            }
            fn model_id(&self) -> &str {
                "stub"
            }
        }

        AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session,
            extensions: Extensions::new(),
        }
    }

    #[tokio::test]
    async fn approval_emits_requires_action_into_session_log() {
        use alva_kernel_abi::agent_session::EventQuery;

        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus_handle);
        let guard = mw.guard();
        let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
        let mut state = make_state_with_session(session.clone());

        let tc = ToolCall {
            id: "call-7".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls /projects/test" }),
        };

        let mw_guard = guard.clone();
        let approver = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                crate::PermissionDecision::AllowOnce,
            );
        });

        mw.before_tool_call(&mut state, &tc).await.unwrap();
        approver.await.unwrap();

        // Pair appears in the log, in order.
        let events = session.query(&EventQuery::default()).await;
        let req_pos = events
            .iter()
            .position(|m| m.event.event_type == EVENT_REQUIRES_ACTION)
            .expect("requires_action present");
        let res_pos = events
            .iter()
            .position(|m| m.event.event_type == EVENT_REQUIRES_ACTION_RESOLVED)
            .expect("requires_action_resolved present");
        assert!(req_pos < res_pos, "request must precede resolution");

        // Resolved event parent_uuid links back to the request.
        let req_uuid = events[req_pos].event.uuid.clone();
        assert_eq!(
            events[res_pos].event.parent_uuid.as_deref(),
            Some(req_uuid.as_str())
        );

        // Carried payload matches the ToolCall.
        let req_data = events[req_pos].event.data.as_ref().unwrap();
        assert_eq!(req_data["action_type"], "tool_confirmation");
        assert_eq!(req_data["tool_name"], "execute_shell");
        assert_eq!(req_data["tool_call_id"], "call-7");
        assert_eq!(req_data["arguments"]["command"], "ls /projects/test");

        // Resolved status matches the decision.
        let res_data = events[res_pos].event.data.as_ref().unwrap();
        assert_eq!(res_data["status"], "allow_once");

        // pending_actions returns empty once resolved.
        assert!(pending_actions(&*session).await.is_empty());
    }

    #[tokio::test]
    async fn pending_actions_lists_in_flight_approval() {
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus_handle);
        let guard = mw.guard();
        let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
        let mut state = make_state_with_session(session.clone());

        let tc = ToolCall {
            id: "call-9".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "rm -rf /tmp" }),
        };

        // Snapshot pending state from a separate task between the request
        // being mirrored into the log and the human's decision arriving.
        let pending_session = session.clone();
        let pending_check = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            // At this point the requires_action event is already in the log
            // (it's appended before the side-channel notifier send).
            let pending = pending_actions(&*pending_session).await;
            assert_eq!(pending.len(), 1);
            assert_eq!(pending[0].request_id, req.request_id);
            assert_eq!(pending[0].tool_call_id, "call-9");
            assert_eq!(pending[0].tool_name, "execute_shell");
            assert_eq!(pending[0].action_type, "tool_confirmation");
            req
        });

        let mw_guard = guard.clone();
        let resolver_session = session.clone();
        let resolver = tokio::spawn(async move {
            let req = pending_check.await.unwrap();
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                crate::PermissionDecision::RejectOnce,
            );
            drop(g);
            // After resolve, the middleware appends `requires_action_resolved`
            // — but that happens in the middleware task, not here. The
            // assertion below the join awaits both.
            let _ = resolver_session;
        });

        let _ = mw.before_tool_call(&mut state, &tc).await;
        resolver.await.unwrap();

        // After resolution: nothing pending.
        assert!(pending_actions(&*session).await.is_empty());
    }

    #[tokio::test]
    async fn timeout_records_resolved_with_timed_out_status() {
        use alva_kernel_abi::agent_session::EventQuery;

        // Drop the rx side immediately so even a successful send doesn't keep
        // the channel alive — but the middleware doesn't depend on rx;
        // it's the oneshot in PermissionManager that drives the wait.
        // Tight timeout forces the TimedOut branch.
        let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus_handle)
            .with_approval_timeout(std::time::Duration::from_millis(10));
        let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
        let mut state = make_state_with_session(session.clone());

        let tc = ToolCall {
            id: "call-timeout".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_err());

        let events = session.query(&EventQuery::default()).await;
        let res = events
            .iter()
            .find(|m| m.event.event_type == EVENT_REQUIRES_ACTION_RESOLVED)
            .expect("timed-out approvals must still emit a resolved event");
        assert_eq!(res.event.data.as_ref().unwrap()["status"], "timed_out");

        // Nothing pending — the timed-out request is resolved.
        assert!(pending_actions(&*session).await.is_empty());
    }

    #[tokio::test]
    async fn no_handler_records_resolved_with_no_handler_status() {
        use alva_kernel_abi::agent_session::EventQuery;

        // Bus without ApprovalNotifier — the (None, _) fallback should still
        // emit the request and a resolved event with status="no_handler".
        let bus = Bus::new();
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus_handle);
        let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
        let mut state = make_state_with_session(session.clone());

        let tc = ToolCall {
            id: "call-nohandler".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_err());

        let events = session.query(&EventQuery::default()).await;
        assert!(
            events
                .iter()
                .any(|m| m.event.event_type == EVENT_REQUIRES_ACTION),
            "missing notifier should still record the request in the log"
        );
        let res = events
            .iter()
            .find(|m| m.event.event_type == EVENT_REQUIRES_ACTION_RESOLVED)
            .expect("missing notifier should still record a resolution");
        assert_eq!(res.event.data.as_ref().unwrap()["status"], "no_handler");
    }

    #[tokio::test]
    async fn subscriber_sees_requires_action_live() {
        use alva_kernel_abi::agent_session::ListenableInMemorySession;
        use tokio_stream::StreamExt;

        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let bus_handle = bus.handle();

        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus_handle);
        let guard = mw.guard();

        // ListenableInMemorySession supports live-tail subscriptions, which
        // is the moral equivalent of Anthropic's `events.stream` endpoint.
        let session = Arc::new(ListenableInMemorySession::new());
        let session_dyn: Arc<dyn AgentSession> = session.clone();
        let mut state = make_state_with_session(session_dyn.clone());

        // Subscribe BEFORE the approval flow runs — we want to see the
        // events appear live, not just in historical replay.
        let mut stream = session.subscribe_events(0).await;

        let tc = ToolCall {
            id: "call-live".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({ "command": "ls /projects/test" }),
        };

        let mw_guard = guard.clone();
        let approver = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                crate::PermissionDecision::AllowOnce,
            );
        });

        mw.before_tool_call(&mut state, &tc).await.unwrap();
        approver.await.unwrap();

        // Collect events from the stream until we see both expected types.
        // Bounded poll so a regression can't hang the test.
        let mut saw_req = false;
        let mut saw_res = false;
        for _ in 0..32 {
            let next =
                tokio::time::timeout(std::time::Duration::from_millis(200), stream.next()).await;
            let Ok(Some(event)) = next else { break };
            if event.event_type == EVENT_REQUIRES_ACTION {
                saw_req = true;
            }
            if event.event_type == EVENT_REQUIRES_ACTION_RESOLVED {
                saw_res = true;
            }
            if saw_req && saw_res {
                break;
            }
        }
        assert!(saw_req, "subscriber must observe requires_action live");
        assert!(
            saw_res,
            "subscriber must observe requires_action_resolved live"
        );
    }

    // -----------------------------------------------------------------------
    // URL-aware SSRF approval (T6 / 3C path, Loop D2)
    //
    // SecurityGuard's `check_url_in_tool_call` triggers a NeedHumanApproval
    // decision when a `url_aware_tools` mapping matches AND the URL's risk
    // is at or above the guard's threshold. The middleware then runs the
    // SAME approval flow it uses for `dangerous_tools` — these tests
    // verify that integration end-to-end with `read_url` (the only
    // url-aware tool registered by default).
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn url_aware_public_url_passes_without_approval() {
        // Public IP literal (8.8.8.8) → Low risk → below default Medium
        // threshold → no HITL request, middleware returns Allow directly.
        let (approval_tx, _approval_rx) = tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        bus.writer()
            .provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus.handle());
        let mut state = make_state();
        let tc = ToolCall {
            id: "u1".into(),
            name: "read_url".into(),
            arguments: serde_json::json!({ "url": "https://8.8.8.8/" }),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(
            result.is_ok(),
            "public URL must NOT trigger approval: {result:?}"
        );
    }

    #[tokio::test]
    async fn url_aware_loopback_triggers_approval_and_proceeds_on_allow() {
        // Loopback (127.0.0.1) → High risk → ≥ default Medium → HITL.
        // Approver returns AllowOnce → middleware returns Ok.
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        bus.writer()
            .provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus.handle());
        let guard = mw.guard();
        let mut state = make_state();

        let tc = ToolCall {
            id: "u2".into(),
            name: "read_url".into(),
            arguments: serde_json::json!({ "url": "http://127.0.0.1:9/" }),
        };

        let mw_guard = guard.clone();
        let approver = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            assert_eq!(req.tool_name, "read_url", "tool_name must be read_url");
            // The arguments payload must carry the URL the LLM was about
            // to fetch — otherwise UI can't render a meaningful prompt.
            assert_eq!(
                req.arguments.get("url").and_then(|v| v.as_str()),
                Some("http://127.0.0.1:9/")
            );
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "read_url",
                crate::PermissionDecision::AllowOnce,
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approver.await.unwrap();
        assert!(
            result.is_ok(),
            "AllowOnce must let the URL through: {result:?}"
        );
    }

    #[tokio::test]
    async fn url_aware_loopback_denied_on_reject() {
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        bus.writer()
            .provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus.handle());
        let guard = mw.guard();
        let mut state = make_state();

        let tc = ToolCall {
            id: "u3".into(),
            name: "read_url".into(),
            arguments: serde_json::json!({ "url": "http://169.254.169.254/iam-role" }),
        };

        let mw_guard = guard.clone();
        let approver = tokio::spawn(async move {
            let req = approval_rx.recv().await.unwrap();
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "read_url",
                crate::PermissionDecision::RejectOnce,
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approver.await.unwrap();
        assert!(result.is_err(), "RejectOnce must block IMDS URL");
    }

    #[tokio::test]
    async fn url_aware_skipped_for_non_mapped_tool() {
        // execute_shell is NOT in url_aware_tools — even if it had a
        // `url` arg, the URL check must be skipped (no double-HITL).
        // This tests that the URL phase is gated on the mapping.
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        bus.writer()
            .provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus.handle());
        let guard = mw.guard();
        let mut state = make_state();

        // execute_shell IS in dangerous_tools → triggers approval anyway,
        // but the approval should be for the shell command (not URL).
        // We auto-allow it and verify the URL phase didn't fire a 2nd time.
        let tc = ToolCall {
            id: "u4".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({
                "command": "echo hi",
                "url": "http://127.0.0.1/",  // intentionally provided but should be ignored
            }),
        };

        let mw_guard = guard.clone();
        let approver = tokio::spawn(async move {
            // Should receive EXACTLY one approval request (for execute_shell),
            // not a second one for the URL.
            let req = approval_rx.recv().await.unwrap();
            assert_eq!(req.tool_name, "execute_shell");
            let mut g = mw_guard.lock().await;
            g.resolve_permission(
                &req.request_id,
                "execute_shell",
                crate::PermissionDecision::AllowOnce,
            );
            // Brief wait then assert no second request arrives
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            assert!(
                approval_rx.try_recv().is_err(),
                "non-url-aware tool must NOT trigger a 2nd URL approval"
            );
        });

        let result = mw.before_tool_call(&mut state, &tc).await;
        approver.await.unwrap();
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn url_aware_threshold_none_passes_loopback_without_approval() {
        // User-configured "trust all" mode: ask_threshold = None.
        // Even loopback must not trigger HITL.
        use crate::url_info::UrlRules;
        let (approval_tx, mut approval_rx) =
            tokio::sync::mpsc::unbounded_channel::<ApprovalRequest>();
        let bus = Bus::new();
        bus.writer()
            .provide(Arc::new(ApprovalNotifier { tx: approval_tx }));
        let mw = SecurityMiddleware::for_workspace("/projects/test", SandboxMode::RestrictiveOpen)
            .with_bus(bus.handle());
        {
            let guard_arc = mw.guard();
            let mut g = guard_arc.lock().await;
            g.set_url_rules(UrlRules {
                ask_threshold: None,
            });
        }
        let mut state = make_state();
        let tc = ToolCall {
            id: "u5".into(),
            name: "read_url".into(),
            arguments: serde_json::json!({ "url": "http://127.0.0.1/" }),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok(), "threshold=None must skip HITL: {result:?}");
        assert!(
            approval_rx.try_recv().is_err(),
            "no approval request should have been sent"
        );
    }
}
