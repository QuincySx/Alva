// INPUT:  crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent, alva_types::*
// OUTPUT: pub async fn run_agent()
// POS:    V2 session-centric agent loop — reads from session each iteration, never stores messages locally.
use std::sync::Arc;

use alva_types::{
    AgentError, AgentMessage, BusHandle, CancellationToken, ContentBlock, Message,
    MessageRole, ModelConfig, StreamEvent, ToolCall, ToolOutput,
};
use alva_types::model::LanguageModel;
use alva_types::tool::Tool;
use async_trait::async_trait;
use serde_json::Value;
use tokio::sync::mpsc;
use tokio_stream::StreamExt;

use crate::middleware::{LlmCallFn, MiddlewareError, ToolCallFn};
use crate::runtime_context::RuntimeExecutionContext;
use crate::state::{AgentConfig, AgentState};
use crate::event::AgentEvent;

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
}

#[async_trait]
impl LlmCallFn for ActualLlmCall {
    async fn call(&self, _state: &mut AgentState, messages: Vec<Message>) -> Result<Message, AgentError> {
        let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
        let mut stream = self.model.stream(&messages, &tool_refs, &self.model_config);

        let msg_id = uuid::Uuid::new_v4().to_string();
        let mut text_content = String::new();
        let mut usage = None;

        // Track in-progress tool calls by index (order of appearance).
        let mut tool_call_builders: std::collections::HashMap<usize, (String, String, String)> =
            std::collections::HashMap::new();

        while let Some(event) = stream.next().await {
            // Build a placeholder message for the MessageUpdate envelope.
            let agent_msg = AgentMessage::Standard(Message {
                id: msg_id.clone(),
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
                StreamEvent::ToolCallDelta { id, name, arguments_delta } => {
                    if !id.is_empty() {
                        // New tool call starting — assign the next index.
                        let idx = tool_call_builders.len();
                        tool_call_builders.insert(idx, (
                            id,
                            name.unwrap_or_default(),
                            arguments_delta,
                        ));
                    } else {
                        // Continuing the most recent tool call.
                        let idx = tool_call_builders.len().saturating_sub(1);
                        if let Some(tc) = tool_call_builders.get_mut(&idx) {
                            tc.2.push_str(&arguments_delta);
                        }
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

        // Assemble the final Message from accumulated chunks.
        let mut content_blocks = Vec::new();
        if !text_content.is_empty() {
            content_blocks.push(ContentBlock::Text { text: text_content });
        }

        // Convert accumulated tool calls to ContentBlocks (sorted by index).
        let mut indices: Vec<usize> = tool_call_builders.keys().cloned().collect();
        indices.sort();
        for idx in indices {
            if let Some((id, name, args_str)) = tool_call_builders.remove(&idx) {
                let input: Value = serde_json::from_str(&args_str)
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        Ok(Message {
            id: msg_id,
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
    async fn call(&self, _state: &mut AgentState, tool_call: &ToolCall) -> Result<ToolOutput, AgentError> {
        // No timeout in the kernel — use ToolTimeoutMiddleware (wrap_tool_call) to add one.
        let mut ctx = RuntimeExecutionContext::new(
            self.cancel.clone(),
            tool_call.id.clone(),
            self.event_tx.clone(),
            self.session_id.clone(),
        );
        if let Some(ref ws) = self.workspace {
            ctx = ctx.with_workspace(ws.clone());
        }
        if let Some(ref bus) = self.bus {
            ctx = ctx.with_bus(bus.clone());
        }
        self.tool.execute(tool_call.arguments.clone(), &ctx).await
    }
}

/// V2 agent loop — session-centric with middleware hooks.
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
    // 1. Lifecycle: on_agent_start
    config
        .middleware
        .run_on_agent_start(state)
        .await
        .map_err(|e| AgentError::Other(e.to_string()))?;

    // Emit AgentStart
    let _ = event_tx.send(AgentEvent::AgentStart);

    // 2. Store input messages in session
    for msg in input {
        state.session.append(msg);
    }

    // 3. Main loop
    let mut error: Option<String> = None;
    let result = run_loop(state, config, &cancel, &event_tx).await;

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
) -> Result<(), AgentError> {
    let mut total_iterations: u32 = 0;

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

            // Emit TurnStart
            let _ = event_tx.send(AgentEvent::TurnStart);

            // 3a. Get messages from session (optionally windowed)
            let session_messages = if config.context_window > 0 {
                state.session.recent(config.context_window)
            } else {
                state.session.messages()
            };

            // 3b. Build LLM messages: [system_prompt] + session messages (only Standard)
            let mut llm_messages = Vec::new();
            if !config.system_prompt.is_empty() {
                llm_messages.push(Message::system(&config.system_prompt));
            }
            for msg in &session_messages {
                // Only Standard messages are sent to the LLM.
                // Steering and FollowUp are normalized to Standard before
                // entering the session (see delegate injection below).
                if let AgentMessage::Standard(m) = msg {
                    llm_messages.push(m.clone());
                }
            }

            // 3c. Middleware: before_llm_call
            config
                .middleware
                .run_before_llm_call(state, &mut llm_messages)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;

            // 3d. Emit MessageStart before the LLM call
            let placeholder_msg = AgentMessage::Standard(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: vec![],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            let _ = event_tx.send(AgentEvent::MessageStart { message: placeholder_msg });

            // 3e. Call LLM through wrap_llm_call middleware chain
            let actual_call = ActualLlmCall {
                model: state.model.clone(),
                tools: state.tools.clone(),
                model_config: config.model_config.clone(),
                event_tx: event_tx.clone(),
            };
            let mut response = config
                .middleware
                .run_wrap_llm_call(state, llm_messages, &actual_call)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;

            // 3f. Middleware: after_llm_call
            config
                .middleware
                .run_after_llm_call(state, &mut response)
                .await
                .map_err(|e| AgentError::Other(e.to_string()))?;

            // 3g. Store response in session
            state
                .session
                .append(AgentMessage::Standard(response.clone()));

            // 3h. Emit MessageEnd with the complete response
            // (MessageStart was emitted before the LLM call; MessageUpdate
            // events were emitted during streaming inside ActualLlmCall.)
            let agent_msg = AgentMessage::Standard(response.clone());
            let _ = event_tx.send(AgentEvent::MessageEnd {
                message: agent_msg,
            });

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
                let _ = event_tx.send(AgentEvent::TurnEnd);
                break 'inner;
            }

            // 3k. Execute each tool_call
            for tool_call in &tool_calls {
                // Emit ToolExecutionStart
                let _ = event_tx.send(AgentEvent::ToolExecutionStart {
                    tool_call: tool_call.clone(),
                });

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
                        return Err(AgentError::Other(e.to_string()));
                    }
                    Ok(()) => {
                        // Execute the tool through wrap_tool_call middleware chain (with timeout)
                        match tool {
                            Some(ref t) => {
                                let actual_tool_call = ActualToolCall {
                                    tool: t.clone(),
                                    cancel: cancel.clone(),
                                    event_tx: event_tx.clone(),
                                    session_id: state.session.id().to_string(),
                                    workspace: config.workspace.clone(),
                                    bus: config.bus.clone(),
                                };
                                config
                                    .middleware
                                    .run_wrap_tool_call(state, tool_call, &actual_tool_call)
                                    .await
                                    .map_err(|e| AgentError::Other(e.to_string()))?
                            }
                            None => ToolOutput::error(format!("Tool not found: {}", tool_call.name)),
                        }
                    }
                };

                // Middleware: after_tool_call
                config
                    .middleware
                    .run_after_tool_call(state, tool_call, &mut result)
                    .await
                    .map_err(|e| AgentError::Other(e.to_string()))?;

                // Build Tool message and append to session
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
                state
                    .session
                    .append(AgentMessage::Standard(tool_message));

                // Emit ToolExecutionEnd
                let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                    tool_call: tool_call.clone(),
                    result,
                });
            }

            // 3l. Emit TurnEnd
            let _ = event_tx.send(AgentEvent::TurnEnd);

            // Steering check: after tool execution, before next LLM call
            if let Some(hook) = &config.loop_hook {
                if let Some(steering_msg) = hook.take_steering() {
                    // Convert to Standard — steering is an injection method, not a message type.
                    // The session should only contain Standard messages for persistence.
                    let msg = match steering_msg {
                        AgentMessage::Steering(m) => AgentMessage::Standard(m),
                        other => other,
                    };
                    state.session.append(msg);
                    continue 'inner;
                }
            }
        }

        // Follow-up check: when inner loop ends naturally
        let follow_ups = config
            .loop_hook
            .as_ref()
            .map(|d| d.take_follow_ups())
            .unwrap_or_default();
        if follow_ups.is_empty() {
            break 'outer; // Truly done
        }
        for msg in follow_ups {
            // Convert to Standard — follow-up is an injection method, not a message type.
            // The session should only contain Standard messages for persistence.
            let msg = match msg {
                AgentMessage::FollowUp(m) => AgentMessage::Standard(m),
                other => other,
            };
            state.session.append(msg);
        }
        // Continue outer loop — process follow-ups
    }

    Ok(())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::*;
    use std::sync::Arc;

    struct EchoModel;
    #[async_trait::async_trait]
    impl LanguageModel for EchoModel {
        async fn complete(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Result<Message, AgentError> {
            let last = messages
                .last()
                .map(|m| m.text_content())
                .unwrap_or_default();
            Ok(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: format!("Echo: {}", last),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }
        fn stream(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            let last = messages
                .last()
                .map(|m| m.text_content())
                .unwrap_or_default();
            let text = format!("Echo: {}", last);
            Box::pin(tokio_stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta { text },
                StreamEvent::Done,
            ]))
        }
        fn model_id(&self) -> &str {
            "echo"
        }
    }

    fn make_state() -> AgentState {
        AgentState {
            model: Arc::new(EchoModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: crate::shared::Extensions::new(),
        }
    }

    #[tokio::test]
    async fn simple_echo() {
        let mut state = make_state();
        let config = AgentConfig {
            middleware: crate::middleware::MiddlewareStack::new(),
            system_prompt: "Echo bot.".to_string(),
            max_iterations: 100,
            model_config: ModelConfig::default(),
            context_window: 0,
            loop_hook: None,
            workspace: None,
            bus: None,
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        run_agent(
            &mut state,
            &config,
            cancel,
            vec![AgentMessage::Standard(Message::user("hello"))],
            tx,
        )
        .await
        .unwrap();

        // Session should have user + assistant
        assert_eq!(state.session.messages().len(), 2);

        // Events
        let mut events = vec![];
        while let Ok(e) = rx.try_recv() {
            events.push(e);
        }
        assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentStart)));
        assert!(events
            .iter()
            .any(|e| matches!(e, AgentEvent::MessageEnd { .. })));
        assert!(events
            .iter()
            .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
    }

    #[tokio::test]
    async fn cancellation_stops_loop() {
        let mut state = make_state();
        let config = AgentConfig {
            middleware: crate::middleware::MiddlewareStack::new(),
            system_prompt: "Test.".to_string(),
            max_iterations: 100,
            model_config: ModelConfig::default(),
            context_window: 0,
            loop_hook: None,
            workspace: None,
            bus: None,
        };
        let cancel = CancellationToken::new();
        cancel.cancel(); // Cancel immediately

        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let result = run_agent(
            &mut state,
            &config,
            cancel,
            vec![AgentMessage::Standard(Message::user("hi"))],
            tx,
        )
        .await;
        assert!(matches!(result, Err(AgentError::Cancelled)));
    }

    #[tokio::test]
    async fn empty_input() {
        let mut state = make_state();
        let config = AgentConfig {
            middleware: crate::middleware::MiddlewareStack::new(),
            system_prompt: "Test.".to_string(),
            max_iterations: 100,
            model_config: ModelConfig::default(),
            context_window: 0,
            loop_hook: None,
            workspace: None,
            bus: None,
        };
        let cancel = CancellationToken::new();
        let (tx, _) = tokio::sync::mpsc::unbounded_channel();

        // Empty input — LLM gets only system prompt, responds once, done
        run_agent(&mut state, &config, cancel, vec![], tx)
            .await
            .unwrap();
        // Session has 1 message (assistant response to empty context)
        assert_eq!(state.session.messages().len(), 1);
    }
}
