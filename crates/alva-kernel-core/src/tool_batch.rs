// INPUT:  AgentState/AgentConfig, Tool/ToolCall scheduling metadata, MiddlewareStack, AgentSession
// OUTPUT: ToolBatchCoordinator, CommittedToolCall
// POS:    Executes conflict-free tool waves concurrently while committing intents and outcomes in model order.
use std::sync::Arc;

use alva_kernel_abi::agent_session::{EmitterKind, EventEmitter, ScopedSession};
use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock, ConversationLog,
    ExecutionRecord, LockMode, RecordValue, ResourceKey, Scheduling, SessionRecord,
};
use alva_kernel_abi::{Message, MessageRole, Tool, ToolCall, ToolOutput};
use async_trait::async_trait;
use futures_util::future::join_all;

use crate::event::{AgentEvent, AgentEventSender};
use crate::middleware::{MiddlewareError, ToolCallFn};
use crate::runtime_context::RuntimeExecutionContext;
use crate::state::{AgentConfig, AgentState};

#[derive(Debug, Clone)]
pub struct CommittedToolCall {
    pub tool_call: ToolCall,
    pub result: ToolOutput,
    pub message: AgentMessage,
    pub tool_use_uuid: String,
}

struct PreparedToolCall {
    tool_call: ToolCall,
    tool: Option<Arc<dyn Tool>>,
    tool_use_uuid: String,
}

struct ExecutedToolCall {
    prepared: PreparedToolCall,
    outcome: Result<ToolOutput, AgentError>,
    duration_ms: u64,
}

#[derive(Default)]
pub struct ToolBatchCoordinator;

impl ToolBatchCoordinator {
    pub fn new() -> Self {
        Self
    }

    /// `log` is the narrowed conversation-log handle — the batch coordinator
    /// commits intents and outcomes through the ConversationLog seam, never
    /// through the full session store.
    pub async fn execute_batch<E>(
        &self,
        state: &AgentState,
        log: &std::sync::Arc<dyn ConversationLog>,
        config: &AgentConfig,
        cancel: CancellationToken,
        tool_calls: &[ToolCall],
        llm_call_uuid: String,
        event_tx: E,
    ) -> Result<Vec<CommittedToolCall>, AgentError>
    where
        E: Into<AgentEventSender>,
    {
        let event_tx = event_tx.into();
        let mut committed = Vec::with_capacity(tool_calls.len());
        let mut cursor = 0;

        while cursor < tool_calls.len() {
            if cancel.is_cancelled() {
                for tool_call in &tool_calls[cursor..] {
                    let committed_cancelled = self
                        .commit_cancelled_tool_call(
                            log,
                            tool_call,
                            llm_call_uuid.clone(),
                            &event_tx,
                        )
                        .await?;
                    committed.push(committed_cancelled);
                }
                break;
            }

            let wave_len =
                parallel_wave_len(state, config.workspace.as_deref(), &tool_calls[cursor..]);
            if wave_len < 2 {
                let tool_call = &tool_calls[cursor];
                let tool_use_uuid = commit_tool_intent(log, tool_call, &llm_call_uuid).await?;
                let executed = execute_tool_call(
                    state,
                    log,
                    config,
                    cancel.clone(),
                    PreparedToolCall {
                        tool_call: tool_call.clone(),
                        tool: find_tool(state, tool_call),
                        tool_use_uuid,
                    },
                    event_tx.clone(),
                )
                .await;

                match executed.outcome {
                    Ok(result) => {
                        committed.push(
                            commit_executed_tool_call(
                                log,
                                executed.prepared,
                                result,
                                executed.duration_ms,
                                &event_tx,
                            )
                            .await?,
                        );
                    }
                    Err(agent_err) => {
                        let error_text =
                            format!("Tool call aborted before completion: {agent_err}");
                        commit_failed_tool_call(log, executed.prepared, &error_text, &event_tx)
                            .await?;
                        backfill_aborted_tool_results(
                            log,
                            &tool_calls[cursor + 1..],
                            &llm_call_uuid,
                            &error_text,
                            &event_tx,
                        )
                        .await?;
                        return Err(agent_err);
                    }
                }
                cursor += 1;
                continue;
            }

            let wave_calls = &tool_calls[cursor..cursor + wave_len];
            let tool_use_uuids = commit_tool_intents(log, wave_calls, &llm_call_uuid).await?;
            let executions =
                wave_calls
                    .iter()
                    .zip(tool_use_uuids)
                    .map(|(tool_call, tool_use_uuid)| {
                        execute_tool_call(
                            state,
                            log,
                            config,
                            cancel.clone(),
                            PreparedToolCall {
                                tool_call: tool_call.clone(),
                                tool: find_tool(state, tool_call),
                                tool_use_uuid,
                            },
                            event_tx.clone(),
                        )
                    });
            let executed = join_all(executions).await;
            let mut first_error = None;

            for execution in executed {
                match execution.outcome {
                    Ok(result) => {
                        committed.push(
                            commit_executed_tool_call(
                                log,
                                execution.prepared,
                                result,
                                execution.duration_ms,
                                &event_tx,
                            )
                            .await?,
                        );
                    }
                    Err(agent_err) => {
                        let error_text =
                            format!("Tool call aborted before completion: {agent_err}");
                        commit_failed_tool_call(log, execution.prepared, &error_text, &event_tx)
                            .await?;
                        if first_error.is_none() {
                            first_error = Some((agent_err, error_text));
                        }
                    }
                }
            }

            cursor += wave_len;
            if let Some((agent_err, error_text)) = first_error {
                if cursor < tool_calls.len() {
                    backfill_aborted_tool_results(
                        log,
                        &tool_calls[cursor..],
                        &llm_call_uuid,
                        &error_text,
                        &event_tx,
                    )
                    .await?;
                }
                return Err(agent_err);
            }
        }

        Ok(committed)
    }

    async fn commit_cancelled_tool_call(
        &self,
        log: &std::sync::Arc<dyn ConversationLog>,
        tool_call: &ToolCall,
        llm_call_uuid: String,
        event_tx: &AgentEventSender,
    ) -> Result<CommittedToolCall, AgentError> {
        let tool_use_uuid = commit_tool_intent(log, tool_call, &llm_call_uuid).await?;

        let _ = event_tx
            .send(AgentEvent::ToolExecutionStart {
                tool_call: tool_call.clone(),
            })
            .await;

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
        commit_tool_outcome(log, tool_call, &result, tool_msg.clone(), &tool_use_uuid).await?;

        let _ = event_tx
            .send(AgentEvent::ToolExecutionEnd {
                tool_call: tool_call.clone(),
                result: result.clone(),
            })
            .await;

        Ok(CommittedToolCall {
            tool_call: tool_call.clone(),
            result,
            message: tool_msg,
            tool_use_uuid,
        })
    }
}

fn find_tool(state: &AgentState, tool_call: &ToolCall) -> Option<Arc<dyn Tool>> {
    state
        .tools
        .iter()
        .find(|tool| tool.name() == tool_call.name)
        .cloned()
}

/// Return the size of the next conflict-free parallel wave.
///
/// A tool must opt in through its [`Scheduling`] descriptor —
/// `concurrency_safe` with fine-grained `ExecutionMode::Parallel` — and have
/// no read/write conflict with an earlier call in the same wave. Unknown,
/// global, coordinator, and concurrency-unsafe calls are hard ordering
/// barriers.
fn parallel_wave_len(
    state: &AgentState,
    workspace: Option<&std::path::Path>,
    tool_calls: &[ToolCall],
) -> usize {
    let Some(first_call) = tool_calls.first() else {
        return 0;
    };
    let Some(first_tool) = find_tool(state, first_call) else {
        return 1;
    };
    let first_scheduling = tool_scheduling(first_tool.as_ref(), first_call, workspace);
    if !is_parallel_candidate(&first_scheduling) {
        return 1;
    }

    let mut claimed = first_scheduling.resource_keys;
    let mut wave_len = 1;
    for tool_call in &tool_calls[1..] {
        let Some(tool) = find_tool(state, tool_call) else {
            break;
        };
        let scheduling = tool_scheduling(tool.as_ref(), tool_call, workspace);
        if !is_parallel_candidate(&scheduling) {
            break;
        }
        if resource_sets_conflict(&claimed, &scheduling.resource_keys) {
            break;
        }
        claimed.extend(scheduling.resource_keys);
        wave_len += 1;
    }
    wave_len
}

/// Read the tool's scheduling descriptor with resource keys resolved against
/// the workspace root (same normalization the lock registry applies).
fn tool_scheduling(
    tool: &dyn Tool,
    tool_call: &ToolCall,
    workspace: Option<&std::path::Path>,
) -> Scheduling {
    let mut scheduling = tool.scheduling(&tool_call.arguments);
    scheduling.resource_keys = scheduling
        .resource_keys
        .into_iter()
        .map(|resource| ResourceKey {
            key: workspace.map_or(resource.key.clone(), |workspace| {
                alva_kernel_abi::tool::scheduler::resolve_against_workspace(
                    &resource.key,
                    workspace,
                )
            }),
            mode: resource.mode,
        })
        .collect();
    scheduling
}

fn is_parallel_candidate(scheduling: &Scheduling) -> bool {
    scheduling.execution_mode == alva_kernel_abi::ExecutionMode::Parallel
        && scheduling.concurrency_safe
}

fn resource_sets_conflict(left: &[ResourceKey], right: &[ResourceKey]) -> bool {
    left.iter().any(|left_key| {
        right.iter().any(|right_key| {
            left_key.key == right_key.key
                && (left_key.mode == LockMode::Write || right_key.mode == LockMode::Write)
        })
    })
}

async fn commit_tool_intents(
    log: &std::sync::Arc<dyn ConversationLog>,
    tool_calls: &[ToolCall],
    llm_call_uuid: &str,
) -> Result<Vec<String>, AgentError> {
    let mut records = Vec::with_capacity(tool_calls.len());
    let mut record_ids = Vec::with_capacity(tool_calls.len());
    for tool_call in tool_calls {
        let (record, record_id) = tool_intent_record(tool_call, llm_call_uuid);
        records.push(record);
        record_ids.push(record_id);
    }
    log.append_records(records)
        .await
        .map_err(|error| AgentError::from(error))?;
    Ok(record_ids)
}

async fn execute_tool_call(
    state: &AgentState,
    log: &std::sync::Arc<dyn ConversationLog>,
    config: &AgentConfig,
    cancel: CancellationToken,
    prepared: PreparedToolCall,
    event_tx: AgentEventSender,
) -> ExecutedToolCall {
    let tool_call = &prepared.tool_call;
    let _ = event_tx
        .send(AgentEvent::ToolExecutionStart {
            tool_call: tool_call.clone(),
        })
        .await;
    let tool_start = web_time::Instant::now();

    // Run before/wrap/after inside one fallible scope. Parallel waves are
    // restricted to tools that explicitly opt in to concurrent execution;
    // Middleware is Send + Sync and therefore shares the same concurrency
    // boundary as the tool invocation it wraps.
    let outcome: Result<ToolOutput, AgentError> = async {
        let before_result = config
            .middleware
            .run_before_tool_call(state, tool_call)
            .await;

        let mut result = match before_result {
            Err(MiddlewareError::Blocked { reason }) => {
                ToolOutput::error(format!("Tool call blocked: {reason}"))
            }
            Err(error) => return Err(error.into_agent_error()),
            Ok(()) => match prepared.tool.as_ref() {
                Some(tool) => {
                    // 跨 agent 资源锁的运行期表与取锁中间件已搬去 SDK 装配层
                    // （alva-agent-core::ToolLockMiddleware）；装配了锁插件的
                    // agent 由洋葱中间件在 wrap 阶段取锁/释放，内核不再认识
                    // "多个 agent 共享同一份资源"。

                    let actual_tool_call = ActualToolCall {
                        tool: tool.clone(),
                        cancel,
                        event_tx: event_tx.clone(),
                        session_id: log.session_id().to_string(),
                        workspace: config.workspace.clone(),
                        bus: config.bus.clone(),
                    };
                    config
                        .middleware
                        .run_wrap_tool_call(state, tool_call, Arc::new(actual_tool_call))
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

    ExecutedToolCall {
        prepared,
        outcome,
        duration_ms: tool_start.elapsed().as_millis() as u64,
    }
}

async fn commit_executed_tool_call(
    log: &std::sync::Arc<dyn ConversationLog>,
    prepared: PreparedToolCall,
    result: ToolOutput,
    duration_ms: u64,
    event_tx: &AgentEventSender,
) -> Result<CommittedToolCall, AgentError> {
    let tool_message = tool_result_message(&prepared.tool_call, &result);
    let tool_msg = AgentMessage::Standard(tool_message);
    commit_tool_outcome(
        log,
        &prepared.tool_call,
        &result,
        tool_msg.clone(),
        &prepared.tool_use_uuid,
    )
    .await?;

    tracing::info!(
        tool = %prepared.tool_call.name,
        duration_ms,
        is_error = result.is_error,
        result_len = result.model_text().len(),
        "tool execution completed"
    );
    let _ = event_tx
        .send(AgentEvent::ToolExecutionEnd {
            tool_call: prepared.tool_call.clone(),
            result: result.clone(),
        })
        .await;

    Ok(CommittedToolCall {
        tool_call: prepared.tool_call,
        result,
        message: tool_msg,
        tool_use_uuid: prepared.tool_use_uuid,
    })
}

async fn commit_failed_tool_call(
    log: &std::sync::Arc<dyn ConversationLog>,
    prepared: PreparedToolCall,
    error_text: &str,
    event_tx: &AgentEventSender,
) -> Result<(), AgentError> {
    append_error_tool_result(
        log,
        &prepared.tool_call,
        Some(prepared.tool_use_uuid),
        error_text,
        event_tx,
    )
    .await
}

fn tool_result_message(tool_call: &ToolCall, result: &ToolOutput) -> Message {
    Message {
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
    }
}

async fn commit_tool_intent(
    log: &std::sync::Arc<dyn ConversationLog>,
    tool_call: &ToolCall,
    llm_call_uuid: &str,
) -> Result<String, AgentError> {
    let (record, record_id) = tool_intent_record(tool_call, llm_call_uuid);
    log.append_records(vec![record])
        .await
        .map_err(|error| AgentError::from(error))?;
    Ok(record_id)
}

fn tool_intent_record(tool_call: &ToolCall, llm_call_uuid: &str) -> (SessionRecord, String) {
    let mut record = SessionRecord::execution(
        "tool_use",
        EventEmitter::runtime(),
        ExecutionRecord::Intent {
            operation_id: tool_call.id.clone(),
            operation: tool_call.name.clone(),
            input: RecordValue::Inline {
                value: tool_call.arguments.clone(),
            },
        },
    );
    record.parent_record_id = Some(llm_call_uuid.to_string());
    let record_id = record.record_id.clone();
    (record, record_id)
}

async fn commit_tool_outcome(
    log: &std::sync::Arc<dyn ConversationLog>,
    tool_call: &ToolCall,
    result: &ToolOutput,
    message: AgentMessage,
    tool_use_uuid: &str,
) -> Result<(), AgentError> {
    let mut outcome = SessionRecord::execution(
        "tool_outcome",
        EventEmitter::runtime(),
        ExecutionRecord::Outcome {
            operation_id: tool_call.id.clone(),
            success: !result.is_error,
            output: RecordValue::Inline {
                value: serde_json::to_value(result).unwrap_or(serde_json::Value::Null),
            },
        },
    );
    outcome.parent_record_id = Some(tool_use_uuid.to_string());
    let conversation = SessionRecord::conversation(message, Some(tool_use_uuid.to_string()));
    log.append_records(vec![outcome, conversation])
        .await
        .map_err(|error| AgentError::from(error))?;
    Ok(())
}

/// Append an error `ToolResult` for `tool_call` (paired to `parent_uuid`)
/// and emit the matching `ToolExecutionEnd`. Used when the batch aborts, so
/// that a tool_use block committed to the session is never left without a
/// tool_result — a provider re-sent such a history rejects it for having
/// unmatched tool_use ids.
async fn append_error_tool_result(
    log: &std::sync::Arc<dyn ConversationLog>,
    tool_call: &ToolCall,
    parent_uuid: Option<String>,
    error_text: &str,
    event_tx: &AgentEventSender,
) -> Result<(), AgentError> {
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
    let message = AgentMessage::Standard(tool_message);
    if let Some(tool_use_uuid) = parent_uuid {
        commit_tool_outcome(log, tool_call, &result, message, &tool_use_uuid).await?;
    } else {
        log.append_message(message, None)
            .await
            .map_err(AgentError::from)?;
    }
    let _ = event_tx
        .send(AgentEvent::ToolExecutionEnd {
            tool_call: tool_call.clone(),
            result,
        })
        .await;
    Ok(())
}

/// Close out every tool call that had not started yet (the ones after the
/// aborting call) with a typed tool intent + an error ToolResult, so
/// the whole batch's tool_use blocks stay paired even though execution
/// stopped early.
async fn backfill_aborted_tool_results(
    log: &std::sync::Arc<dyn ConversationLog>,
    remaining: &[ToolCall],
    llm_call_uuid: &str,
    error_text: &str,
    event_tx: &AgentEventSender,
) -> Result<(), AgentError> {
    for tool_call in remaining {
        let tool_use_uuid = commit_tool_intent(log, tool_call, llm_call_uuid).await?;
        append_error_tool_result(log, tool_call, Some(tool_use_uuid), error_text, event_tx).await?;
    }
    Ok(())
}

struct ActualToolCall {
    tool: Arc<dyn Tool>,
    cancel: CancellationToken,
    event_tx: AgentEventSender,
    session_id: String,
    workspace: Option<std::path::PathBuf>,
    bus: Option<BusHandle>,
}

#[async_trait]
impl ToolCallFn for ActualToolCall {
    async fn call(
        &self,
        state: &AgentState,
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
