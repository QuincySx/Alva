use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::adapters::acp::{
    protocol::{
        content::ContentBlock,
        lifecycle::TaskFinishReason,
        message::{AcpInboundMessage, AcpOutboundMessage},
        permission::{PermissionData, PermissionRequest},
    },
    process::manager::AcpProcessManager,
    session::permission_manager::PermissionManager,
    AcpError,
};
use crate::application::engine::EngineEvent;

#[derive(Debug, Clone, PartialEq)]
pub enum AcpSessionState {
    Ready,
    Running,
    WaitingForPermission { request_id: String },
    Completed,
    Cancelled,
    Error { message: String },
    Crashed,
}

/// A single ACP interaction session (corresponds to one prompt -> response cycle)
pub struct AcpSession {
    pub session_id: String,
    /// Bound ACP child process ID
    pub process_id: String,
    pub state: Arc<Mutex<AcpSessionState>>,
    /// Pending permission requests (request_id -> callback sender)
    pending_permissions:
        Arc<Mutex<std::collections::HashMap<String, oneshot::Sender<PermissionData>>>>,
    permission_manager: Arc<PermissionManager>,
    process_manager: Arc<AcpProcessManager>,
    /// Sender to forward events to Sub-2 engine event bus
    engine_event_tx: mpsc::Sender<EngineEvent>,
}

impl AcpSession {
    pub fn new(
        session_id: String,
        process_id: String,
        permission_manager: Arc<PermissionManager>,
        process_manager: Arc<AcpProcessManager>,
        engine_event_tx: mpsc::Sender<EngineEvent>,
    ) -> Self {
        Self {
            session_id,
            process_id,
            state: Arc::new(Mutex::new(AcpSessionState::Ready)),
            pending_permissions: Arc::new(Mutex::new(Default::default())),
            permission_manager,
            process_manager,
            engine_event_tx,
        }
    }

    /// Send prompt to external Agent, start execution loop
    pub async fn send_prompt(&self, prompt: String, resume: bool) -> Result<(), AcpError> {
        *self.state.lock().await = AcpSessionState::Running;
        self.process_manager
            .send(
                &self.process_id,
                AcpOutboundMessage::Prompt {
                    content: prompt,
                    resume: if resume { Some(true) } else { None },
                },
            )
            .await
    }

    /// Cancel current task
    pub async fn cancel(&self) -> Result<(), AcpError> {
        self.process_manager
            .send(&self.process_id, AcpOutboundMessage::Cancel)
            .await
    }

    /// Handle inbound message from external Agent.
    /// Driven by AcpProcessManager's subscribe (runs in independent task).
    pub async fn handle_inbound(&self, msg: AcpInboundMessage) {
        match msg {
            AcpInboundMessage::TaskStart { data } => {
                tracing::debug!("acp task_start: task_id={}", data.task_id);
                *self.state.lock().await = AcpSessionState::Running;
            }

            AcpInboundMessage::TaskComplete { data } => {
                let new_state = match data.finish_reason {
                    TaskFinishReason::Complete => AcpSessionState::Completed,
                    TaskFinishReason::Cancelled => AcpSessionState::Cancelled,
                    TaskFinishReason::Error => AcpSessionState::Error {
                        message: data
                            .summary
                            .unwrap_or_else(|| "unknown error".to_string()),
                    },
                    TaskFinishReason::MaxIterations => AcpSessionState::Error {
                        message: "max iterations reached".to_string(),
                    },
                };
                *self.state.lock().await = new_state;
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::Completed {
                        session_id: self.session_id.clone(),
                    })
                    .await;
            }

            AcpInboundMessage::SessionUpdate { content, .. }
            | AcpInboundMessage::MessageUpdate { content, .. } => {
                for block in content {
                    self.forward_content_block(block).await;
                }
            }

            AcpInboundMessage::RequestPermission { request_id, data } => {
                self.handle_permission_request(request_id, data).await;
            }

            AcpInboundMessage::PreToolUse { data } => {
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::ToolCallStarted {
                        session_id: self.session_id.clone(),
                        tool_name: data.tool_name,
                        tool_call_id: data.tool_call_id,
                    })
                    .await;
            }

            AcpInboundMessage::PostToolUse { data } => {
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::ToolCallCompleted {
                        session_id: self.session_id.clone(),
                        tool_call_id: data.tool_call_id,
                        output: data.output,
                        is_error: data.is_error,
                    })
                    .await;
            }

            AcpInboundMessage::ErrorData { data } => {
                *self.state.lock().await = AcpSessionState::Error {
                    message: data.message.clone(),
                };
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::Error {
                        session_id: self.session_id.clone(),
                        error: data.message,
                    })
                    .await;
            }

            AcpInboundMessage::PingPong { data } => {
                // Reply pong
                let _ = self
                    .process_manager
                    .send(&self.process_id, AcpOutboundMessage::Pong { id: data.id })
                    .await;
            }

            // Plan / SystemMessage / FinishData / ToolCallData — log or persist, no state change
            AcpInboundMessage::Plan { data } => {
                tracing::info!("acp plan: {} steps", data.steps.len());
            }
            AcpInboundMessage::SystemMessage { data } => {
                tracing::debug!("acp system[{:?}]: {}", data.level, data.message);
            }
            AcpInboundMessage::FinishData { data } => {
                tracing::debug!("acp finish_data: output_len={}", data.output.len());
            }
            AcpInboundMessage::ToolCallData { .. } => {
                // Persistence handled by storage layer (injected at higher level)
            }
        }
    }

    /// Content block -> EngineEvent forwarding
    async fn forward_content_block(&self, block: ContentBlock) {
        match block {
            ContentBlock::Text { text, .. } => {
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::TextDelta {
                        session_id: self.session_id.clone(),
                        text,
                    })
                    .await;
            }
            ContentBlock::ToolUse { id, name, .. } => {
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::ToolCallStarted {
                        session_id: self.session_id.clone(),
                        tool_name: name,
                        tool_call_id: id,
                    })
                    .await;
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                let _ = self
                    .engine_event_tx
                    .send(EngineEvent::ToolCallCompleted {
                        session_id: self.session_id.clone(),
                        tool_call_id: tool_use_id,
                        output: content,
                        is_error,
                    })
                    .await;
            }
        }
    }

    /// Handle permission request: check cache first, if hit respond directly;
    /// if miss, wait for UI callback
    async fn handle_permission_request(&self, request_id: String, req: PermissionRequest) {
        // 1. Check allow_always / reject_always cache
        if let Some(cached) = self.permission_manager.check_cached(&req.tool_name).await {
            let _ = self
                .process_manager
                .send(
                    &self.process_id,
                    AcpOutboundMessage::PermissionResponse {
                        request_id: request_id.clone(),
                        data: cached,
                    },
                )
                .await;
            return;
        }

        // 2. Update session state -> WaitingForPermission
        *self.state.lock().await = AcpSessionState::WaitingForPermission {
            request_id: request_id.clone(),
        };

        // 3. Create oneshot channel to wait for UI response
        let (tx, rx) = oneshot::channel::<PermissionData>();
        self.pending_permissions
            .lock()
            .await
            .insert(request_id.clone(), tx);

        // 4. Notify UI layer (via EngineEvent::WaitingForHuman)
        // Sub-7 will pop a HITL permission dialog here
        let _ = self
            .engine_event_tx
            .send(EngineEvent::WaitingForHuman {
                session_id: self.session_id.clone(),
                question: format!(
                    "[Permission Request] {}\nTool: {} | Risk: {:?}",
                    req.description, req.tool_name, req.risk_level
                ),
                ask_id: request_id.clone(),
            })
            .await;

        // 5. Wait for user response (async suspend, does not block tokio executor)
        let process_manager = self.process_manager.clone();
        let process_id = self.process_id.clone();
        let session_state = self.state.clone();
        let permission_manager = self.permission_manager.clone();
        let tool_name = req.tool_name.clone();

        tokio::spawn(async move {
            if let Ok(data) = rx.await {
                // Persist allow_always / reject_always to cache
                permission_manager.record(&tool_name, &data).await;

                // Send PermissionData back to external Agent
                let _ = process_manager
                    .send(
                        &process_id,
                        AcpOutboundMessage::PermissionResponse { request_id, data },
                    )
                    .await;

                // Resume Running state
                *session_state.lock().await = AcpSessionState::Running;
            }
        });
    }

    /// UI layer calls this: after user makes permission choice, forward via this method
    pub async fn resolve_permission(
        &self,
        request_id: &str,
        data: PermissionData,
    ) -> Result<(), AcpError> {
        let mut pending = self.pending_permissions.lock().await;
        if let Some(tx) = pending.remove(request_id) {
            let _ = tx.send(data);
            Ok(())
        } else {
            Err(AcpError::PermissionRequestNotFound(
                request_id.to_string(),
            ))
        }
    }
}
