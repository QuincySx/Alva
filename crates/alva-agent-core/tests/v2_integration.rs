// V2 integration tests — full agent run with session + middleware.

use std::sync::Arc;

use alva_agent_core::builtins::{DanglingToolCallMiddleware, LoopDetectionMiddleware};
use alva_agent_core::middleware::MiddlewareStack;
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
use alva_types::{AgentMessage, CancellationToken, ModelConfig};
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

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn make_state() -> AgentState {
    AgentState {
        model: Arc::new(EchoModel),
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
        loop_hook: None,
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
        loop_hook: None,
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
        loop_hook: None,
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
        loop_hook: None,
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

    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Echo bot.".to_string(),
        max_iterations: 100,
        model_config: ModelConfig::default(),
        context_window: 0,
        loop_hook: Some(mailbox),
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
    assert_eq!(messages.len(), 4, "expected 4 messages, got {}", messages.len());

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
        loop_hook: None,
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
