// Integration tests — full agent run with session + middleware.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use alva_kernel_core::builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, MiddlewareStack};
use alva_kernel_core::run::run_agent;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::AgentEvent;
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
use alva_kernel_abi::agent_session::InMemoryAgentSession;
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{
    AgentMessage, Bus, CancellationToken, ModelConfig, ToolExecutionContext, ToolOutput,
};
use async_trait::async_trait;

// ---------------------------------------------------------------------------
// EchoModel — echoes last user message
// ---------------------------------------------------------------------------

struct EchoModel;

#[async_trait]
impl LanguageModel for EchoModel {
    async fn complete(
        &self,
        messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        let last = messages
            .last()
            .map(|m| m.text_content())
            .unwrap_or_default();
        Ok(CompletionResponse::from_message(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: format!("Echo: {}", last),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }))
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
        Box::pin(futures::stream::iter(vec![
            StreamEvent::Start,
            StreamEvent::TextDelta { text },
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "echo"
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_state() -> AgentState {
    make_state_with_model(Arc::new(EchoModel))
}

fn make_state_with_model(model: Arc<dyn LanguageModel>) -> AgentState {
    AgentState {
        model,
        tools: vec![],
        session: Arc::new(InMemoryAgentSession::new()),
        extensions: alva_kernel_core::shared::Extensions::new(),
    }
}

// ---------------------------------------------------------------------------
// Test 1: simple_echo_run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn simple_echo_run() {
    let mut state = make_state();
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
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
    let messages = state.session.messages().await;
    assert_eq!(messages.len(), 2);

    // First is user
    if let AgentMessage::Standard(ref m) = messages[0] {
        assert_eq!(m.role, MessageRole::User);
        assert!(m.text_content().contains("hello"));
    } else {
        panic!("expected Standard user message");
    }

    // Second is assistant echo
    if let AgentMessage::Standard(ref m) = messages[1] {
        assert_eq!(m.role, MessageRole::Assistant);
        assert!(m.text_content().contains("Echo:"));
    } else {
        panic!("expected Standard assistant message");
    }

    // Verify events
    let mut events = vec![];
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentStart)));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })));
}

// ---------------------------------------------------------------------------
// Test 2: run_with_middleware
// ---------------------------------------------------------------------------

#[tokio::test]
async fn run_with_middleware() {
    let mut state = make_state();

    let mut mw_stack = MiddlewareStack::new();
    mw_stack.push_sorted(Arc::new(LoopDetectionMiddleware::new()));
    mw_stack.push_sorted(Arc::new(DanglingToolCallMiddleware::new()));

    let config = AgentConfig {
        middleware: mw_stack,
        system_prompt: "Echo bot with middleware.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // Should complete without errors — middleware is present but no loops / dangling
    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("test input"))],
        tx,
    )
    .await;
    assert!(result.is_ok());

    let messages = state.session.messages().await;
    assert_eq!(messages.len(), 2);
}

// ---------------------------------------------------------------------------
// Test 3: cancellation_mid_run
// ---------------------------------------------------------------------------

#[tokio::test]
async fn cancellation_mid_run() {
    let mut state = make_state();
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Test.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    cancel.cancel(); // Cancel immediately before run

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hi"))],
        tx,
    )
    .await;

    assert!(matches!(result, Err(AgentError::Cancelled)));

    // AgentEnd event should have an error
    let mut events = vec![];
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentEnd { error: Some(_) })));
}

// ---------------------------------------------------------------------------
// Test 4: session_persists_across_check
// ---------------------------------------------------------------------------

#[tokio::test]
async fn session_persists_across_check() {
    let mut state = make_state();
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Persist test.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };

    // First run
    let cancel1 = CancellationToken::new();
    let (tx1, _) = tokio::sync::mpsc::unbounded_channel();
    run_agent(
        &mut state,
        &config,
        cancel1,
        vec![AgentMessage::Standard(Message::user("first"))],
        tx1,
    )
    .await
    .unwrap();

    // After first run: user + assistant = 2 messages
    assert_eq!(state.session.messages().await.len(), 2);

    // Second run (same state/session)
    let cancel2 = CancellationToken::new();
    let (tx2, _) = tokio::sync::mpsc::unbounded_channel();
    run_agent(
        &mut state,
        &config,
        cancel2,
        vec![AgentMessage::Standard(Message::user("second"))],
        tx2,
    )
    .await
    .unwrap();

    // After second run: 2 (first run) + user + assistant = 4 messages
    let messages = state.session.messages().await;
    assert_eq!(messages.len(), 4);

    // Verify message roles in order
    let roles: Vec<MessageRole> = messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::Standard(msg) => Some(msg.role.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        roles,
        vec![
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::User,
            MessageRole::Assistant,
        ]
    );

    // Verify content
    if let AgentMessage::Standard(ref m) = messages[0] {
        assert!(m.text_content().contains("first"));
    }
    if let AgentMessage::Standard(ref m) = messages[2] {
        assert!(m.text_content().contains("second"));
    }
}

// ---------------------------------------------------------------------------
// Test 5: follow_up_continues_after_natural_stop
// ---------------------------------------------------------------------------

#[tokio::test]
async fn follow_up_continues_after_natural_stop() {
    let mut state = make_state();

    // Create mailbox and queue a follow-up message BEFORE running the agent.
    // The inner loop will finish after responding to the user message,
    // then the outer loop should pick up the follow-up.
    let mailbox = Arc::new(alva_kernel_core::PendingMessageQueue::new());
    mailbox.follow_up(AgentMessage::Standard(Message::user("follow-up question")));

    // Register the mailbox on a bus as dyn AgentLoopHook
    let bus = Bus::new();
    let bus_writer = bus.writer();
    bus_writer.provide::<dyn alva_kernel_core::pending_queue::AgentLoopHook>(
        mailbox as Arc<dyn alva_kernel_core::pending_queue::AgentLoopHook>,
    );
    let bus_handle = bus.handle();

    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: Some(bus_handle),
        context_system: None,
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hello"))],
        tx,
    )
    .await
    .unwrap();

    // Session should have:
    //   [0] user "hello"
    //   [1] assistant echo of "hello"
    //   [2] user "follow-up question"  (injected)
    //   [3] assistant echo of "follow-up question"
    let messages = state.session.messages().await;
    assert_eq!(
        messages.len(),
        4,
        "expected 4 messages, got {}",
        messages.len()
    );

    // Verify roles
    let roles: Vec<MessageRole> = messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::Standard(msg) => Some(msg.role.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        roles,
        vec![
            MessageRole::User,
            MessageRole::Assistant,
            MessageRole::User,
            MessageRole::Assistant,
        ]
    );

    // Verify follow-up content
    if let AgentMessage::Standard(ref m) = messages[2] {
        assert!(
            m.text_content().contains("follow-up question"),
            "expected follow-up message, got: {}",
            m.text_content()
        );
    } else {
        panic!("expected Standard user message at index 2");
    }

    // Verify assistant responded to the follow-up
    if let AgentMessage::Standard(ref m) = messages[3] {
        assert_eq!(m.role, MessageRole::Assistant);
        assert!(m.text_content().contains("Echo:"));
    } else {
        panic!("expected Standard assistant message at index 3");
    }
}

// ---------------------------------------------------------------------------
// Test 6: no_follow_up_means_single_pass
// ---------------------------------------------------------------------------

#[tokio::test]
async fn no_follow_up_means_single_pass() {
    // Without any injected messages, the double loop behaves identically
    // to the old single loop.
    let mut state = make_state();
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("just one"))],
        tx,
    )
    .await
    .unwrap();

    // Session should have exactly 2 messages: user + assistant
    assert_eq!(state.session.messages().await.len(), 2);
}

// ---------------------------------------------------------------------------
// Test 7: message_events_share_the_same_message_id
// ---------------------------------------------------------------------------

#[tokio::test]
async fn message_events_share_the_same_message_id() {
    let mut state = make_state();
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
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

    let mut start_id: Option<String> = None;
    let mut update_ids = Vec::new();
    let mut end_id: Option<String> = None;

    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::MessageStart { message } => {
                if let AgentMessage::Standard(message) = message {
                    start_id = Some(message.id);
                }
            }
            AgentEvent::MessageUpdate { message, .. } => {
                if let AgentMessage::Standard(message) = message {
                    update_ids.push(message.id);
                }
            }
            AgentEvent::MessageEnd { message } => {
                if let AgentMessage::Standard(message) = message {
                    end_id = Some(message.id);
                }
            }
            _ => {}
        }
    }

    let start_id = start_id.expect("missing MessageStart");
    let end_id = end_id.expect("missing MessageEnd");

    assert!(!update_ids.is_empty(), "missing MessageUpdate");
    assert!(
        update_ids.iter().all(|id| id == &start_id),
        "expected all MessageUpdate ids to match MessageStart id"
    );
    assert_eq!(
        end_id, start_id,
        "MessageEnd id should match MessageStart id"
    );
}

// ---------------------------------------------------------------------------
// Test 8: claude_style_tool_call_deltas_merge_into_one_tool_use
// ---------------------------------------------------------------------------

struct ClaudeStyleToolDeltaModel;

#[async_trait]
impl LanguageModel for ClaudeStyleToolDeltaModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("tests use stream() only")
    }

    fn stream(
        &self,
        messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
        let has_tool_result = messages
            .iter()
            .any(|message| message.role == MessageRole::Tool);

        if has_tool_result {
            Box::pin(futures::stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta {
                    text: "done".to_string(),
                },
                StreamEvent::Done,
            ]))
        } else {
            Box::pin(futures::stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::ToolCallDelta {
                    id: "toolu_1".to_string(),
                    name: Some("read_file".to_string()),
                    arguments_delta: "{\"path\":\"a".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    id: "toolu_1".to_string(),
                    name: None,
                    arguments_delta: "b.txt\"}".to_string(),
                },
                StreamEvent::Done,
            ]))
        }
    }

    fn model_id(&self) -> &str {
        "claude-style-tool-delta"
    }
}

#[tokio::test]
async fn claude_style_tool_call_deltas_merge_into_one_tool_use() {
    let mut state = make_state_with_model(Arc::new(ClaudeStyleToolDeltaModel));
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Tool caller.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("read the file"))],
        tx,
    )
    .await
    .unwrap();

    let assistant_with_tool_use = state
        .session
        .messages()
        .await
        .into_iter()
        .find_map(|message| match message {
            AgentMessage::Standard(message)
                if message.role == MessageRole::Assistant && message.has_tool_calls() =>
            {
                Some(message)
            }
            _ => None,
        })
        .expect("missing assistant tool call message");

    let tool_uses: Vec<(String, String, serde_json::Value)> = assistant_with_tool_use
        .content
        .iter()
        .filter_map(|block| match block {
            ContentBlock::ToolUse { id, name, input } => {
                Some((id.clone(), name.clone(), input.clone()))
            }
            _ => None,
        })
        .collect();

    assert_eq!(tool_uses.len(), 1, "expected a single merged ToolUse");
    assert_eq!(tool_uses[0].0, "toolu_1");
    assert_eq!(tool_uses[0].1, "read_file");
    assert_eq!(tool_uses[0].2["path"], "ab.txt");
}

// ---------------------------------------------------------------------------
// Test 9: malformed_tool_arguments_fail_fast
// ---------------------------------------------------------------------------

struct MalformedToolDeltaModel;

#[async_trait]
impl LanguageModel for MalformedToolDeltaModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("tests use stream() only")
    }

    fn stream(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![
            StreamEvent::Start,
            StreamEvent::ToolCallDelta {
                id: "toolu_bad".to_string(),
                name: Some("read_file".to_string()),
                arguments_delta: "{\"path\":\"missing-end\"".to_string(),
            },
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "malformed-tool-delta"
    }
}

#[tokio::test]
async fn malformed_tool_arguments_fail_fast() {
    let mut state = make_state_with_model(Arc::new(MalformedToolDeltaModel));
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Tool caller.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("read the file"))],
        tx,
    )
    .await;

    match result {
        Err(AgentError::LlmError(message)) => {
            assert!(
                message.contains("toolu_bad"),
                "expected error to mention tool call id, got: {message}"
            );
            assert!(
                message.contains("invalid tool arguments"),
                "expected parse failure context, got: {message}"
            );
        }
        other => panic!("expected LlmError for malformed tool args, got {other:?}"),
    }

    assert_eq!(
        state.session.messages().await.len(),
        1,
        "malformed assistant response should not be appended to session"
    );

    let mut saw_agent_end_error = false;
    while let Ok(event) = rx.try_recv() {
        if let AgentEvent::AgentEnd { error } = event {
            saw_agent_end_error = error.is_some();
        }
    }
    assert!(
        saw_agent_end_error,
        "expected AgentEnd to carry the parse error"
    );
}

// ---------------------------------------------------------------------------
// Test 10: stream_error_emits_message_error_event
// ---------------------------------------------------------------------------

struct StreamErrorModel;

#[async_trait]
impl LanguageModel for StreamErrorModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("tests use stream() only")
    }

    fn stream(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![
            StreamEvent::Start,
            StreamEvent::TextDelta {
                text: "partial".to_string(),
            },
            StreamEvent::Error("stream exploded".to_string()),
        ]))
    }

    fn model_id(&self) -> &str {
        "stream-error"
    }
}

#[tokio::test]
async fn stream_error_emits_message_error_event() {
    let mut state = make_state_with_model(Arc::new(StreamErrorModel));
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Failing bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hello"))],
        tx,
    )
    .await;

    match result {
        Err(AgentError::LlmError(message)) => {
            assert!(message.contains("stream exploded"));
        }
        other => panic!("expected LlmError, got {other:?}"),
    }

    let mut start_id: Option<String> = None;
    let mut error_id: Option<String> = None;
    let mut error_text: Option<String> = None;
    let mut saw_message_end = false;
    let mut saw_agent_end_error = false;

    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::MessageStart { message } => {
                if let AgentMessage::Standard(message) = message {
                    start_id = Some(message.id);
                }
            }
            AgentEvent::MessageError { message, error } => {
                if let AgentMessage::Standard(message) = message {
                    error_id = Some(message.id);
                }
                error_text = Some(error);
            }
            AgentEvent::MessageEnd { .. } => {
                saw_message_end = true;
            }
            AgentEvent::AgentEnd { error } => {
                saw_agent_end_error = error.is_some();
            }
            _ => {}
        }
    }

    assert_eq!(state.session.messages().await.len(), 1);
    assert_eq!(
        error_id, start_id,
        "MessageError should close the same message"
    );
    assert_eq!(error_text.as_deref(), Some("LLM error: stream exploded"));
    assert!(
        !saw_message_end,
        "failed message should not emit MessageEnd"
    );
    assert!(
        saw_agent_end_error,
        "AgentEnd should still carry the run error"
    );
}

// ---------------------------------------------------------------------------
// Test 11: after_llm_call_failure_emits_message_error_event
// ---------------------------------------------------------------------------

struct FailingAfterLlmCallMiddleware;

#[async_trait]
impl Middleware for FailingAfterLlmCallMiddleware {
    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Err(MiddlewareError::Other(
            "after_llm_call exploded".to_string(),
        ))
    }
}

#[tokio::test]
async fn after_llm_call_failure_emits_message_error_event() {
    let mut state = make_state();
    let mut middleware = MiddlewareStack::new();
    middleware.push(Arc::new(FailingAfterLlmCallMiddleware));

    let config = AgentConfig {
        middleware,
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hello"))],
        tx,
    )
    .await;

    match result {
        Err(AgentError::Other(message)) => {
            assert!(message.contains("after_llm_call exploded"));
        }
        other => panic!("expected AgentError::Other, got {other:?}"),
    }

    let mut start_id: Option<String> = None;
    let mut error_id: Option<String> = None;
    let mut saw_message_end = false;

    while let Ok(event) = rx.try_recv() {
        match event {
            AgentEvent::MessageStart { message } => {
                if let AgentMessage::Standard(message) = message {
                    start_id = Some(message.id);
                }
            }
            AgentEvent::MessageError { message, error } => {
                if let AgentMessage::Standard(message) = message {
                    assert!(
                        message.text_content().contains("Echo:"),
                        "failed post-processing should preserve the assembled response"
                    );
                    error_id = Some(message.id);
                }
                assert!(error.contains("after_llm_call exploded"));
            }
            AgentEvent::MessageEnd { .. } => {
                saw_message_end = true;
            }
            _ => {}
        }
    }

    assert_eq!(error_id, start_id);
    assert!(
        !saw_message_end,
        "failed message should not emit MessageEnd"
    );
    assert_eq!(
        state.session.messages().await.len(),
        1,
        "response should not be persisted when after_llm_call fails"
    );
}

// ---------------------------------------------------------------------------
// Test 12: cancellation_between_tool_calls_stops_remaining_tools
// ---------------------------------------------------------------------------

struct MultiToolCancelModel;

#[async_trait]
impl LanguageModel for MultiToolCancelModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("tests use stream() only")
    }

    fn stream(
        &self,
        messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
        let tool_results = messages
            .iter()
            .filter(|message| message.role == MessageRole::Tool)
            .count();

        if tool_results == 0 {
            Box::pin(futures::stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::ToolCallDelta {
                    id: "tool_a".to_string(),
                    name: Some("tool_a".to_string()),
                    arguments_delta: "{}".to_string(),
                },
                StreamEvent::ToolCallDelta {
                    id: "tool_b".to_string(),
                    name: Some("tool_b".to_string()),
                    arguments_delta: "{}".to_string(),
                },
                StreamEvent::Done,
            ]))
        } else {
            Box::pin(futures::stream::iter(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta {
                    text: "done".to_string(),
                },
                StreamEvent::Done,
            ]))
        }
    }

    fn model_id(&self) -> &str {
        "multi-tool-cancel"
    }
}

struct CancelOnExecuteTool {
    name: &'static str,
    executions: Arc<AtomicUsize>,
    should_cancel: bool,
}

#[async_trait]
impl Tool for CancelOnExecuteTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        self.name
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        if self.should_cancel {
            ctx.cancel_token().cancel();
        }
        Ok(ToolOutput::text(format!("{} done", self.name)))
    }
}

#[tokio::test]
async fn cancellation_between_tool_calls_stops_remaining_tools() {
    let tool_a_runs = Arc::new(AtomicUsize::new(0));
    let tool_b_runs = Arc::new(AtomicUsize::new(0));

    let mut state = make_state_with_model(Arc::new(MultiToolCancelModel));
    state.tools = vec![
        Arc::new(CancelOnExecuteTool {
            name: "tool_a",
            executions: tool_a_runs.clone(),
            should_cancel: true,
        }),
        Arc::new(CancelOnExecuteTool {
            name: "tool_b",
            executions: tool_b_runs.clone(),
            should_cancel: false,
        }),
    ];

    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Tool caller.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("run tools"))],
        tx,
    )
    .await;

    assert!(matches!(result, Err(AgentError::Cancelled)));
    assert_eq!(tool_a_runs.load(Ordering::SeqCst), 1);
    assert_eq!(
        tool_b_runs.load(Ordering::SeqCst),
        0,
        "tool_b should not execute after tool_a cancels the run"
    );
}

// ---------------------------------------------------------------------------
// Phase 4: ContextHooks integration — proves run_agent fires the wired hooks
// ---------------------------------------------------------------------------

#[tokio::test]
async fn context_hooks_fire_at_lifecycle_points() {
    use alva_kernel_abi::scope::context::{
        ContextHandle, ContextHooks, ContextSystem, Injection, NoopContextHandle,
    };

    #[derive(Default)]
    struct Counters {
        bootstrap: AtomicUsize,
        on_message: AtomicUsize,
        assemble: AtomicUsize,
        after_turn: AtomicUsize,
        dispose: AtomicUsize,
    }

    struct CountingHooks {
        c: Arc<Counters>,
    }

    #[async_trait]
    impl ContextHooks for CountingHooks {
        fn name(&self) -> &str { "counting" }

        async fn bootstrap(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
        ) -> Result<(), alva_kernel_abi::scope::context::ContextError> {
            self.c.bootstrap.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }

        async fn on_message(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            _message: &AgentMessage,
        ) -> Vec<Injection> {
            self.c.on_message.fetch_add(1, Ordering::SeqCst);
            vec![]
        }

        async fn assemble(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            entries: Vec<alva_kernel_abi::scope::context::ContextEntry>,
            _token_budget: usize,
        ) -> Vec<alva_kernel_abi::scope::context::ContextEntry> {
            self.c.assemble.fetch_add(1, Ordering::SeqCst);
            entries
        }

        async fn after_turn(&self, _sdk: &dyn ContextHandle, _agent_id: &str) {
            self.c.after_turn.fetch_add(1, Ordering::SeqCst);
        }

        async fn dispose(&self) -> Result<(), alva_kernel_abi::scope::context::ContextError> {
            self.c.dispose.fetch_add(1, Ordering::SeqCst);
            Ok(())
        }
    }

    let counters = Arc::new(Counters::default());
    let hooks: Arc<dyn ContextHooks> = Arc::new(CountingHooks { c: counters.clone() });
    let handle: Arc<dyn ContextHandle> = Arc::new(NoopContextHandle);
    let cs = Arc::new(ContextSystem::new(hooks, handle));

    let mut state = AgentState {
        model: Arc::new(EchoModel),
        tools: vec![],
        session: Arc::new(InMemoryAgentSession::new()),
        extensions: alva_kernel_core::shared::Extensions::new(),
    };
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: String::new(),
        max_iterations: 5,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: Some(cs),
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hi"))],
        tx,
    )
    .await;
    assert!(result.is_ok(), "run_agent should succeed: {:?}", result);

    // bootstrap fires exactly once at the start.
    assert_eq!(counters.bootstrap.load(Ordering::SeqCst), 1, "bootstrap");
    // dispose fires exactly once at the end.
    assert_eq!(counters.dispose.load(Ordering::SeqCst), 1, "dispose");
    // EchoModel returns text-only with no tool calls → exactly 1 turn → after_turn fires once.
    assert_eq!(counters.after_turn.load(Ordering::SeqCst), 1, "after_turn");
    // assemble fires once per LLM call → exactly 1 here.
    assert_eq!(counters.assemble.load(Ordering::SeqCst), 1, "assemble");
    // on_message fires for the input user message + the LLM assistant response = 2.
    assert_eq!(counters.on_message.load(Ordering::SeqCst), 2, "on_message");
}

#[tokio::test]
async fn assemble_can_inject_extra_message() {
    // Proves the assemble hook can ADD entries that reach the LLM call,
    // by adding an extra user message and asserting the captured model
    // input contains it.
    use alva_kernel_abi::scope::context::{
        ContextEntry, ContextHandle, ContextHooks, ContextLayer, ContextMetadata, ContextSystem,
        Injection, NoopContextHandle,
    };

    struct InjectingHooks;

    #[async_trait]
    impl ContextHooks for InjectingHooks {
        fn name(&self) -> &str { "injecting" }

        async fn assemble(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            mut entries: Vec<ContextEntry>,
            _token_budget: usize,
        ) -> Vec<ContextEntry> {
            entries.push(ContextEntry {
                id: "smuggled".into(),
                message: AgentMessage::Standard(Message::user("smuggled-by-assemble")),
                metadata: ContextMetadata::new(ContextLayer::RuntimeInject),
            });
            entries
        }

        async fn on_message(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            _message: &AgentMessage,
        ) -> Vec<Injection> {
            Vec::new()
        }
    }

    // Model that records every set of messages it's asked to complete on.
    #[derive(Default)]
    struct CapturingModel {
        captured: std::sync::Mutex<Vec<Vec<Message>>>,
    }
    #[async_trait]
    impl LanguageModel for CapturingModel {
        async fn complete(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            self.captured.lock().unwrap().push(messages.to_vec());
            Ok(CompletionResponse::from_message(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text { text: "ok".into() }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            }))
        }
        fn stream(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            self.captured.lock().unwrap().push(messages.to_vec());
            Box::pin(futures::stream::iter(vec![
                StreamEvent::TextDelta { text: "ok".into() },
                StreamEvent::Done,
            ]))
        }
        fn model_id(&self) -> &str { "capturing" }
    }

    let model = Arc::new(CapturingModel::default());
    let captured_handle = model.clone();

    let hooks: Arc<dyn ContextHooks> = Arc::new(InjectingHooks);
    let handle: Arc<dyn ContextHandle> = Arc::new(NoopContextHandle);
    let cs = Arc::new(ContextSystem::new(hooks, handle));

    let mut state = AgentState {
        model,
        tools: vec![],
        session: Arc::new(InMemoryAgentSession::new()),
        extensions: alva_kernel_core::shared::Extensions::new(),
    };
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: String::new(),
        max_iterations: 5,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: Some(cs),
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("user-input"))],
        tx,
    )
    .await;
    assert!(result.is_ok(), "run_agent should succeed: {:?}", result);

    let captured = captured_handle.captured.lock().unwrap();
    assert_eq!(captured.len(), 1, "exactly one LLM call");
    let messages = &captured[0];
    let texts: Vec<String> = messages.iter().map(|m| m.text_content()).collect();
    assert!(
        texts.iter().any(|t| t.contains("user-input")),
        "should still contain user input: {:?}",
        texts
    );
    assert!(
        texts.iter().any(|t| t.contains("smuggled-by-assemble")),
        "assemble hook should have injected an extra message: {:?}",
        texts
    );
}

#[tokio::test]
async fn on_budget_exceeded_sliding_window_drops_old_messages() {
    // Pre-populate session with 30 messages, set a budget that triggers
    // immediately, and assert the LLM only sees the sliding-window keep_recent.
    use alva_kernel_abi::scope::context::{
        CompressAction, ContextHandle, ContextHooks, ContextSnapshot, ContextSystem, Injection,
        NoopContextHandle,
    };

    struct WindowHooks {
        keep: usize,
        fired: Arc<AtomicUsize>,
    }
    #[async_trait]
    impl ContextHooks for WindowHooks {
        fn name(&self) -> &str { "window" }

        async fn on_budget_exceeded(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            _snapshot: &ContextSnapshot,
        ) -> Vec<CompressAction> {
            self.fired.fetch_add(1, Ordering::SeqCst);
            vec![CompressAction::SlidingWindow { keep_recent: self.keep }]
        }

        async fn on_message(
            &self,
            _sdk: &dyn ContextHandle,
            _agent_id: &str,
            _message: &AgentMessage,
        ) -> Vec<Injection> {
            Vec::new()
        }
    }

    #[derive(Default)]
    struct CapturingModel {
        captured: std::sync::Mutex<Vec<Vec<Message>>>,
    }
    #[async_trait]
    impl LanguageModel for CapturingModel {
        async fn complete(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            self.captured.lock().unwrap().push(messages.to_vec());
            Ok(CompletionResponse::from_message(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text { text: "ok".into() }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            }))
        }
        fn stream(
            &self,
            messages: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            self.captured.lock().unwrap().push(messages.to_vec());
            Box::pin(futures::stream::iter(vec![
                StreamEvent::TextDelta { text: "ok".into() },
                StreamEvent::Done,
            ]))
        }
        fn model_id(&self) -> &str { "capturing" }
    }

    let fired = Arc::new(AtomicUsize::new(0));
    let hooks: Arc<dyn ContextHooks> = Arc::new(WindowHooks {
        keep: 5,
        fired: fired.clone(),
    });
    let handle: Arc<dyn ContextHandle> = Arc::new(NoopContextHandle);
    let cs = Arc::new(ContextSystem::new(hooks, handle));

    let session = Arc::new(InMemoryAgentSession::new());
    {
        let s: &dyn alva_kernel_abi::agent_session::AgentSession = session.as_ref();
        for i in 0..30 {
            s.append_message(AgentMessage::Standard(Message::user(&format!(
                "msg-{}-with-some-padding-text-to-pump-up-the-token-estimate",
                i
            ))), None).await;
        }
    }

    let model = Arc::new(CapturingModel::default());
    let captured_handle = model.clone();

    let mut state = AgentState {
        model,
        tools: vec![],
        session,
        extensions: alva_kernel_core::shared::Extensions::new(),
    };
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: String::new(),
        max_iterations: 5,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: Some(cs),
        context_token_budget: Some(50),
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let result = run_agent(&mut state, &config, cancel, vec![], tx).await;
    assert!(result.is_ok(), "run_agent should succeed: {:?}", result);

    assert_eq!(fired.load(Ordering::SeqCst), 1, "on_budget_exceeded fired once");

    let captured = captured_handle.captured.lock().unwrap();
    assert_eq!(captured.len(), 1, "exactly one LLM call");
    let llm_msgs = &captured[0];
    assert_eq!(
        llm_msgs.len(),
        5,
        "sliding window should have kept 5 messages, got {}",
        llm_msgs.len()
    );
    let texts: Vec<String> = llm_msgs.iter().map(|m| m.text_content()).collect();
    assert!(texts[0].contains("msg-25"), "first kept = msg-25, got {}", texts[0]);
    assert!(texts[4].contains("msg-29"), "last kept = msg-29, got {}", texts[4]);
}

#[tokio::test]
async fn context_hooks_disabled_by_default() {
    // Sanity: when context_system is None, run_agent still works as before.
    let mut state = AgentState {
        model: Arc::new(EchoModel),
        tools: vec![],
        session: Arc::new(InMemoryAgentSession::new()),
        extensions: alva_kernel_core::shared::Extensions::new(),
    };
    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: String::new(),
        max_iterations: 5,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    let result = run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hi"))],
        tx,
    )
    .await;
    assert!(result.is_ok());
}
