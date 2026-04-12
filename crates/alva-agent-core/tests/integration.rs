// Integration tests — full agent run with session + middleware.

use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use alva_agent_core::builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};
use alva_agent_core::middleware::{Middleware, MiddlewareError, MiddlewareStack};
use alva_agent_core::run::run_agent;
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::AgentEvent;
use alva_types::base::content::ContentBlock;
use alva_types::base::error::AgentError;
use alva_types::base::message::{Message, MessageRole};
use alva_types::base::stream::StreamEvent;
use alva_types::model::LanguageModel;
use alva_types::session::InMemorySession;
use alva_types::tool::Tool;
use alva_types::{
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
        session: Arc::new(InMemorySession::new()),
        extensions: alva_agent_core::shared::Extensions::new(),
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
    let messages = state.session.messages();
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

    let messages = state.session.messages();
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
    assert_eq!(state.session.messages().len(), 2);

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
    let messages = state.session.messages();
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
    let mailbox = Arc::new(alva_agent_core::PendingMessageQueue::new());
    mailbox.follow_up(AgentMessage::Standard(Message::user("follow-up question")));

    // Register the mailbox on a bus as dyn AgentLoopHook
    let bus = Bus::new();
    let bus_writer = bus.writer();
    bus_writer.provide::<dyn alva_agent_core::pending_queue::AgentLoopHook>(
        mailbox as Arc<dyn alva_agent_core::pending_queue::AgentLoopHook>,
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
    let messages = state.session.messages();
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
    assert_eq!(state.session.messages().len(), 2);
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
    ) -> Result<Message, AgentError> {
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
    ) -> Result<Message, AgentError> {
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
        state.session.messages().len(),
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
    ) -> Result<Message, AgentError> {
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

    assert_eq!(state.session.messages().len(), 1);
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
        state.session.messages().len(),
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
    ) -> Result<Message, AgentError> {
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
