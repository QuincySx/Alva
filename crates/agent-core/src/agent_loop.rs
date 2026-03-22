use agent_types::{
    CancellationToken, ContentBlock, LanguageModel, Message, MessageRole, ToolCall,
};
use tokio::sync::mpsc;
use tracing::{debug, info};

use crate::event::AgentEvent;
use crate::tool_executor::execute_tools;
use crate::types::{AgentConfig, AgentContext, AgentMessage, AgentState};

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
pub(crate) async fn run_agent_loop(
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), agent_types::AgentError> {
    let _ = event_tx.send(AgentEvent::AgentStart);

    let result = run_agent_loop_inner(state, model, config, cancel, event_tx).await;

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
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
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

            let llm_messages =
                (config.convert_to_llm)(&context_messages, &state.system_prompt);

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

            let assistant_message = model
                .complete(&llm_messages, &tool_refs, &state.model_config)
                .await?;

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
                .tool_calls
                .iter()
                .map(|tc| ToolCall {
                    id: tc.id.clone(),
                    name: tc.name.clone(),
                    arguments: tc.arguments.clone(),
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

            let results = execute_tools(
                &tool_calls,
                &state.tools,
                config,
                &context,
                cancel,
                event_tx,
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
                    tool_calls: vec![],
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
            if let Some(ref get_steering) = config.get_steering_messages {
                let ctx = AgentContext {
                    system_prompt: &state.system_prompt,
                    messages: &state.messages,
                    tools: &state.tools,
                };
                let steering = get_steering(&ctx);
                if !steering.is_empty() {
                    info!(count = steering.len(), "injecting steering messages");
                    state.messages.extend(steering);
                    continue 'inner;
                }
            }

            // Tool results exist — we need another LLM call, so continue.
            continue 'inner;
        }
        // ===== END INNER LOOP ==============================================

        // 10. Follow-up messages --------------------------------------------
        if let Some(ref get_follow_up) = config.get_follow_up_messages {
            let ctx = AgentContext {
                system_prompt: &state.system_prompt,
                messages: &state.messages,
                tools: &state.tools,
            };
            let follow_ups = get_follow_up(&ctx);
            if !follow_ups.is_empty() {
                info!(count = follow_ups.len(), "injecting follow-up messages");
                state.messages.extend(follow_ups);
                continue 'outer;
            }
        }

        // Nothing more to do.
        break 'outer;
    }
    // ===== END OUTER LOOP ==================================================

    Ok(())
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
                tool_calls: vec![],
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
    fn default_convert_to_llm(
        messages: &[AgentMessage],
        system_prompt: &str,
    ) -> Vec<Message> {
        let mut result = vec![Message::system(system_prompt)];
        for m in messages {
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
        let config = AgentConfig::new(Arc::new(default_convert_to_llm));
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
        let config = AgentConfig::new(Arc::new(default_convert_to_llm));
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
}
