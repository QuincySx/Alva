// INPUT:  agent_types (CancellationToken, ContentBlock, LanguageModel, Message, MessageRole, StreamEvent, ToolCall), tokio, tokio_stream, tracing, uuid, chrono, crate::middleware, crate::tool_executor, crate::types, crate::event
// OUTPUT: run_agent_loop (pub(crate))
// POS:    Double-loop agent execution — outer loop handles follow-ups, inner loop drives LLM + tool calls + steering, with middleware hooks at each stage.
use agent_types::{
    CancellationToken, ContentBlock, LanguageModel, Message, MessageRole, StreamEvent, ToolCall,
};
use tokio::sync::mpsc;
use tokio_stream::StreamExt;
use tracing::{debug, info, warn};

use crate::event::AgentEvent;
use crate::middleware::{Extensions, MiddlewareContext};
use crate::tool_executor::execute_tools;
use crate::types::{AgentHooks, AgentContext, AgentMessage, AgentState};

/// Run the double-loop agent execution algorithm.
///
/// * **Outer loop** — checks for follow-up messages after the inner loop
///   finishes.
/// * **Inner loop** — repeatedly calls the LLM, executes any requested tool
///   calls, injects steering messages, and continues until the model produces
///   a response with no tool calls.
///
/// The function mutates `state` in place and emits `AgentEvent`s through the
/// provided channel.
///
/// A single [`MiddlewareContext`] is created at the start and shared across
/// all middleware hook invocations so that [`Extensions`] set in
/// `on_agent_start` are visible to `before_llm_call`, `before_tool_call`, etc.
pub(crate) async fn run_agent_loop(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), agent_types::AgentError> {
    let _ = event_tx.send(AgentEvent::AgentStart);

    // Create a single MiddlewareContext that persists across the entire run.
    let mut mw_ctx = MiddlewareContext {
        session_id: state.tool_context.session_id().to_string(),
        system_prompt: state.system_prompt.clone(),
        messages: state.messages.clone(),
        extensions: Extensions::new(),
    };

    // Middleware: on_agent_start
    if !config.middleware.is_empty() {
        if let Err(e) = config.middleware.run_on_agent_start(&mut mw_ctx).await {
            warn!(error = %e, "middleware on_agent_start failed");
        }
    }

    let result = run_agent_loop_inner(state, model, config, cancel, event_tx, &mut mw_ctx).await;

    // Middleware: on_agent_end
    if !config.middleware.is_empty() {
        mw_ctx.messages = state.messages.clone();
        let err_str = match &result {
            Ok(()) => None,
            Err(e) => Some(e.to_string()),
        };
        if let Err(e) = config
            .middleware
            .run_on_agent_end(&mut mw_ctx, err_str.as_deref())
            .await
        {
            warn!(error = %e, "middleware on_agent_end failed");
        }
    }

    match &result {
        Ok(()) => {
            let _ = event_tx.send(AgentEvent::AgentEnd { error: None });
        }
        Err(e) => {
            let _ = event_tx.send(AgentEvent::AgentEnd {
                error: Some(e.to_string()),
            });
        }
    }

    result
}

async fn run_agent_loop_inner(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentHooks,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
    mw_ctx: &mut MiddlewareContext,
) -> Result<(), agent_types::AgentError> {
    let mut iteration: u32 = 0;

    // ===== OUTER LOOP (follow-up) ==========================================
    'outer: loop {
        if cancel.is_cancelled() {
            return Err(agent_types::AgentError::Cancelled);
        }

        // ===== INNER LOOP (tool calls + steering) ==========================
        'inner: loop {
            iteration += 1;
            if iteration > config.max_iterations {
                return Err(agent_types::AgentError::MaxIterations(
                    config.max_iterations,
                ));
            }

            if cancel.is_cancelled() {
                return Err(agent_types::AgentError::Cancelled);
            }

            let _ = event_tx.send(AgentEvent::TurnStart);

            // 1. Build the messages to send to the LLM ---------------------
            let context_messages = match &config.transform_context {
                Some(transform) => transform(&state.messages),
                None => state.messages.clone(),
            };

            let convert_ctx = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &context_messages,
                tools: &state.tools,
            };
            let mut llm_messages = (config.convert_to_llm)(&convert_ctx);

            // 1b. Middleware: before_llm_call --------------------------------
            if !config.middleware.is_empty() {
                mw_ctx.messages = state.messages.clone();
                if let Err(e) = config
                    .middleware
                    .run_before_llm_call(mw_ctx, &mut llm_messages)
                    .await
                {
                    warn!(error = %e, "middleware before_llm_call failed");
                }
            }

            // 2. Collect tool references for the model ----------------------
            let tool_refs: Vec<&dyn agent_types::Tool> =
                state.tools.iter().map(|t| t.as_ref()).collect();

            // 3. Call the model ---------------------------------------------
            debug!(
                model = model.model_id(),
                messages = llm_messages.len(),
                tools = tool_refs.len(),
                "calling LLM"
            );

            let mut assistant_message = if state.is_streaming {
                stream_llm_response(model, &llm_messages, &tool_refs, &state.model_config, event_tx).await?
            } else {
                model.complete(&llm_messages, &tool_refs, &state.model_config).await?
            };

            // 3b. Middleware: after_llm_call ---------------------------------
            if !config.middleware.is_empty() {
                mw_ctx.messages = state.messages.clone();
                if let Err(e) = config
                    .middleware
                    .run_after_llm_call(mw_ctx, &mut assistant_message)
                    .await
                {
                    warn!(error = %e, "middleware after_llm_call failed");
                }
            }

            let agent_msg = AgentMessage::Standard(assistant_message.clone());

            // 4. Emit message events ----------------------------------------
            let _ = event_tx.send(AgentEvent::MessageStart {
                message: agent_msg.clone(),
            });
            let _ = event_tx.send(AgentEvent::MessageEnd {
                message: agent_msg.clone(),
            });

            // 5. Push assistant message into state --------------------------
            state.messages.push(agent_msg.clone());

            // 6. Check for tool calls ---------------------------------------
            let tool_calls: Vec<ToolCall> = assistant_message
                .content
                .iter()
                .filter_map(|b| match b {
                    ContentBlock::ToolUse { id, name, input } => Some(ToolCall {
                        id: id.clone(),
                        name: name.clone(),
                        arguments: input.clone(),
                    }),
                    _ => None,
                })
                .collect();

            if tool_calls.is_empty() {
                // No tool calls — the model is done for this inner loop.
                let _ = event_tx.send(AgentEvent::TurnEnd);
                break 'inner;
            }

            // 7. Execute tools ----------------------------------------------
            let context = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &state.messages,
                tools: &state.tools,
            };

            mw_ctx.messages = state.messages.clone();
            let results = execute_tools(
                &tool_calls,
                &state.tools,
                config,
                &context,
                cancel,
                event_tx,
                &state.tool_context,
                mw_ctx,
            )
            .await;

            // 8. Push tool results as messages into state -------------------
            for (tc, result) in tool_calls.iter().zip(results.iter()) {
                let tool_msg = Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        id: tc.id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                    }],
                    tool_call_id: Some(tc.id.clone()),
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                state
                    .messages
                    .push(AgentMessage::Standard(tool_msg));
            }

            let _ = event_tx.send(AgentEvent::TurnEnd);

            // 9. Steering messages ------------------------------------------
            let mut steering = Vec::new();
            for hook in &config.get_steering_messages {
                let ctx = AgentContext {
                    system_prompt: &state.system_prompt,
                    messages: &state.messages,
                    tools: &state.tools,
                };
                steering.extend(hook(&ctx));
            }
            if !steering.is_empty() {
                info!(count = steering.len(), "injecting steering messages");
                state.messages.extend(steering);
                continue 'inner;
            }

            // Tool results exist — we need another LLM call, so continue.
            continue 'inner;
        }
        // ===== END INNER LOOP ==============================================

        // 10. Follow-up messages --------------------------------------------
        let mut follow_ups = Vec::new();
        for hook in &config.get_follow_up_messages {
            let ctx = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &state.messages,
                tools: &state.tools,
            };
            follow_ups.extend(hook(&ctx));
        }
        if !follow_ups.is_empty() {
            info!(count = follow_ups.len(), "injecting follow-up messages");
            state.messages.extend(follow_ups);
            continue 'outer;
        }

        // Nothing more to do.
        break 'outer;
    }
    // ===== END OUTER LOOP ==================================================

    Ok(())
}

// ===========================================================================
// Streaming helper
// ===========================================================================

/// Consumes the model's stream, emitting MessageUpdate events and accumulating
/// the final Message from deltas.
async fn stream_llm_response(
    model: &dyn LanguageModel,
    messages: &[Message],
    tools: &[&dyn agent_types::Tool],
    config: &agent_types::ModelConfig,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<Message, agent_types::AgentError> {
    let mut stream = model.stream(messages, tools, config);

    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_call_accumulators: Vec<ToolCallAccumulator> = Vec::new();
    let mut usage = None;

    while let Some(event) = stream.next().await {
        match &event {
            StreamEvent::TextDelta { text: delta } => {
                text.push_str(delta);
            }
            StreamEvent::ReasoningDelta { text: delta } => {
                reasoning.push_str(delta);
            }
            StreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
            } => {
                if let Some(acc) = tool_call_accumulators.iter_mut().find(|a| a.id == *id) {
                    acc.arguments_json.push_str(arguments_delta);
                    if let Some(n) = name {
                        acc.name = n.clone();
                    }
                } else {
                    tool_call_accumulators.push(ToolCallAccumulator {
                        id: id.clone(),
                        name: name.clone().unwrap_or_default(),
                        arguments_json: arguments_delta.clone(),
                    });
                }
            }
            StreamEvent::Usage(u) => {
                usage = Some(u.clone());
            }
            StreamEvent::Error(e) => {
                return Err(agent_types::AgentError::LlmError(e.clone()));
            }
            _ => {} // Start, Done
        }

        // Build partial message from accumulated state so far
        let mut partial_content = Vec::new();
        if !text.is_empty() {
            partial_content.push(ContentBlock::Text { text: text.clone() });
        }
        if !reasoning.is_empty() {
            partial_content.push(ContentBlock::Reasoning { text: reasoning.clone() });
        }
        for acc in &tool_call_accumulators {
            let input: serde_json::Value = serde_json::from_str(&acc.arguments_json)
                .unwrap_or(serde_json::Value::String(acc.arguments_json.clone()));
            partial_content.push(ContentBlock::ToolUse {
                id: acc.id.clone(),
                name: acc.name.clone(),
                input,
            });
        }

        let partial_message = Message {
            id: String::new(),
            role: MessageRole::Assistant,
            content: partial_content,
            tool_call_id: None,
            usage: usage.clone(),
            timestamp: chrono::Utc::now().timestamp_millis(),
        };

        let _ = event_tx.send(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(partial_message),
            delta: event,
        });
    }

    // Build final message from accumulated deltas
    let mut content = Vec::new();
    if !text.is_empty() {
        content.push(ContentBlock::Text { text });
    }
    if !reasoning.is_empty() {
        content.push(ContentBlock::Reasoning { text: reasoning });
    }
    for acc in &tool_call_accumulators {
        let input: serde_json::Value = serde_json::from_str(&acc.arguments_json)
            .unwrap_or(serde_json::Value::String(acc.arguments_json.clone()));
        content.push(ContentBlock::ToolUse {
            id: acc.id.clone(),
            name: acc.name.clone(),
            input,
        });
    }

    Ok(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content,
        tool_call_id: None,
        usage,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}

struct ToolCallAccumulator {
    id: String,
    name: String,
    arguments_json: String,
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use agent_types::*;
    use async_trait::async_trait;
    use std::pin::Pin;
    use std::sync::Arc;

    // -----------------------------------------------------------------------
    // Mock model that returns a simple text response with no tool calls
    // -----------------------------------------------------------------------
    struct MockModel;

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            Ok(Message {
                id: "mock-msg-1".to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "Hello from mock model!".to_string(),
                }],
                tool_call_id: None,
                usage: Some(UsageMetadata {
                    input_tokens: 10,
                    output_tokens: 5,
                    total_tokens: 15,
                }),
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            // Return an empty stream using async_stream-style manual impl.
            struct EmptyStream;
            impl futures_core::Stream for EmptyStream {
                type Item = StreamEvent;
                fn poll_next(
                    self: Pin<&mut Self>,
                    _cx: &mut std::task::Context<'_>,
                ) -> std::task::Poll<Option<Self::Item>> {
                    std::task::Poll::Ready(None)
                }
            }
            // SAFETY: EmptyStream has no state, trivially Send.
            unsafe impl Send for EmptyStream {}
            Box::pin(EmptyStream)
        }

        fn model_id(&self) -> &str {
            "mock-model"
        }
    }

    // -----------------------------------------------------------------------
    // Helper: default convert_to_llm — just extract Standard messages
    // -----------------------------------------------------------------------
    fn default_convert_to_llm(ctx: &AgentContext<'_>) -> Vec<Message> {
        let mut result = vec![Message::system(ctx.system_prompt)];
        for m in ctx.messages {
            if let AgentMessage::Standard(msg) = m {
                result.push(msg.clone());
            }
        }
        result
    }

    // -----------------------------------------------------------------------
    // Test: simple text response with no tool calls
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_simple_text_response() {
        let model = MockModel;
        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new(
            "You are a test assistant.".to_string(),
            ModelConfig::default(),
        );

        // Seed a user message.
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx)
                .await;

        assert!(result.is_ok(), "agent loop should succeed");

        // Collect all events.
        drop(event_tx); // close channel so recv terminates
        let mut events = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            events.push(ev);
        }

        // Verify expected event sequence.
        assert!(
            matches!(events.first(), Some(AgentEvent::AgentStart)),
            "first event should be AgentStart"
        );
        assert!(
            matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })),
            "last event should be AgentEnd with no error"
        );

        // Should contain a MessageEnd with the assistant response.
        let has_message_end = events.iter().any(|e| {
            matches!(e, AgentEvent::MessageEnd { message: AgentMessage::Standard(m) } if m.text_content() == "Hello from mock model!")
        });
        assert!(has_message_end, "should have MessageEnd with model response");

        // State should now have 2 messages: the user message + assistant.
        assert_eq!(state.messages.len(), 2);
    }

    // -----------------------------------------------------------------------
    // Test: cancellation stops the loop
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_cancellation() {
        let model = MockModel;
        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        cancel.cancel(); // pre-cancel

        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let mut state = AgentState::new(
            "test".to_string(),
            ModelConfig::default(),
        );
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result =
            run_agent_loop(&mut state, &model, &config, &cancel, &event_tx)
                .await;

        assert!(
            matches!(result, Err(AgentError::Cancelled)),
            "should return Cancelled"
        );
    }

    // -----------------------------------------------------------------------
    // Test: streaming text response
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_streaming_text_response() {
        // Create a model that implements stream() returning TextDelta events
        struct StreamingMockModel;

        #[async_trait]
        impl LanguageModel for StreamingMockModel {
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<Message, AgentError> {
                panic!("should use stream path, not complete")
            }

            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::iter(vec![
                    StreamEvent::Start,
                    StreamEvent::TextDelta {
                        text: "Hello ".into(),
                    },
                    StreamEvent::TextDelta {
                        text: "world!".into(),
                    },
                    StreamEvent::Done,
                ]))
            }

            fn model_id(&self) -> &str {
                "streaming-mock"
            }
        }

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test".into(), ModelConfig::default());
        state.is_streaming = true;
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let result = run_agent_loop(
            &mut state,
            &StreamingMockModel,
            &config,
            &cancel,
            &event_tx,
        )
        .await;
        assert!(result.is_ok(), "agent loop should succeed");

        drop(event_tx);
        let mut got_update = false;
        let mut got_message_end = false;
        while let Some(ev) = event_rx.recv().await {
            if matches!(ev, AgentEvent::MessageUpdate { .. }) {
                got_update = true;
            }
            if let AgentEvent::MessageEnd {
                message: AgentMessage::Standard(m),
            } = &ev
            {
                if m.text_content() == "Hello world!" {
                    got_message_end = true;
                }
            }
        }
        assert!(got_update, "should have received MessageUpdate events");
        assert!(
            got_message_end,
            "should have MessageEnd with accumulated text"
        );

        // State should have the accumulated message
        assert_eq!(state.messages.len(), 2); // user + assistant
    }

    // -----------------------------------------------------------------------
    // Test: streaming emits partial messages (not placeholders) in MessageUpdate
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_streaming_emits_partial_message() {
        struct PartialStreamModel;

        #[async_trait]
        impl LanguageModel for PartialStreamModel {
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<Message, AgentError> {
                panic!("should use stream path")
            }

            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::iter(vec![
                    StreamEvent::Start,
                    StreamEvent::TextDelta { text: "Hello ".into() },
                    StreamEvent::TextDelta { text: "world!".into() },
                    StreamEvent::Done,
                ]))
            }

            fn model_id(&self) -> &str {
                "partial-stream-mock"
            }
        }

        let config = AgentHooks::new(Arc::new(default_convert_to_llm));
        let cancel = CancellationToken::new();
        let (event_tx, mut event_rx) = mpsc::unbounded_channel();

        let mut state = AgentState::new("test".into(), ModelConfig::default());
        state.is_streaming = true;
        state.messages.push(AgentMessage::Standard(Message::user("Hi")));

        let result = run_agent_loop(
            &mut state,
            &PartialStreamModel,
            &config,
            &cancel,
            &event_tx,
        )
        .await;
        assert!(result.is_ok());

        drop(event_tx);
        let mut partial_texts = Vec::new();
        while let Some(ev) = event_rx.recv().await {
            if let AgentEvent::MessageUpdate {
                message: AgentMessage::Standard(m),
                ..
            } = &ev
            {
                partial_texts.push(m.text_content());
            }
        }

        // After "Hello " delta, partial should contain "Hello "
        // After "world!" delta, partial should contain "Hello world!"
        assert!(
            partial_texts.iter().any(|t| t == "Hello "),
            "should have partial with 'Hello ', got: {:?}",
            partial_texts
        );
        assert!(
            partial_texts.iter().any(|t| t == "Hello world!"),
            "should have partial with 'Hello world!', got: {:?}",
            partial_texts
        );
    }

    // -----------------------------------------------------------------------
    // Test: MiddlewareContext Extensions persist across hooks
    // -----------------------------------------------------------------------
    #[tokio::test]
    async fn test_middleware_extensions_persist_across_hooks() {
        use crate::middleware::{Middleware, MiddlewareContext, MiddlewareError};
        use async_trait::async_trait;

        #[derive(Debug)]
        struct Budget(u32);

        struct PersistenceMiddleware {
            observed: Arc<parking_lot::Mutex<Option<u32>>>,
        }

        #[async_trait]
        impl Middleware for PersistenceMiddleware {
            async fn on_agent_start(
                &self,
                ctx: &mut MiddlewareContext,
            ) -> Result<(), MiddlewareError> {
                ctx.extensions.insert(Budget(999));
                Ok(())
            }
            async fn before_llm_call(
                &self,
                ctx: &mut MiddlewareContext,
                _msgs: &mut Vec<Message>,
            ) -> Result<(), MiddlewareError> {
                if let Some(b) = ctx.extensions.get::<Budget>() {
                    *self.observed.lock() = Some(b.0);
                }
                Ok(())
            }
        }

        let observed = Arc::new(parking_lot::Mutex::new(None));
        let mw = PersistenceMiddleware {
            observed: observed.clone(),
        };

        let mut config = AgentHooks::new(Arc::new(default_convert_to_llm));
        config.middleware.push(Arc::new(mw));

        let cancel = CancellationToken::new();
        let (event_tx, _event_rx) = mpsc::unbounded_channel();
        let mut state = AgentState::new("test".to_string(), ModelConfig::default());
        state
            .messages
            .push(AgentMessage::Standard(Message::user("Hi")));

        let _ = run_agent_loop(&mut state, &MockModel, &config, &cancel, &event_tx).await;

        assert_eq!(
            *observed.lock(),
            Some(999),
            "Extensions should persist from on_agent_start to before_llm_call"
        );
    }
}
