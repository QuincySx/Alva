// INPUT:  std::sync, tokio::sync, crate::agent::agent_client::{protocol, connection, session::permission_manager, AcpError}, crate::ui_message_stream, uuid
// OUTPUT: AcpSessionState, AcpSession
// POS:    ACP session state machine — drives inbound message handling, content forwarding, and HITL permission flow.
use std::sync::Arc;
use tokio::sync::{mpsc, oneshot, Mutex};

use crate::agent::agent_client::{
    protocol::{
        content::ContentBlock,
        lifecycle::TaskFinishReason,
        message::{AcpInboundMessage, AcpOutboundMessage},
        permission::{PermissionData, PermissionRequest},
    },
    connection::factory::AcpProcessManager,
    session::permission_manager::PermissionManager,
    AcpError,
};
use crate::ui_message_stream::{FinishReason, UIMessageChunk};

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
    /// Sender to forward UIMessageChunk events to the UI layer
    chunk_tx: mpsc::Sender<UIMessageChunk>,
}

impl AcpSession {
    pub fn new(
        session_id: String,
        process_id: String,
        permission_manager: Arc<PermissionManager>,
        process_manager: Arc<AcpProcessManager>,
        chunk_tx: mpsc::Sender<UIMessageChunk>,
    ) -> Self {
        Self {
            session_id,
            process_id,
            state: Arc::new(Mutex::new(AcpSessionState::Ready)),
            pending_permissions: Arc::new(Mutex::new(Default::default())),
            permission_manager,
            process_manager,
            chunk_tx,
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
                    .chunk_tx
                    .send(UIMessageChunk::Finish {
                        finish_reason: FinishReason::Stop,
                        usage: None,
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
                    .chunk_tx
                    .send(UIMessageChunk::ToolInputStart {
                        id: data.tool_call_id,
                        tool_name: data.tool_name,
                        title: None,
                    })
                    .await;
            }

            AcpInboundMessage::PostToolUse { data } => {
                if data.is_error {
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::ToolOutputError {
                            id: data.tool_call_id,
                            error: data.output,
                        })
                        .await;
                } else {
                    let output = serde_json::from_str(&data.output)
                        .unwrap_or(serde_json::Value::String(data.output));
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::ToolOutputAvailable {
                            id: data.tool_call_id,
                            output,
                        })
                        .await;
                }
            }

            AcpInboundMessage::ErrorData { data } => {
                *self.state.lock().await = AcpSessionState::Error {
                    message: data.message.clone(),
                };
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::Error {
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

            // Plan / SystemMessage / FinishData / ToolCallData -- log or persist, no state change
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

    /// Content block -> UIMessageChunk forwarding
    async fn forward_content_block(&self, block: ContentBlock) {
        match block {
            ContentBlock::Text { text, .. } => {
                // Generate a part id for the text chunk
                let id = uuid::Uuid::new_v4().to_string();
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::TextStart { id: id.clone() })
                    .await;
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::TextDelta {
                        id: id.clone(),
                        delta: text,
                    })
                    .await;
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::TextEnd { id })
                    .await;
            }
            ContentBlock::ToolUse { id, name, .. } => {
                let _ = self
                    .chunk_tx
                    .send(UIMessageChunk::ToolInputStart {
                        id,
                        tool_name: name,
                        title: None,
                    })
                    .await;
            }
            ContentBlock::ToolResult {
                tool_use_id,
                content,
                is_error,
            } => {
                if is_error {
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::ToolOutputError {
                            id: tool_use_id,
                            error: content,
                        })
                        .await;
                } else {
                    let output = serde_json::from_str(&content)
                        .unwrap_or(serde_json::Value::String(content));
                    let _ = self
                        .chunk_tx
                        .send(UIMessageChunk::ToolOutputAvailable {
                            id: tool_use_id,
                            output,
                        })
                        .await;
                }
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

        // 4. Notify UI layer (via ToolApprovalRequest)
        let _ = self
            .chunk_tx
            .send(UIMessageChunk::ToolApprovalRequest {
                id: request_id.clone(),
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
