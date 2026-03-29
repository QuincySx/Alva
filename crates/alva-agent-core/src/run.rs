// INPUT:  crate::state::{AgentState, AgentConfig}, crate::event::AgentEvent, alva_types::*
// OUTPUT: pub async fn run_agent()
// POS:    V2 session-centric agent loop — reads from session each iteration, never stores messages locally.
use std::sync::Arc;

use alva_types::{
    AgentError, AgentMessage, CancellationToken, ContentBlock, EmptyToolContext, Message,
    MessageRole, ModelConfig, ToolCall, ToolResult,
};
use alva_types::model::LanguageModel;
use alva_types::tool::Tool;
use async_trait::async_trait;
use tokio::sync::mpsc;

use crate::middleware::{LlmCallFn, MiddlewareError, ToolCallFn};
use crate::state::{AgentConfig, AgentState};
use crate::event::AgentEvent;

/// Default timeout for tool execution (2 minutes).
const TOOL_EXECUTION_TIMEOUT_SECS: u64 = 120;

// ---------------------------------------------------------------------------
// LlmCallFn / ToolCallFn adapters for wrap hooks
// ---------------------------------------------------------------------------

/// Wraps the actual LLM model call as a `LlmCallFn` so it can be passed
/// into middleware `wrap_llm_call` as the `next` callback.
struct ActualLlmCall {
    model: Arc<dyn LanguageModel>,
    tools: Vec<Arc<dyn Tool>>,
    model_config: ModelConfig,
}

#[async_trait]
impl LlmCallFn for ActualLlmCall {
    async fn call(&self, _state: &mut AgentState, messages: Vec<Message>) -> Result<Message, AgentError> {
        let tool_refs: Vec<&dyn Tool> = self.tools.iter().map(|t| t.as_ref()).collect();
        self.model
            .complete(&messages, &tool_refs, &self.model_config)
            .await
    }
}

/// Wraps the actual tool execution as a `ToolCallFn` so it can be passed
/// into middleware `wrap_tool_call` as the `next` callback.
struct ActualToolCall {
    tool: Arc<dyn Tool>,
    cancel: CancellationToken,
}

#[async_trait]
impl ToolCallFn for ActualToolCall {
    async fn call(&self, _state: &mut AgentState, tool_call: &ToolCall) -> Result<ToolResult, AgentError> {
        let timeout_duration = std::time::Duration::from_secs(TOOL_EXECUTION_TIMEOUT_SECS);
        match tokio::time::timeout(
            timeout_duration,
            self.tool
                .execute(tool_call.arguments.clone(), &self.cancel, &EmptyToolContext),
        )
        .await
        {
            Ok(result) => result,
            Err(_) => Ok(ToolResult {
                content: format!(
                    "Tool '{}' timed out after {:?}",
                    tool_call.name, timeout_duration
                ),
                is_error: true,
                details: None,
            }),
        }
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
    let _ = config
        .middleware
        .run_on_agent_end(state, error.as_deref())
        .await;

    // 5. Emit AgentEnd
    let _ = event_tx.send(AgentEvent::AgentEnd {
        error: error.clone(),
    });

    result
}

/// Inner loop extracted so we can capture the error cleanly for lifecycle hooks.
async fn run_loop(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError> {
    for _iteration in 0..config.max_iterations {
        // Check cancellation
        if cancel.is_cancelled() {
            return Err(AgentError::Cancelled);
        }

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

        // 3d. Call LLM through wrap_llm_call middleware chain
        let actual_call = ActualLlmCall {
            model: state.model.clone(),
            tools: state.tools.clone(),
            model_config: config.model_config.clone(),
        };
        let mut response = config
            .middleware
            .run_wrap_llm_call(state, llm_messages, &actual_call)
            .await
            .map_err(|e| AgentError::Other(e.to_string()))?;

        // 3e. Middleware: after_llm_call
        config
            .middleware
            .run_after_llm_call(state, &mut response)
            .await
            .map_err(|e| AgentError::Other(e.to_string()))?;

        // 3g. Store response in session
        state
            .session
            .append(AgentMessage::Standard(response.clone()));

        // 3h. Emit events
        // NOTE: Currently uses model.complete() (non-streaming).
        // MessageUpdate events with TextDelta are not emitted.
        // To enable streaming, switch to model.stream() and emit
        // MessageUpdate events for each chunk. The CLI's streaming
        // handler code is ready for this.
        let agent_msg = AgentMessage::Standard(response.clone());
        let _ = event_tx.send(AgentEvent::MessageStart {
            message: agent_msg.clone(),
        });
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

        // 3j. If no tool_calls, emit TurnEnd and break
        if tool_calls.is_empty() {
            let _ = event_tx.send(AgentEvent::TurnEnd);
            break;
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
                    ToolResult {
                        content: format!("Tool call blocked: {}", reason),
                        is_error: true,
                        details: None,
                    }
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
                            };
                            config
                                .middleware
                                .run_wrap_tool_call(state, tool_call, &actual_tool_call)
                                .await
                                .map_err(|e| AgentError::Other(e.to_string()))?
                        }
                        None => ToolResult {
                            content: format!("Tool not found: {}", tool_call.name),
                            is_error: true,
                            details: None,
                        },
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

        // 3l. Emit TurnEnd, then back to top of loop
        let _ = event_tx.send(AgentEvent::TurnEnd);
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
            _: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures::stream::empty())
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
