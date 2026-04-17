// INPUT:  crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent, alva_kernel_abi::*
// OUTPUT: pub async fn run_agent()
// POS:    session-centric agent loop. Mid-run steering is NOT a kernel concern —
//         external callers inject messages by appending them to the session directly
//         (typically via an opt-in `PendingExtension` middleware).
use std::sync::Arc;

use alva_kernel_abi::agent_session::{
    AgentSession, ComponentDescriptor, EmitterKind, EventEmitter, ScopedSession, SessionEvent,
};
use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock, Message, MessageRole,
    ModelConfig, StreamEvent, ToolCall, ToolOutput,
};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::event::AgentEvent;
use crate::middleware::{LlmCallFn, MiddlewareError, ToolCallFn};
use crate::runtime_context::RuntimeExecutionContext;
use crate::state::{AgentConfig, AgentState};

// ---------------------------------------------------------------------------
// Session skeleton event helper
// ---------------------------------------------------------------------------

/// Append a runtime-emitted event to the session. The emitter is always
/// `EventEmitter::runtime()`; callers only set event_type, parent_uuid, and
/// data. Returns the uuid of the appended event so callers can use it as
/// a parent for subsequent events.
async fn emit_runtime_event(
    session: &std::sync::Arc<dyn AgentSession>,
    event_type: &str,
    parent_uuid: Option<String>,
    data: Option<serde_json::Value>,
) -> String {
    let mut event = SessionEvent::new_runtime(event_type);
    event.parent_uuid = parent_uuid;
    event.data = data;
    let uuid = event.uuid.clone();
    session.append(event).await;
    uuid
}

// ---------------------------------------------------------------------------
// ContextHooks integration helpers (Phase 4)
//
// These free functions are no-ops when `config.context_system` is None,
// so the run loop's behavior is unchanged for callers that don't opt in.
// `assemble` and `on_budget_exceeded` are intentionally NOT wired here —
// they require a ContextEntry ↔ Message translation layer that hasn't been
// designed yet. Only the hooks that operate directly on AgentMessage or
// pure lifecycle events are wired.
// ---------------------------------------------------------------------------

async fn fire_context_bootstrap(config: &AgentConfig, agent_id: &str) {
    if let Some(cs) = config.context_system.as_ref() {
        if let Err(e) = cs.hooks().bootstrap(cs.handle(), agent_id).await {
            tracing::warn!(error = ?e, "context bootstrap failed");
        }
    }
}

async fn fire_context_on_message(
    config: &AgentConfig,
    agent_id: &str,
    message: &AgentMessage,
) -> Vec<alva_kernel_abi::scope::context::Injection> {
    if let Some(cs) = config.context_system.as_ref() {
        cs.hooks().on_message(cs.handle(), agent_id, message).await
    } else {
        Vec::new()
    }
}

async fn fire_context_after_turn(config: &AgentConfig, agent_id: &str) {
    if let Some(cs) = config.context_system.as_ref() {
        cs.hooks().after_turn(cs.handle(), agent_id).await;
    }
}

async fn fire_context_dispose(config: &AgentConfig) {
    if let Some(cs) = config.context_system.as_ref() {
        if let Err(e) = cs.hooks().dispose().await {
            tracing::warn!(error = ?e, "context dispose failed");
        }
    }
}

/// Estimate total tokens for a working message list. Uses a bus-registered
/// `TokenCounter` if available, otherwise a 4-chars-per-token heuristic.
/// 4 token of overhead per message accounts for role / separator framing.
fn estimate_message_tokens(
    messages: &[AgentMessage],
    bus: Option<&alva_kernel_abi::BusHandle>,
) -> usize {
    let counter = bus.and_then(|b| b.get::<dyn alva_kernel_abi::TokenCounter>());
    messages
        .iter()
        .map(|m| {
            let text = match m {
                AgentMessage::Standard(msg) => msg.text_content(),
                AgentMessage::Steering(msg) => msg.text_content(),
                AgentMessage::FollowUp(msg) => msg.text_content(),
                AgentMessage::Marker(_) => String::new(),
                AgentMessage::Extension { data, .. } => data.to_string(),
            };
            let tokens = match &counter {
                Some(c) => c.count_tokens(&text),
                None => text.len() / 4,
            };
            tokens + 4
        })
        .sum()
}

/// Build a synthetic `ContextSnapshot` to hand to `on_budget_exceeded`
/// when the kernel itself decided the budget was exceeded (rather than
/// the ContextStore tracking it).
fn build_budget_snapshot(
    total_tokens: usize,
    budget: usize,
) -> alva_kernel_abi::scope::context::ContextSnapshot {
    alva_kernel_abi::scope::context::ContextSnapshot {
        total_tokens,
        budget_tokens: budget,
        model_window: budget,
        usage_ratio: if budget == 0 {
            1.0
        } else {
            total_tokens as f32 / budget as f32
        },
        layer_breakdown: std::collections::HashMap::new(),
        entries: Vec::new(),
        recent_tool_patterns: Vec::new(),
    }
}

fn placeholder_assistant_message(message_id: &str) -> AgentMessage {
    AgentMessage::Standard(Message {
        id: message_id.to_string(),
        role: MessageRole::Assistant,
        content: vec![],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

// ---------------------------------------------------------------------------
// LlmCallFn / ToolCallFn adapters for wrap hooks
// ---------------------------------------------------------------------------

/// Wraps the actual LLM model call as a `LlmCallFn` so it can be passed
/// into middleware `wrap_llm_call` as the `next` callback.
///
/// Internally uses `model.stream()` to emit `MessageUpdate` events in
/// real-time, then assembles the final `Message` from accumulated chunks
/// so the middleware chain still receives a complete `Message`.
struct ActualLlmCall {
    model: Arc<dyn LanguageModel>,
    tools: Vec<Arc<dyn Tool>>,
    model_config: ModelConfig,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    message_id: String,
}

#[async_trait]
impl LlmCallFn for ActualLlmCall {
    async fn call(
        &self,
        _state: &mut AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError> {
        let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
        let mut stream = self.model.stream(&messages, &tool_refs, &self.model_config);

        let mut text_content = String::new();
        let mut usage = None;
        let mut last_tool_call_index = None;
        let mut event_count: u32 = 0;

        // Track in-progress tool calls in appearance order while allowing
        // providers to repeat the same tool-call id across multiple deltas.
        let mut tool_call_builders: Vec<(String, String, String)> = Vec::new();
        let mut tool_call_indices: std::collections::HashMap<String, usize> =
            std::collections::HashMap::new();

        while let Some(event) = stream.next().await {
            event_count += 1;
            // Build a placeholder message for the MessageUpdate envelope.
            let agent_msg = AgentMessage::Standard(Message {
                id: self.message_id.clone(),
                role: MessageRole::Assistant,
                content: vec![],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                message: agent_msg,
                delta: event.clone(),
            });

            match event {
                StreamEvent::TextDelta { text } => {
                    text_content.push_str(&text);
                }
                StreamEvent::ToolCallDelta {
                    id,
                    name,
                    arguments_delta,
                } => {
                    let target_index = if !id.is_empty() {
                        if let Some(existing_index) = tool_call_indices.get(&id).copied() {
                            existing_index
                        } else {
                            let next_index = tool_call_builders.len();
                            tool_call_builders.push((id.clone(), String::new(), String::new()));
                            tool_call_indices.insert(id, next_index);
                            next_index
                        }
                    } else if let Some(last_index) = last_tool_call_index {
                        last_index
                    } else {
                        continue;
                    };

                    last_tool_call_index = Some(target_index);

                    if let Some((_, existing_name, existing_arguments)) =
                        tool_call_builders.get_mut(target_index)
                    {
                        if let Some(name) = name {
                            if !name.is_empty() {
                                *existing_name = name;
                            }
                        }
                        existing_arguments.push_str(&arguments_delta);
                    }
                }
                StreamEvent::Usage(u) => {
                    usage = Some(u);
                }
                StreamEvent::Error(e) => {
                    return Err(AgentError::LlmError(e));
                }
                StreamEvent::Start | StreamEvent::Done | StreamEvent::ReasoningDelta { .. } => {}
            }
        }

        // Check if streaming produced anything useful
        let text_len = text_content.len();
        let tool_call_count = tool_call_builders.len();

        if text_len == 0 && tool_call_count == 0 {
            // Stream returned nothing — fallback to non-streaming complete()
            tracing::warn!(
                model = %self.model.model_id(),
                stream_events = event_count,
                "LLM stream produced empty response — falling back to non-streaming"
            );

            let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
            match self.model.complete(&messages, &tool_refs, &self.model_config).await {
                Ok(resp) => {
                    let msg = resp.message;
                    // Emit the fallback result as synthetic events so the UI still sees them
                    for block in &msg.content {
                        let delta = match block {
                            ContentBlock::Text { text } => StreamEvent::TextDelta { text: text.clone() },
                            ContentBlock::ToolUse { id, name, input } => StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                name: Some(name.clone()),
                                arguments_delta: input.to_string(),
                            },
                            _ => continue,
                        };
                        let agent_msg = AgentMessage::Standard(Message {
                            id: self.message_id.clone(),
                            role: MessageRole::Assistant,
                            content: vec![],
                            tool_call_id: None,
                            usage: None,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        });
                        let _ = self.event_tx.send(AgentEvent::MessageUpdate {
                            message: agent_msg,
                            delta,
                        });
                    }

                    tracing::info!(
                        model = %self.model.model_id(),
                        content_blocks = msg.content.len(),
                        has_usage = msg.usage.is_some(),
                        "non-streaming fallback succeeded"
                    );
                    return Ok(msg);
                }
                Err(e) => {
                    tracing::error!(error = %e, "non-streaming fallback also failed");
                    return Err(e);
                }
            }
        }

        if usage.is_none() {
            tracing::debug!(
                model = %self.model.model_id(),
                stream_events = event_count,
                text_len,
                "LLM stream completed without usage data"
            );
        }

        // Assemble the final Message from accumulated stream chunks.
        let mut content_blocks = Vec::new();
        if !text_content.is_empty() {
            content_blocks.push(ContentBlock::Text { text: text_content });
        }

        for (id, name, args_str) in tool_call_builders {
            let input: Value = serde_json::from_str(&args_str).map_err(|error| {
                AgentError::LlmError(format!(
                    "invalid tool arguments for tool call '{id}' ({name}): {error}"
                ))
            })?;
            content_blocks.push(ContentBlock::ToolUse { id, name, input });
        }

        Ok(Message {
            id: self.message_id.clone(),
            role: MessageRole::Assistant,
            content: content_blocks,
            tool_call_id: None,
            usage,
            timestamp: chrono::Utc::now().timestamp_millis(),
        })
    }
}

/// Wraps the actual tool execution as a `ToolCallFn` so it can be passed
/// into middleware `wrap_tool_call` as the `next` callback.
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
        // No timeout in the kernel — use ToolTimeoutMiddleware (wrap_tool_call) to add one.
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

/// agent loop — session-centric with middleware hooks.
///
/// Messages are never stored in local variables across iterations;
/// instead, every iteration reads the full history from `state.session`.
pub async fn run_agent(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: CancellationToken,
    input: Vec<AgentMessage>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError> {
    state.extensions.insert(cancel.clone());

    // 1. Lifecycle: on_agent_start
    config
        .middleware
        .run_on_agent_start(state)
        .await
        .map_err(MiddlewareError::into_agent_error)?;

    // 1b. ContextHooks: bootstrap (no-op when context_system is None)
    let agent_id = state.session.session_id().to_string();
    fire_context_bootstrap(config, &agent_id).await;

    // Emit AgentStart
    let _ = event_tx.send(AgentEvent::AgentStart);

    // --- Session skeleton: run_start ---
    let run_start_uuid = emit_runtime_event(
        &state.session,
        "run_start",
        None,
        Some(serde_json::json!({
            "agent_id": agent_id.clone(),
            "max_iterations": config.max_iterations,
        })),
    ).await;

    // --- Session skeleton: component_registry ---
    // Collect descriptors for every tool and middleware in this run.
    let mut components: Vec<ComponentDescriptor> = Vec::new();
    for tool in &state.tools {
        components.push(ComponentDescriptor {
            kind: EmitterKind::Tool,
            id: tool.name().to_string(),
            name: tool.name().to_string(),
        });
    }
    for mw_name in config.middleware.names() {
        components.push(ComponentDescriptor {
            kind: EmitterKind::Middleware,
            id: mw_name.clone(),
            name: mw_name,
        });
    }
    emit_runtime_event(
        &state.session,
        "component_registry",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({ "components": components })),
    ).await;

    // 2. Store input messages in session + fire on_message for each.
    //    Injections returned here are dropped — input messages typically arrive
    //    before any LLM call, and bootstrap is the proper place for plugins to
    //    seed initial context. If a future use case needs input-message-driven
    //    injections, plumb them into run_loop via a parameter.
    for msg in input {
        state.session.append_message(msg.clone(), None).await;
        let _ = fire_context_on_message(config, &agent_id, &msg).await;
    }

    // 3. Main loop
    let mut error: Option<String> = None;
    let result = run_loop(state, config, &cancel, &event_tx, &run_start_uuid).await;

    if let Err(ref e) = result {
        error = Some(e.to_string());
    }

    // 4. Lifecycle: on_agent_end
    if let Err(e) = config
        .middleware
        .run_on_agent_end(state, error.as_deref())
        .await
    {
        tracing::warn!(error = %e, "on_agent_end middleware failed");
    }

    // 4b. ContextHooks: dispose (no-op when context_system is None)
    fire_context_dispose(config).await;

    // --- Session skeleton: run_end ---
    emit_runtime_event(
        &state.session,
        "run_end",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({
            "error": error.clone(),
        })),
    ).await;

    // 5. Emit AgentEnd
    let _ = event_tx.send(AgentEvent::AgentEnd {
        error: error.clone(),
    });

    result
}

/// Inner loop extracted so we can capture the error cleanly for lifecycle hooks.
///
/// Double-loop structure:
/// - **Outer loop**: processes follow-up messages after the inner loop finishes naturally.
/// - **Inner loop**: LLM calls + tool execution + steering injection.
async fn run_loop(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    run_start_uuid: &str,
) -> Result<(), AgentError> {
    let mut total_iterations: u32 = 0;
    let mut _session_input_tokens: u64 = 0;
    let mut _session_output_tokens: u64 = 0;
    // Stable identifier handed to ContextHooks. Same string for the entire run
    // so plugins can correlate hook calls. Cloned because we'll borrow `state`
    // mutably later for middleware.
    let agent_id = state.session.session_id().to_string();
    // Buffer for Injections returned by ContextHooks::on_message. Drained and
    // applied at the start of each LLM-call cycle (3a*) before assemble runs.
    let mut pending_injections: Vec<alva_kernel_abi::scope::context::Injection> = Vec::new();

    // Outer loop: processes follow-up messages
    'outer: loop {
        // Inner loop: LLM calls + tool execution + steering checks
        'inner: loop {
            if cancel.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            if total_iterations >= config.max_iterations {
                tracing::warn!(
                    max_iterations = config.max_iterations,
                    "agent loop exhausted max_iterations without finishing"
                );
                return Err(AgentError::MaxIterations(config.max_iterations));
            }
            total_iterations += 1;

            // Session skeleton: iteration boundary
            let iteration_start_uuid = emit_runtime_event(
                &state.session,
                "iteration_start",
                Some(run_start_uuid.to_string()),
                Some(serde_json::json!({ "iteration": total_iterations })),
            ).await;

            // Emit TurnStart
            let _ = event_tx.send(AgentEvent::TurnStart);
            let turn_start = web_time::Instant::now();

            // 3a. Get messages from session (optionally windowed)
            let session_messages = if config.context_window > 0 {
                state.session.recent_messages(config.context_window).await
            } else {
                state.session.messages().await
            };

            // 3a*. ContextHooks: apply pending injections + run assemble hook.
            //      When context_system is None, this collapses to a passthrough
            //      and the behavior matches the pre-Phase-4 path bit-for-bit.
            let mut system_prompt_buf = config.system_prompt.clone();
            let mut working_messages: Vec<AgentMessage> = session_messages;
            if !pending_injections.is_empty() {
                alva_kernel_abi::scope::context::apply_injections(
                    std::mem::take(&mut pending_injections),
                    &mut system_prompt_buf,
                    &mut working_messages,
                );
            }
            if let Some(cs) = config.context_system.as_ref() {
                use alva_kernel_abi::scope::context::{ContextEntry, ContextLayer, ContextMetadata};
                let entries: Vec<ContextEntry> = working_messages
                    .into_iter()
                    .map(|m| {
                        let id = match &m {
                            AgentMessage::Standard(msg) => msg.id.clone(),
                            _ => uuid::Uuid::new_v4().to_string(),
                        };
                        ContextEntry {
                            id,
                            message: m,
                            metadata: ContextMetadata::new(ContextLayer::RuntimeInject),
                        }
                    })
                    .collect();
                let assembled = cs
                    .hooks()
                    .assemble(cs.handle(), &agent_id, entries, /*token_budget*/ 0)
                    .await;
                working_messages = assembled.into_iter().map(|e| e.message).collect();
            }

            // 3a***. ContextHooks: token-budget check + on_budget_exceeded.
            //        Only runs when both context_system and context_token_budget
            //        are set. Estimate uses bus TokenCounter when available, else
            //        a 4-chars-per-token heuristic.
            if let (Some(cs), Some(budget)) =
                (config.context_system.as_ref(), config.context_token_budget)
            {
                let total_tokens = estimate_message_tokens(&working_messages, config.bus.as_ref());
                if total_tokens > budget {
                    let snapshot = build_budget_snapshot(total_tokens, budget);
                    let actions = cs
                        .hooks()
                        .on_budget_exceeded(cs.handle(), &agent_id, &snapshot)
                        .await;
                    alva_kernel_abi::scope::context::apply_compressions(
                        actions,
                        &mut working_messages,
                        cs.handle(),
                        &agent_id,
                    )
                    .await;
                }
            }

            // 3b. Build LLM messages: [system_prompt] + working_messages (only Standard)
            let mut llm_messages = Vec::new();
            if !system_prompt_buf.is_empty() {
                llm_messages.push(Message::system(&system_prompt_buf));
            }
            for msg in &working_messages {
                // Only Standard messages are sent to the LLM.
                // Steering and FollowUp are normalized to Standard before
                // entering the session (see delegate injection below).
                if let AgentMessage::Standard(m) = msg {
                    llm_messages.push(m.clone());
                }
            }

            // Session skeleton: llm_call_start — emitted BEFORE before_llm_call
            // middleware so that any events emitted by middleware (loop_detection,
            // compaction, etc.) land between llm_call_start and llm_call_end in
            // causal order (spec §9 timing contract).
            // message_count reflects the pre-middleware message list; the
            // middleware may shrink or grow llm_messages, but the skeleton event
            // records the count at the point the LLM turn was initiated.
            //
            // llm_call_start carries the full messages list so projection-based consumers
            // (eval, debug) can show exactly what was sent to the model for this turn.
            let llm_start_uuid = emit_runtime_event(
                &state.session,
                "llm_call_start",
                Some(iteration_start_uuid.clone()),
                Some(serde_json::json!({
                    "iteration": total_iterations,
                    "message_count": llm_messages.len(),
                    "messages": llm_messages,
                })),
            ).await;

            // 3c. Middleware: before_llm_call
            config
                .middleware
                .run_before_llm_call(state, &mut llm_messages)
                .await
                .map_err(MiddlewareError::into_agent_error)?;

            // 3d. Emit MessageStart before the LLM call
            let message_id = uuid::Uuid::new_v4().to_string();
            let placeholder_msg = placeholder_assistant_message(&message_id);
            let _ = event_tx.send(AgentEvent::MessageStart {
                message: placeholder_msg.clone(),
            });

            // 3e. Call LLM through wrap_llm_call middleware chain
            let actual_call = ActualLlmCall {
                model: state.model.clone(),
                tools: state.tools.clone(),
                model_config: config.model_config.clone(),
                event_tx: event_tx.clone(),
                message_id,
            };
            let mut response = match config
                .middleware
                .run_wrap_llm_call(state, llm_messages, &actual_call)
                .await
            {
                Ok(response) => response,
                Err(error) => {
                    let agent_error = error.into_agent_error();
                    let _ = event_tx.send(AgentEvent::MessageError {
                        message: placeholder_msg.clone(),
                        error: agent_error.to_string(),
                    });
                    return Err(agent_error);
                }
            };

            // 3f. Middleware: after_llm_call
            if let Err(error) = config
                .middleware
                .run_after_llm_call(state, &mut response)
                .await
            {
                let agent_error = error.into_agent_error();
                let _ = event_tx.send(AgentEvent::MessageError {
                    message: AgentMessage::Standard(response.clone()),
                    error: agent_error.to_string(),
                });
                return Err(agent_error);
            }

            // Session skeleton: llm_call_end.
            // Carries token usage for this turn — basic in/out counts plus
            // cache stats for providers that report them (e.g. Anthropic).
            // Absent fields (e.g. OpenAI cache) serialize as null, not 0,
            // so consumers can distinguish "unknown" from "zero".
            emit_runtime_event(
                &state.session,
                "llm_call_end",
                Some(llm_start_uuid.clone()),
                Some(serde_json::json!({
                    "input_tokens": response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                    "output_tokens": response.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                    "cache_creation_input_tokens": response.usage.as_ref().and_then(|u| u.cache_creation_input_tokens),
                    "cache_read_input_tokens": response.usage.as_ref().and_then(|u| u.cache_read_input_tokens),
                })),
            ).await;

            // 3g. Store response in session + fire ContextHooks::on_message
            let response_msg = AgentMessage::Standard(response.clone());
            state.session.append_message(response_msg.clone(), Some(llm_start_uuid.clone())).await;
            pending_injections.extend(
                fire_context_on_message(config, &agent_id, &response_msg).await,
            );

            // 3h. Track token usage from this turn
            if let Some(ref usage) = response.usage {
                _session_input_tokens += usage.input_tokens as u64;
                _session_output_tokens += usage.output_tokens as u64;
            }

            // 3i. Emit MessageEnd with the complete response
            // (MessageStart was emitted before the LLM call; MessageUpdate
            // events were emitted during streaming inside ActualLlmCall.)
            let agent_msg = AgentMessage::Standard(response.clone());
            let _ = event_tx.send(AgentEvent::MessageEnd { message: agent_msg });

            // 3i. Extract tool_calls from response
            let tool_calls: Vec<ToolCall> = response
                .content
                .iter()
                .filter_map(|block| {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        Some(ToolCall {
                            id: id.clone(),
                            name: name.clone(),
                            arguments: input.clone(),
                        })
                    } else {
                        None
                    }
                })
                .collect();

            // 3j. If no tool_calls, emit TurnEnd and break inner (natural finish — check follow-ups)
            if tool_calls.is_empty() {
                tracing::info!(
                    turn = total_iterations,
                    duration_ms = turn_start.elapsed().as_millis() as u64,
                    tool_calls = 0,
                    "turn completed (no tool calls)"
                );
                let _ = event_tx.send(AgentEvent::TurnEnd);
                fire_context_after_turn(config, &agent_id).await;
                emit_runtime_event(
                    &state.session,
                    "iteration_end",
                    Some(iteration_start_uuid.clone()),
                    None,
                ).await;
                break 'inner;
            }

            // 3k. Execute each tool_call sequentially.
            // NOTE: concurrent execution for is_concurrency_safe() tools is
            // deferred — it requires refactoring tool execution to not hold
            // &mut AgentState across the await boundary.
            for tool_call in &tool_calls {
                if cancel.is_cancelled() {
                    return Err(AgentError::Cancelled);
                }

                // Session skeleton: tool_use
                let tool_use_uuid = emit_runtime_event(
                    &state.session,
                    "tool_use",
                    Some(llm_start_uuid.clone()),
                    Some(serde_json::json!({
                        "tool_name": tool_call.name.clone(),
                        "tool_call_id": tool_call.id.clone(),
                    })),
                ).await;

                // Emit ToolExecutionStart
                let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                    tool_call: tool_call.clone(),
                });
                let tool_start = web_time::Instant::now();

                // Find tool by name — clone the Arc so we don't hold an immutable
                // borrow on state.tools across the mutable middleware calls.
                let tool = state
                    .tools
                    .iter()
                    .find(|t| t.name() == tool_call.name)
                    .cloned();

                // Middleware: before_tool_call
                let before_result = config
                    .middleware
                    .run_before_tool_call(state, tool_call)
                    .await;

                let mut result = match before_result {
                    Err(MiddlewareError::Blocked { reason }) => {
                        // If blocked, make an error result
                        ToolOutput::error(format!("Tool call blocked: {}", reason))
                    }
                    Err(e) => {
                        return Err(e.into_agent_error());
                    }
                    Ok(()) => {
                        // Execute the tool through wrap_tool_call middleware chain (with timeout)
                        match tool {
                            Some(ref t) => {
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
                            None => {
                                ToolOutput::error(format!("Tool not found: {}", tool_call.name))
                            }
                        }
                    }
                };

                // Middleware: after_tool_call
                config
                    .middleware
                    .run_after_tool_call(state, tool_call, &mut result)
                    .await
                    .map_err(MiddlewareError::into_agent_error)?;

                // Build Tool message and append to session.
                // Single tool_result event via append_message, linked to its tool_use
                // via parent_uuid. Projection reads this event's `data` (serialized
                // AgentMessage containing ToolOutput) for content + is_error, and computes
                // duration_ms from the timestamp delta against the tool_use event.
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
                state.session.append_message(tool_msg.clone(), Some(tool_use_uuid.clone())).await;
                pending_injections.extend(
                    fire_context_on_message(config, &agent_id, &tool_msg).await,
                );

                let tool_duration_ms = tool_start.elapsed().as_millis() as u64;
                tracing::info!(
                    tool = %tool_call.name,
                    duration_ms = tool_duration_ms,
                    is_error = result.is_error,
                    result_len = result.model_text().len(),
                    "tool execution completed"
                );

                // Emit ToolExecutionEnd
                let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                    tool_call: tool_call.clone(),
                    result,
                });
            }

            // 3l. Emit TurnEnd + ContextHooks::after_turn
            tracing::info!(
                turn = total_iterations,
                duration_ms = turn_start.elapsed().as_millis() as u64,
                tool_calls = tool_calls.len(),
                "turn completed (with tool calls)"
            );
            let _ = event_tx.send(AgentEvent::TurnEnd);
            fire_context_after_turn(config, &agent_id).await;

            // Session skeleton: end of iteration (tool-calls path).
            // Any mid-run user interjection lands via an extension that
            // runs at `before_llm_call`, not here.
            emit_runtime_event(
                &state.session,
                "iteration_end",
                Some(iteration_start_uuid.clone()),
                None,
            ).await;
        }

        // Inner loop ended with no more tool calls → agent run is done.
        // Callers that want to "continue the conversation" invoke
        // `run_agent` again with the next user input.
        break 'outer;
    }

    Ok(())
}

