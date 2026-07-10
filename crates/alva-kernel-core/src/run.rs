// INPUT:  crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent, alva_kernel_abi::*
// OUTPUT: pub async fn run_agent()
// POS:    session-centric agent loop. Mid-run steering is NOT a kernel concern —
//         external callers inject messages by appending them to the session directly
//         (typically via an opt-in `PendingPlugin` middleware). Before each LLM
//         call, tool schemas are pre-baked against a `ToolSchemaContext` carrying the
//         bus handle so `Tool::parameters_schema_with` sees live runtime state.
use std::sync::Arc;

use alva_kernel_abi::agent_session::{ComponentDescriptor, EmitterKind};
use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::tool::execution::{
    ToolExecutionContext as AbiToolExecutionContext, ToolOutput as AbiToolOutput,
};
use alva_kernel_abi::tool::schema::ToolSchemaContext;
use alva_kernel_abi::tool::{SearchReadInfo, Tool, ToolDefinition, ToolPermissionResult};
use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock, Message, MessageRole,
    ModelConfig, StreamEvent, ToolCall,
};
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::context_runtime::ContextRuntime;
use crate::event::AgentEvent;
use crate::middleware::{LlmCallFn, MiddlewareError};
use crate::session_events::emit_runtime_event;
use crate::state::{AgentConfig, AgentState};

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
// Schema pre-baking — freeze each tool's schema against the live bus before
// the provider sees it
// ---------------------------------------------------------------------------

/// Transparent `Tool` wrapper whose `parameters_schema()` returns a
/// pre-baked JSON Schema value instead of re-running the inner tool's
/// `parameters_schema` each time it's asked.
///
/// Every provider's `to_*_tools` helper reads `t.parameters_schema()`.
/// That entry point can't see the bus, so runtime-dependent enums
/// (sibling tools, registered `SpawnCommunication` kinds, MCP servers,
/// skills) would otherwise be absent from the schema sent to the model.
///
/// Instead, the kernel run loop generates each tool's schema once per
/// turn via [`Tool::parameters_schema_with`] with a
/// [`ToolSchemaContext`] carrying the config bus, then hands providers a
/// slice of [`PrebakedSchemaTool`]s that replay the baked schema back
/// on `parameters_schema()`. Every other `Tool` method delegates to
/// the inner implementation unchanged.
///
/// Tools that don't override `parameters_schema_with` fall through to
/// their static `parameters_schema()`, so this wrapper is inert for
/// the common case and cheap for everyone else.
struct PrebakedSchemaTool {
    inner: Arc<dyn Tool>,
    schema: serde_json::Value,
}

impl PrebakedSchemaTool {
    fn new(inner: Arc<dyn Tool>, schema: serde_json::Value) -> Self {
        Self { inner, schema }
    }
}

#[async_trait]
impl Tool for PrebakedSchemaTool {
    fn name(&self) -> &str {
        self.inner.name()
    }

    fn description(&self) -> &str {
        self.inner.description()
    }

    fn parameters_schema(&self) -> serde_json::Value {
        self.schema.clone()
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.inner.name().to_string(),
            description: self.inner.description().to_string(),
            parameters: self.schema.clone(),
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn AbiToolExecutionContext,
    ) -> Result<AbiToolOutput, AgentError> {
        self.inner.execute(input, ctx).await
    }

    fn is_concurrency_safe(&self, input: &serde_json::Value) -> bool {
        self.inner.is_concurrency_safe(input)
    }

    fn is_read_only(&self, input: &serde_json::Value) -> bool {
        self.inner.is_read_only(input)
    }

    fn is_destructive(&self, input: &serde_json::Value) -> bool {
        self.inner.is_destructive(input)
    }

    fn manages_own_timeout(&self) -> bool {
        self.inner.manages_own_timeout()
    }

    fn is_search_or_read(&self, input: &serde_json::Value) -> Option<SearchReadInfo> {
        self.inner.is_search_or_read(input)
    }

    fn check_permissions(
        &self,
        input: &serde_json::Value,
        ctx: &dyn AbiToolExecutionContext,
    ) -> ToolPermissionResult {
        self.inner.check_permissions(input, ctx)
    }

    fn user_facing_name(&self, input: &serde_json::Value) -> String {
        self.inner.user_facing_name(input)
    }

    fn max_result_size_chars(&self) -> Option<usize> {
        self.inner.max_result_size_chars()
    }

    fn should_defer(&self) -> bool {
        self.inner.should_defer()
    }

    fn aliases(&self) -> Vec<String> {
        self.inner.aliases()
    }

    fn is_enabled(&self) -> bool {
        self.inner.is_enabled()
    }

    fn tool_prompt(&self) -> String {
        self.inner.tool_prompt()
    }
}

/// Build a fresh set of tool handles whose `parameters_schema()` returns
/// the schema produced by each tool's `parameters_schema_with(&ctx)`.
///
/// Used right before calling `model.stream` / `model.complete` so the
/// provider-facing `to_*_tools` paths pick up dynamic enums sourced from
/// `ctx.bus`. The wrapping is a no-op for tools that don't override
/// `parameters_schema_with` — their static schema is recomputed once and
/// baked in place.
fn bake_tool_schemas(tools: &[Arc<dyn Tool>], bus: Option<&BusHandle>) -> Vec<Arc<dyn Tool>> {
    let ctx = match bus {
        Some(b) => ToolSchemaContext::with_bus(b),
        None => ToolSchemaContext::empty(),
    };
    tools
        .iter()
        .map(|t| -> Arc<dyn Tool> {
            let schema = t.parameters_schema_with(&ctx);
            Arc::new(PrebakedSchemaTool::new(t.clone(), schema))
        })
        .collect()
}

// ---------------------------------------------------------------------------
// LlmCallFn adapter for wrap hooks
// ---------------------------------------------------------------------------

/// Wraps the actual LLM model call as a `LlmCallFn` so it can be passed
/// into middleware `wrap_llm_call` as the `next` callback.
///
/// Internally uses `model.stream()` to emit `MessageUpdate` events in
/// real-time, then assembles the final `Message` from accumulated chunks
/// so the middleware chain still receives a complete `Message`.
///
/// `tools` is expected to already carry context-baked schemas (see
/// [`bake_tool_schemas`]) so the provider's `to_*_tools` path picks up
/// any dynamic enums sourced from the bus via
/// [`Tool::parameters_schema_with`].
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

        // Reasoning / thinking blocks captured from the stream. Anthropic
        // emits one on `content_block_stop` with an attestation signature
        // that MUST be echoed back on the next turn (otherwise 400 errors).
        // We capture them here and splice into the final assistant message.
        let mut reasoning_blocks: Vec<(String, Option<String>)> = Vec::new();

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
                StreamEvent::ToolCallStart { id, name } => {
                    // Anthropic emits the tool name ONLY on `content_block_start`
                    // (→ our ToolCallStart). Subsequent ToolCallDelta carries
                    // the arguments stream but name: None. If we ignored
                    // ToolCallStart, the builder's name stayed "" and the
                    // tool dispatch later reported "Tool not found: ".
                    if !id.is_empty() {
                        let target_index =
                            if let Some(existing) = tool_call_indices.get(&id).copied() {
                                existing
                            } else {
                                let next = tool_call_builders.len();
                                tool_call_builders.push((
                                    id.clone(),
                                    String::new(),
                                    String::new(),
                                ));
                                tool_call_indices.insert(id, next);
                                next
                            };
                        last_tool_call_index = Some(target_index);
                        if !name.is_empty() {
                            if let Some((_, existing_name, _)) =
                                tool_call_builders.get_mut(target_index)
                            {
                                *existing_name = name;
                            }
                        }
                    }
                }
                StreamEvent::ReasoningBlock { text, signature } => {
                    // Authoritative capture of a completed thinking block.
                    // Anthropic's extended thinking requires round-tripping
                    // the full text + signature verbatim on the next turn.
                    reasoning_blocks.push((text, signature));
                }
                StreamEvent::Start
                | StreamEvent::Done
                | StreamEvent::ReasoningDelta { .. }
                | StreamEvent::ToolCallEnd { .. }
                // agent loop doesn't consume stop reason (Message has no stop_reason field; Stop serves the gateway path)
                | StreamEvent::Stop { .. } => {
                    // Boundary signals / UI progress — no builder state to update.
                }
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
            match self
                .model
                .complete(&messages, &tool_refs, &self.model_config)
                .await
            {
                Ok(resp) => {
                    let msg = resp.message;
                    // Emit the fallback result as synthetic events so the UI still sees them
                    for block in &msg.content {
                        let delta = match block {
                            ContentBlock::Text { text } => {
                                StreamEvent::TextDelta { text: text.clone() }
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                StreamEvent::ToolCallDelta {
                                    id: id.clone(),
                                    name: Some(name.clone()),
                                    arguments_delta: input.to_string(),
                                }
                            }
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
        // Order matters for Anthropic: extended thinking requires thinking
        // blocks to precede text / tool_use in the echoed assistant message.
        let mut content_blocks = Vec::new();
        for (text, signature) in reasoning_blocks {
            content_blocks.push(ContentBlock::Reasoning { text, signature });
        }
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

    let agent_id = state.session.session_id().to_string();
    let mut context_runtime = ContextRuntime::new(agent_id.clone());
    context_runtime.bootstrap(config).await;

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
    )
    .await;

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
    )
    .await;

    // 2. Store input messages in session + collect context injections.
    for msg in input {
        state.session.append_message(msg.clone(), None).await;
        config
            .middleware
            .run_input_committed(state, &msg)
            .await
            .map_err(MiddlewareError::into_agent_error)?;
        context_runtime.on_message(config, &msg).await;
    }

    // 3. Main loop
    let mut error: Option<String> = None;
    let result = run_loop(
        state,
        config,
        &cancel,
        &event_tx,
        &run_start_uuid,
        &mut context_runtime,
    )
    .await;

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

    context_runtime.dispose(config).await;

    // --- Session skeleton: run_end ---
    emit_runtime_event(
        &state.session,
        "run_end",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({
            "error": error.clone(),
        })),
    )
    .await;

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
    context_runtime: &mut ContextRuntime,
) -> Result<(), AgentError> {
    let mut total_iterations: u32 = 0;
    let mut _session_input_tokens: u64 = 0;
    let mut _session_output_tokens: u64 = 0;

    // The run loop: LLM call → tool execution, iterating until the model
    // stops requesting tools. Follow-up / steering moved to PendingPlugin,
    // so there is no longer an outer conversation loop — callers re-invoke
    // run_agent for the next user turn.
    loop {
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
        )
        .await;

        // Emit TurnStart
        let _ = event_tx.send(AgentEvent::TurnStart);
        let turn_start = web_time::Instant::now();

        // 3a. Get messages from session (optionally windowed)
        let session_messages = if config.context_window > 0 {
            state.session.recent_messages(config.context_window).await
        } else {
            state.session.messages().await
        };

        let mut system_prompt_buf: Vec<String> = config.system_prompt.clone();
        let working_messages = context_runtime
            .prepare_llm_context(config, &mut system_prompt_buf, session_messages)
            .await;

        // 3b. Build LLM messages: [system_prompt segments] + working_messages
        //
        //     We push ONE Message::system per segment. Adapters decide
        //     how to render — Anthropic emits a `system: [{...}]`
        //     block array with `cache_control: ephemeral` on every
        //     segment except the last (cacheable); OpenAI / Gemini
        //     concat into one string (auto-prefix-cached).
        let mut llm_messages = Vec::new();
        let total_segments = system_prompt_buf.len();
        let total_chars: usize = system_prompt_buf.iter().map(|s| s.len()).sum();
        if total_chars > 0 {
            let head_segment = system_prompt_buf
                .first()
                .map(|s| s.chars().take(200).collect::<String>())
                .unwrap_or_default();
            let tail_segment = system_prompt_buf
                .last()
                .map(|s| {
                    let start = s.len().saturating_sub(200);
                    s[start..].to_string()
                })
                .unwrap_or_default();
            tracing::debug!(
                segments = total_segments,
                total_chars,
                head_first = %head_segment,
                tail_last = %tail_segment,
                "llm_call: system_prompt"
            );
        }
        for segment in &system_prompt_buf {
            if !segment.is_empty() {
                llm_messages.push(Message::system(segment));
            }
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
        //
        // Per-turn observability fields (P2 markers):
        //   * `system_prompt_segments` — cache boundary count (>1 = stable+dynamic split)
        //   * `system_prompt_segment_hashes` — sha256 per segment, lets the user diagnose
        //     "which segment caused a cache miss" by hash diff across turns
        //   * `disable_tools` — whether this call ran without tools
        //   * `tools_count_sent` — actual number of tools in the request body
        //   * `provider_options_applied` — whether vendor `extra_body` was non-empty
        let segment_hashes: Vec<String> = system_prompt_buf
            .iter()
            .map(|seg| {
                use sha2::{Digest, Sha256};
                let h = Sha256::digest(seg.as_bytes());
                format!("{:x}", h).chars().take(16).collect::<String>()
            })
            .collect();
        let provider_options_applied = config
            .model_config
            .extra_body
            .as_ref()
            .map(|m| !m.is_empty())
            .unwrap_or(false);
        let tools_count_sent_for_event = if config.model_config.disable_tools {
            0
        } else {
            state.tools.len()
        };
        let llm_start_uuid = emit_runtime_event(
            &state.session,
            "llm_call_start",
            Some(iteration_start_uuid.clone()),
            Some(serde_json::json!({
                "iteration": total_iterations,
                "message_count": llm_messages.len(),
                "messages": llm_messages,
                "system_prompt_segments": system_prompt_buf.len(),
                "system_prompt_segment_hashes": segment_hashes,
                "disable_tools": config.model_config.disable_tools,
                "tools_count_sent": tools_count_sent_for_event,
                "provider_options_applied": provider_options_applied,
            })),
        )
        .await;
        // Wall-clock start for analytics. Captured here (just before
        // before_llm_call middleware) so the latency includes any
        // pre-call middleware work too. web_time, NOT std::time —
        // std::time::Instant::now() panics on wasm32-unknown-unknown
        // (`:608` uses web_time for the same reason; this line was the
        // regression that made every wasm agent die on its first turn).
        let llm_call_started_at = web_time::Instant::now();

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

        // 3e. Call LLM through wrap_llm_call middleware chain.
        //
        // Pre-bake each tool's JSON Schema against a `ToolSchemaContext`
        // that carries the config bus. Tools that override
        // `Tool::parameters_schema_with` (e.g. `AgentSpawnTool`
        // pulling registered `SpawnCommunication` kinds off the bus)
        // observe the live runtime state here; tools that don't just
        // see their usual static schema cached once per turn.
        //
        // `model_config.disable_tools = true` short-circuits to an
        // empty list — used when the user (or runtime probe) has
        // marked the active model as not supporting function
        // calling. The provider then omits the `tools` field from
        // the request entirely (no empty array; AMP / pi-mono
        // behavior).
        let baked_tools = if config.model_config.disable_tools {
            tracing::debug!("model_config.disable_tools=true; skipping all tool injection");
            Vec::new()
        } else {
            bake_tool_schemas(&state.tools, config.bus.as_ref())
        };
        let actual_call = ActualLlmCall {
            model: state.model.clone(),
            tools: baked_tools,
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

        // Analytics: emit a structured LlmCall event for telemetry sinks.
        // No-op if no AnalyticsSink is registered on the bus.
        if let Some(sink) = config
            .bus
            .as_ref()
            .and_then(|b| b.get::<dyn alva_kernel_abi::AnalyticsSink>())
        {
            let usage = response.usage.as_ref();
            sink.record(alva_kernel_abi::AnalyticsEvent::LlmCall {
                session_id: state.session.session_id().to_string(),
                provider: state.model.provider_id().to_string(),
                model: state.model.model_id().to_string(),
                input_tokens: usage.map(|u| u.input_tokens).unwrap_or(0),
                output_tokens: usage.map(|u| u.output_tokens).unwrap_or(0),
                cache_read: usage.and_then(|u| u.cache_read_input_tokens).unwrap_or(0),
                cache_write: usage
                    .and_then(|u| u.cache_creation_input_tokens)
                    .unwrap_or(0),
                cost_usd: 0.0,
                latency_ms: llm_call_started_at.elapsed().as_millis() as u64,
                // The analytics ABI field is std::time::SystemTime, whose
                // `now()` panics on wasm32. Take "now" from web_time and
                // rebase it onto the std type — SystemTime ARITHMETIC is
                // wasm-safe, only ::now() is not. On native web_time is a
                // re-export of std, so this is byte-identical there.
                ts: std::time::SystemTime::UNIX_EPOCH
                    + web_time::SystemTime::now()
                        .duration_since(web_time::SystemTime::UNIX_EPOCH)
                        .unwrap_or_default(),
            });
        }

        // 3g. Store response in session + fire ContextHooks::on_message
        let response_msg = AgentMessage::Standard(response.clone());
        state
            .session
            .append_message(response_msg.clone(), Some(llm_start_uuid.clone()))
            .await;
        context_runtime.on_message(config, &response_msg).await;

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
            context_runtime.after_turn(config).await;
            emit_runtime_event(
                &state.session,
                "iteration_end",
                Some(iteration_start_uuid.clone()),
                None,
            )
            .await;
            break;
        }

        let committed_tools = crate::tool_batch::ToolBatchCoordinator::new()
            .execute_batch(
                state,
                config,
                cancel.clone(),
                &tool_calls,
                llm_start_uuid.clone(),
                event_tx.clone(),
            )
            .await?;
        for committed in committed_tools {
            context_runtime.on_message(config, &committed.message).await;
        }

        // 3l. Emit TurnEnd + ContextHooks::after_turn
        tracing::info!(
            turn = total_iterations,
            duration_ms = turn_start.elapsed().as_millis() as u64,
            tool_calls = tool_calls.len(),
            "turn completed (with tool calls)"
        );
        let _ = event_tx.send(AgentEvent::TurnEnd);
        context_runtime.after_turn(config).await;

        // Session skeleton: end of iteration (tool-calls path).
        // Any mid-run user interjection lands via an extension that
        // runs at `before_llm_call`, not here.
        emit_runtime_event(
            &state.session,
            "iteration_end",
            Some(iteration_start_uuid.clone()),
            None,
        )
        .await;
    }

    // The loop ended with no more tool calls → agent run is done. Callers
    // that want to "continue the conversation" invoke `run_agent` again
    // with the next user input.

    Ok(())
}
