// INPUT:  alva_kernel_abi::{LanguageModel, Tool, ToolSchemaContext, ...}, crate::{event, middleware, state}
// OUTPUT: pub(crate) bake_tool_schemas, pub(crate) ActualLlmCall, compact durable stream timing events
// POS:    LLM-call preparation and execution: prebake dynamic tool schemas, then wrap the real
//         model.stream/complete as an LlmCallFn so middleware can interpose through wrap_llm_call;
//         records first-output and stream-summary metadata without persisting every token delta.

use std::sync::Arc;

use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::tool::execution::{
    ToolExecutionContext as AbiToolExecutionContext, ToolOutput as AbiToolOutput,
};
use alva_kernel_abi::tool::schema::ToolSchemaContext;
use alva_kernel_abi::tool::{EffectDescription, Tool};
use alva_kernel_abi::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock, Message, MessageRole,
    ModelConfig, StreamEvent, StreamMessageAccumulator,
};
use async_trait::async_trait;
use tokio_stream::StreamExt;

use crate::event::{AgentEvent, AgentEventSender};
use crate::middleware::LlmCallFn;
use crate::session_events::emit_runtime_event;
use crate::state::AgentState;

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

    fn parameters_schema(&self, _ctx: &ToolSchemaContext) -> serde_json::Value {
        self.schema.clone()
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn AbiToolExecutionContext,
    ) -> Result<AbiToolOutput, AgentError> {
        self.inner.execute(input, ctx).await
    }

    fn scheduling(&self, input: &serde_json::Value) -> alva_kernel_abi::Scheduling {
        self.inner.scheduling(input)
    }

    fn is_read_only(&self, input: &serde_json::Value) -> bool {
        self.inner.is_read_only(input)
    }

    fn describe_effect(&self, input: &serde_json::Value) -> EffectDescription {
        self.inner.describe_effect(input)
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
pub(crate) fn bake_tool_schemas(
    tools: &[Arc<dyn Tool>],
    bus: Option<&BusHandle>,
) -> Vec<Arc<dyn Tool>> {
    let ctx = match bus {
        Some(b) => ToolSchemaContext::with_bus(b),
        None => ToolSchemaContext::empty(),
    };
    tools
        .iter()
        .map(|t| -> Arc<dyn Tool> {
            let schema = t.parameters_schema(&ctx);
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
pub(crate) struct ActualLlmCall {
    pub(crate) model: Arc<dyn LanguageModel>,
    pub(crate) tools: Vec<Arc<dyn Tool>>,
    pub(crate) model_config: ModelConfig,
    pub(crate) event_tx: AgentEventSender,
    pub(crate) message_id: String,
    pub(crate) session: Arc<dyn alva_kernel_abi::agent_session::AgentSession>,
    /// Causal parent for compact durable stream-observation events.
    pub(crate) llm_call_id: String,
    /// Peer handle on the run's cancellation token. Consumed inside the
    /// streaming loop so a cancel that arrives while we're blocked on the
    /// next chunk tears the connection down mid-flight, instead of the
    /// caller having to wait for the whole LLM response to finish before
    /// the between-iterations check in `run_loop` can react.
    pub(crate) cancel: CancellationToken,
}

struct StreamObservation {
    started_at: web_time::Instant,
    event_count: u32,
    visible_event_count: u32,
    text_delta_count: u32,
    reasoning_delta_count: u32,
    tool_delta_count: u32,
    first_event_ms: Option<u64>,
    first_output_ms: Option<u64>,
    first_token_ms: Option<u64>,
}

impl StreamObservation {
    fn new() -> Self {
        Self {
            started_at: web_time::Instant::now(),
            event_count: 0,
            visible_event_count: 0,
            text_delta_count: 0,
            reasoning_delta_count: 0,
            tool_delta_count: 0,
            first_event_ms: None,
            first_output_ms: None,
            first_token_ms: None,
        }
    }

    /// Observe metadata only. Token/reasoning/tool contents are intentionally
    /// excluded from the durable stream envelope.
    fn observe(&mut self, event: &StreamEvent) -> Option<(&'static str, u64)> {
        let elapsed = self.started_at.elapsed().as_millis() as u64;
        self.event_count = self.event_count.saturating_add(1);
        self.first_event_ms.get_or_insert(elapsed);

        let (kind, visible, token) = match event {
            StreamEvent::TextDelta { text } if !text.is_empty() => {
                self.text_delta_count = self.text_delta_count.saturating_add(1);
                ("text", true, true)
            }
            StreamEvent::ReasoningDelta { text } if !text.is_empty() => {
                self.reasoning_delta_count = self.reasoning_delta_count.saturating_add(1);
                ("reasoning", true, true)
            }
            StreamEvent::ToolCallStart { .. } => {
                self.tool_delta_count = self.tool_delta_count.saturating_add(1);
                ("tool_call", true, false)
            }
            StreamEvent::ToolCallDelta {
                arguments_delta, ..
            } if !arguments_delta.is_empty() => {
                self.tool_delta_count = self.tool_delta_count.saturating_add(1);
                ("tool_call", true, false)
            }
            _ => (stream_event_kind(event), false, false),
        };

        if token {
            self.first_token_ms.get_or_insert(elapsed);
        }
        if visible {
            self.visible_event_count = self.visible_event_count.saturating_add(1);
            if self.first_output_ms.is_none() {
                self.first_output_ms = Some(elapsed);
                return Some((kind, elapsed));
            }
        }
        None
    }

    fn duration_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

fn stream_event_kind(event: &StreamEvent) -> &'static str {
    match event {
        StreamEvent::Start => "start",
        StreamEvent::TextDelta { .. } => "text",
        StreamEvent::ReasoningDelta { .. } => "reasoning",
        StreamEvent::ReasoningBlock { .. } => "reasoning_block",
        StreamEvent::ProviderState { .. } => "provider_state",
        StreamEvent::ToolCallStart { .. }
        | StreamEvent::ToolCallDelta { .. }
        | StreamEvent::ToolCallEnd { .. } => "tool_call",
        StreamEvent::Usage(_) => "usage",
        StreamEvent::Stop { .. } => "stop",
        StreamEvent::Done => "done",
        StreamEvent::Error(_) => "error",
    }
}

impl ActualLlmCall {
    async fn emit_first_output(&self, kind: &str, elapsed_ms: u64, transport: &str) {
        emit_runtime_event(
            &self.session,
            "llm_first_output",
            Some(self.llm_call_id.clone()),
            Some(serde_json::json!({
                "schema_version": 1,
                "message_id": &self.message_id,
                "output_kind": kind,
                "time_to_first_output_ms": elapsed_ms,
                "transport": transport,
            })),
        )
        .await;
    }

    async fn emit_stream_summary(
        &self,
        observation: &StreamObservation,
        transport: &str,
        outcome: &str,
        error: Option<&str>,
    ) {
        emit_runtime_event(
            &self.session,
            "llm_stream_summary",
            Some(self.llm_call_id.clone()),
            Some(serde_json::json!({
                "schema_version": 1,
                "message_id": &self.message_id,
                "transport": transport,
                "outcome": outcome,
                "error": error,
                "event_count": observation.event_count,
                "visible_event_count": observation.visible_event_count,
                "text_delta_count": observation.text_delta_count,
                "reasoning_delta_count": observation.reasoning_delta_count,
                "tool_delta_count": observation.tool_delta_count,
                "first_event_ms": observation.first_event_ms,
                "time_to_first_output_ms": observation.first_output_ms,
                "time_to_first_token_ms": observation.first_token_ms,
                "stream_duration_ms": observation.duration_ms(),
            })),
        )
        .await;
    }
}

#[async_trait]
impl LlmCallFn for ActualLlmCall {
    async fn call(
        &self,
        _state: &AgentState,
        messages: Vec<Message>,
    ) -> Result<Message, AgentError> {
        let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
        let mut stream = self.model.stream(&messages, &tool_refs, &self.model_config);

        let mut accumulator = StreamMessageAccumulator::new();
        let mut event_count: u32 = 0;
        let mut observation = StreamObservation::new();

        // Peer handle for cancellation; `cancelled()` needs `&mut`.
        let mut cancel = self.cancel.clone();
        loop {
            // Race the next chunk against cancellation so a cancel that lands
            // while we're blocked on a slow/hung stream aborts the connection
            // immediately, rather than being seen only after the stream ends.
            // `select` is biased toward the stream, so a stream that is
            // actively producing still drains normally; cancellation wins
            // precisely when the stream is blocked waiting. On cancel we
            // return, dropping `stream` and severing the underlying request.
            let next_fut = stream.next();
            let cancelled_fut = cancel.cancelled();
            let event =
                match futures_util::future::select(Box::pin(next_fut), Box::pin(cancelled_fut))
                    .await
                {
                    futures_util::future::Either::Left((Some(event), _)) => event,
                    futures_util::future::Either::Left((None, _)) => break,
                    futures_util::future::Either::Right(((), _)) => {
                        tracing::info!(
                            model = %self.model.model_id(),
                            stream_events = event_count,
                            "LLM stream cancelled mid-flight; abandoning the connection"
                        );
                        self.emit_stream_summary(
                            &observation,
                            "stream",
                            "cancelled",
                            Some("cancelled"),
                        )
                        .await;
                        return Err(AgentError::Cancelled);
                    }
                };
            event_count += 1;
            if let Some((kind, elapsed_ms)) = observation.observe(&event) {
                self.emit_first_output(kind, elapsed_ms, "stream").await;
            }
            // Build a placeholder message for the MessageUpdate envelope.
            let agent_msg = AgentMessage::Standard(Message {
                id: self.message_id.clone(),
                role: MessageRole::Assistant,
                content: vec![],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let _ = self
                .event_tx
                .send(AgentEvent::MessageUpdate {
                    message: agent_msg,
                    delta: event.clone(),
                })
                .await;

            if let Err(error) = accumulator.push(event) {
                let error = error.to_string();
                self.emit_stream_summary(&observation, "stream", "error", Some(&error))
                    .await;
                return Err(AgentError::LlmError(error));
            }
        }

        // Check if streaming produced anything useful
        let text_len = accumulator.text_len();

        if accumulator.is_empty() {
            // Stream returned nothing — fallback to non-streaming complete()
            tracing::warn!(
                model = %self.model.model_id(),
                stream_events = event_count,
                "LLM stream produced empty response — falling back to non-streaming"
            );

            let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
            let complete = self
                .model
                .complete(&messages, &tool_refs, &self.model_config);
            let fallback = match futures_util::future::select(
                Box::pin(complete),
                Box::pin(cancel.cancelled()),
            )
            .await
            {
                futures_util::future::Either::Left((result, _)) => result,
                futures_util::future::Either::Right(((), _)) => {
                    tracing::info!(
                        model = %self.model.model_id(),
                        "non-streaming LLM fallback cancelled"
                    );
                    self.emit_stream_summary(
                        &observation,
                        "stream_then_complete_fallback",
                        "cancelled",
                        Some("cancelled"),
                    )
                    .await;
                    return Err(AgentError::Cancelled);
                }
            };
            match fallback {
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
                        if let Some((kind, elapsed_ms)) = observation.observe(&delta) {
                            self.emit_first_output(
                                kind,
                                elapsed_ms,
                                "stream_then_complete_fallback",
                            )
                            .await;
                        }
                        let agent_msg = AgentMessage::Standard(Message {
                            id: self.message_id.clone(),
                            role: MessageRole::Assistant,
                            content: vec![],
                            tool_call_id: None,
                            usage: None,
                            timestamp: chrono::Utc::now().timestamp_millis(),
                        });
                        let _ = self
                            .event_tx
                            .send(AgentEvent::MessageUpdate {
                                message: agent_msg,
                                delta,
                            })
                            .await;
                    }

                    tracing::info!(
                        model = %self.model.model_id(),
                        content_blocks = msg.content.len(),
                        has_usage = msg.usage.is_some(),
                        "non-streaming fallback succeeded"
                    );
                    self.emit_stream_summary(
                        &observation,
                        "stream_then_complete_fallback",
                        "ok",
                        None,
                    )
                    .await;
                    return Ok(msg);
                }
                Err(e) => {
                    tracing::error!(error = %e, "non-streaming fallback also failed");
                    let error = e.to_string();
                    self.emit_stream_summary(
                        &observation,
                        "stream_then_complete_fallback",
                        "error",
                        Some(&error),
                    )
                    .await;
                    return Err(e);
                }
            }
        }

        if !accumulator.has_usage() {
            tracing::debug!(
                model = %self.model.model_id(),
                stream_events = event_count,
                text_len,
                "LLM stream completed without usage data"
            );
        }

        let result = accumulator
            .finish(
                self.message_id.clone(),
                chrono::Utc::now().timestamp_millis(),
            )
            .map_err(|error| AgentError::LlmError(error.to_string()));
        let error = result.as_ref().err().map(ToString::to_string);
        self.emit_stream_summary(
            &observation,
            "stream",
            if result.is_ok() { "ok" } else { "error" },
            error.as_deref(),
        )
        .await;
        result
    }
}
