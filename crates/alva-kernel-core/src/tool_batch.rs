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

        for (idx, tool_call) in tool_calls.iter().enumerate() {
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

            // Run before/wrap/after inside one fallible scope. A non-Blocked
            // middleware error is fatal to the batch, but we must not let it
            // escape before backfilling tool_results — see the Err arm below.
            let outcome: Result<ToolOutput, AgentError> = async {
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
                        Some(ref t) => 'locked: {
                            let _lock_guards = if let Some(bus) = config.bus.as_ref() {
                                if let Some(registry) =
                                    bus.get::<alva_kernel_abi::ToolLockRegistry>()
                                {
                                    let keys = t.resource_keys(&tool_call.arguments);
                                    let mode = t.execution_mode();
                                    // Bounded acquire: deadlock-shaped contention
                                    // becomes a visible, retryable tool error
                                    // instead of parking the agent loop forever
                                    // (the timeout variant sat unused while the
                                    // hot path hung — gap scan 2026-07-05).
                                    match registry
                                        .acquire_bounded(&keys, mode, config.workspace.as_deref())
                                        .await
                                    {
                                        Ok(guards) => Some(guards),
                                        Err(e) => {
                                            break 'locked ToolOutput::error(format!(
                                            "Tool lock acquisition failed: {e}. Another tool may \
                                             be holding the lock unusually long; try again."
                                        ));
                                        }
                                    }
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

                Ok(result)
            }
            .await;

            let result = match outcome {
                Ok(result) => result,
                Err(agent_err) => {
                    // A non-Blocked middleware error aborts the batch. Before
                    // propagating it, close out THIS tool call and every one
                    // that has not started yet with an error ToolResult, so the
                    // assistant's committed tool_use blocks are never left
                    // dangling for the next provider request.
                    let error_text = format!("Tool call aborted before completion: {agent_err}");
                    append_error_tool_result(
                        state,
                        tool_call,
                        Some(tool_use_uuid.clone()),
                        &error_text,
                        &event_tx,
                    )
                    .await;
                    backfill_aborted_tool_results(
                        state,
                        &tool_calls[idx + 1..],
                        &llm_call_uuid,
                        &error_text,
                        &event_tx,
                    )
                    .await;
                    return Err(agent_err);
                }
            };

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

/// Append an error `ToolResult` for `tool_call` (paired to `parent_uuid`)
/// and emit the matching `ToolExecutionEnd`. Used when the batch aborts, so
/// that a tool_use block committed to the session is never left without a
/// tool_result — a provider re-sent such a history rejects it for having
/// unmatched tool_use ids.
async fn append_error_tool_result(
    state: &mut AgentState,
    tool_call: &ToolCall,
    parent_uuid: Option<String>,
    error_text: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    let result = ToolOutput::error(error_text.to_string());
    let tool_message = Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Tool,
        content: vec![ContentBlock::ToolResult {
            id: tool_call.id.clone(),
            content: result.content.clone(),
            is_error: true,
        }],
        tool_call_id: Some(tool_call.id.clone()),
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    };
    state
        .session
        .append_message(AgentMessage::Standard(tool_message), parent_uuid)
        .await;
    let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
        tool_call: tool_call.clone(),
        result,
    });
}

/// Close out every tool call that had not started yet (the ones after the
/// aborting call) with a tool_use skeleton event + an error ToolResult, so
/// the whole batch's tool_use blocks stay paired even though execution
/// stopped early.
async fn backfill_aborted_tool_results(
    state: &mut AgentState,
    remaining: &[ToolCall],
    llm_call_uuid: &str,
    error_text: &str,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) {
    for tool_call in remaining {
        let tool_use_uuid = emit_runtime_event(
            &state.session,
            "tool_use",
            Some(llm_call_uuid.to_string()),
            Some(serde_json::json!({
                "tool_name": tool_call.name.clone(),
                "tool_call_id": tool_call.id.clone(),
                "aborted": true,
            })),
        )
        .await;
        append_error_tool_result(state, tool_call, Some(tool_use_uuid), error_text, event_tx).await;
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
