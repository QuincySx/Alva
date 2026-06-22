use std::sync::Arc;

use alva_kernel_abi::agent_session::{EmitterKind, EventEmitter, ScopedSession};
use alva_kernel_abi::{AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock};
use alva_kernel_abi::{Message, MessageRole, Tool, ToolCall, ToolOutput};
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::event::AgentEvent;
use crate::middleware::{MiddlewareError, ToolCallFn};
use crate::runtime_context::RuntimeExecutionContext;
use crate::session_events::emit_runtime_event;
use crate::state::{AgentConfig, AgentState};

#[derive(Debug, Clone)]
pub struct CommittedToolCall {
    pub tool_call: ToolCall,
    pub result: ToolOutput,
    pub message: AgentMessage,
    pub tool_use_uuid: String,
}

#[derive(Default)]
pub struct ToolBatchCoordinator;

impl ToolBatchCoordinator {
    pub fn new() -> Self {
        Self
    }

    pub async fn execute_batch(
        &self,
        state: &mut AgentState,
        config: &AgentConfig,
        cancel: CancellationToken,
        tool_calls: &[ToolCall],
        llm_call_uuid: String,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
    ) -> Result<Vec<CommittedToolCall>, AgentError> {
        let mut committed = Vec::with_capacity(tool_calls.len());

        for tool_call in tool_calls {
            if cancel.is_cancelled() {
                let committed_cancelled = self
                    .commit_cancelled_tool_call(state, tool_call, llm_call_uuid.clone(), &event_tx)
                    .await;
                committed.push(committed_cancelled);
                continue;
            }

            let tool_use_uuid = emit_runtime_event(
                &state.session,
                "tool_use",
                Some(llm_call_uuid.clone()),
                Some(serde_json::json!({
                    "tool_name": tool_call.name.clone(),
                    "tool_call_id": tool_call.id.clone(),
                })),
            )
            .await;

            let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                tool_call: tool_call.clone(),
            });
            let tool_start = web_time::Instant::now();

            let tool = state
                .tools
                .iter()
                .find(|t| t.name() == tool_call.name)
                .cloned();

            let before_result = config
                .middleware
                .run_before_tool_call(state, tool_call)
                .await;

            let mut result = match before_result {
                Err(MiddlewareError::Blocked { reason }) => {
                    ToolOutput::error(format!("Tool call blocked: {}", reason))
                }
                Err(e) => {
                    return Err(e.into_agent_error());
                }
                Ok(()) => match tool {
                    Some(ref t) => {
                        let _lock_guards = if let Some(bus) = config.bus.as_ref() {
                            if let Some(registry) = bus.get::<alva_kernel_abi::ToolLockRegistry>() {
                                let keys = t.resource_keys(&tool_call.arguments);
                                let mode = t.execution_mode();
                                Some(match config.workspace.as_deref() {
                                    Some(ws) => registry.acquire_within(&keys, mode, ws).await,
                                    None => registry.acquire(&keys, mode).await,
                                })
                            } else {
                                None
                            }
                        } else {
                            None
                        };

                        let actual_tool_call = ActualToolCall {
                            tool: t.clone(),
                            cancel: cancel.clone(),
                            event_tx: event_tx.clone(),
                            session_id: state.session.session_id().to_string(),
                            workspace: config.workspace.clone(),
                            bus: config.bus.clone(),
                        };
                        config
                            .middleware
                            .run_wrap_tool_call(state, tool_call, &actual_tool_call)
                            .await
                            .map_err(MiddlewareError::into_agent_error)?
                    }
                    None => ToolOutput::error(format!("Tool not found: {}", tool_call.name)),
                },
            };

            config
                .middleware
                .run_after_tool_call(state, tool_call, &mut result)
                .await
                .map_err(MiddlewareError::into_agent_error)?;

            let tool_message = Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    id: tool_call.id.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                }],
                tool_call_id: Some(tool_call.id.clone()),
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            };
            let tool_msg = AgentMessage::Standard(tool_message);
            state
                .session
                .append_message(tool_msg.clone(), Some(tool_use_uuid.clone()))
                .await;

            let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
            tracing::info!(
                tool = %tool_call.name,
                duration_ms = tool_duration_ms,
                is_error = result.is_error,
                result_len = result.model_text().len(),
                "tool execution completed"
            );

            let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                tool_call: tool_call.clone(),
                result: result.clone(),
            });

            committed.push(CommittedToolCall {
                tool_call: tool_call.clone(),
                result,
                message: tool_msg,
                tool_use_uuid,
            });
        }

        Ok(committed)
    }

    async fn commit_cancelled_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        llm_call_uuid: String,
        event_tx: &mpsc::UnboundedSender<AgentEvent>,
    ) -> CommittedToolCall {
        let tool_use_uuid = emit_runtime_event(
            &state.session,
            "tool_use",
            Some(llm_call_uuid),
            Some(serde_json::json!({
                "tool_name": tool_call.name.clone(),
                "tool_call_id": tool_call.id.clone(),
                "cancelled": true,
            })),
        )
        .await;

        let _ = event_tx.send(AgentEvent::ToolExecutionStart {
            tool_call: tool_call.clone(),
        });

        let result = ToolOutput::error("Tool call cancelled before execution");
        let tool_message = Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: tool_call.id.clone(),
                content: result.content.clone(),
                is_error: result.is_error,
            }],
            tool_call_id: Some(tool_call.id.clone()),
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        };
        let tool_msg = AgentMessage::Standard(tool_message);
        state
            .session
            .append_message(tool_msg.clone(), Some(tool_use_uuid.clone()))
            .await;

        let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
            tool_call: tool_call.clone(),
            result: result.clone(),
        });

        CommittedToolCall {
            tool_call: tool_call.clone(),
            result,
            message: tool_msg,
            tool_use_uuid,
        }
    }
}

struct ActualToolCall {
    tool: Arc<dyn Tool>,
    cancel: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    workspace: Option<std::path::PathBuf>,
    bus: Option<BusHandle>,
}

#[async_trait]
impl ToolCallFn for ActualToolCall {
    async fn call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError> {
        let scoped_session = ScopedSession::new(
            state.session.clone(),
            EventEmitter {
                kind: EmitterKind::Tool,
                id: self.tool.name().to_string(),
                instance: None,
            },
        );
        let mut ctx = RuntimeExecutionContext::new(
            self.cancel.clone(),
            tool_call.id.clone(),
            self.event_tx.clone(),
            self.session_id.clone(),
        )
        .with_session(scoped_session);
        if let Some(ref ws) = self.workspace {
            ctx = ctx.with_workspace(ws.clone());
        }
        if let Some(ref bus) = self.bus {
            ctx = ctx.with_bus(bus.clone());
        }
        self.tool.execute(tool_call.arguments.clone(), &ctx).await
    }
}
