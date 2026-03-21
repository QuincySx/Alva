use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use futures::stream;

use srow_ai::chat::*;
use srow_ai::transport::*;

use srow_core::error::*;
use srow_core::ui_message::*;
use srow_core::ui_message_stream::*;

// ---------------------------------------------------------------------------
// Mock Transport
// ---------------------------------------------------------------------------

struct MockTransport {
    chunks: Vec<UIMessageChunk>,
}

#[async_trait]
impl ChatTransport for MockTransport {
    async fn send_messages(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        let items: Vec<Result<UIMessageChunk, StreamError>> =
            self.chunks.iter().cloned().map(Ok).collect();
        Ok(Box::pin(stream::iter(items)))
    }

    async fn reconnect(
        &self,
        _chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        Ok(None)
    }
}

/// A transport that returns an error on send.
struct ErrorTransport;

#[async_trait]
impl ChatTransport for ErrorTransport {
    async fn send_messages(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        Err(TransportError::Http("mock error".into()))
    }

    async fn reconnect(
        &self,
        _chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        Ok(None)
    }
}

/// A transport that returns a stream with a long delay for testing stop/abort.
struct SlowTransport;

#[async_trait]
impl ChatTransport for SlowTransport {
    async fn send_messages(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        // Return a stream that takes a while to produce items.
        let s = async_stream::stream! {
            yield Ok(UIMessageChunk::Start { message_id: None, message_metadata: None });
            yield Ok(UIMessageChunk::TextStart { id: "t1".into() });
            // Long pause.
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            yield Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "slow".into() });
            yield Ok(UIMessageChunk::TextEnd { id: "t1".into() });
            yield Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None });
        };
        Ok(Box::pin(s))
    }

    async fn reconnect(
        &self,
        _chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        Ok(None)
    }
}

// ---------------------------------------------------------------------------
// Test ChatState
// ---------------------------------------------------------------------------

struct TestChatState {
    messages: Vec<UIMessage>,
    status: ChatStatus,
    error: Option<ChatError>,
    /// Record status transitions for verifying lifecycle.
    status_history: Vec<ChatStatus>,
}

impl TestChatState {
    fn new() -> Self {
        Self {
            messages: vec![],
            status: ChatStatus::Ready,
            error: None,
            status_history: vec![ChatStatus::Ready],
        }
    }
}

impl ChatState for TestChatState {
    fn messages(&self) -> Vec<UIMessage> {
        self.messages.clone()
    }
    fn set_messages(&mut self, m: Vec<UIMessage>) {
        self.messages = m;
    }
    fn push_message(&mut self, m: UIMessage) {
        self.messages.push(m);
    }
    fn pop_message(&mut self) -> Option<UIMessage> {
        self.messages.pop()
    }
    fn replace_message(&mut self, i: usize, m: UIMessage) {
        if i < self.messages.len() {
            self.messages[i] = m;
        }
    }
    fn status(&self) -> ChatStatus {
        self.status.clone()
    }
    fn set_status(&mut self, s: ChatStatus) {
        self.status = s.clone();
        self.status_history.push(s);
    }
    fn error(&self) -> Option<ChatError> {
        self.error.clone()
    }
    fn set_error(&mut self, e: Option<ChatError>) {
        self.error = e;
    }
    fn notify_messages_changed(&mut self) {}
    fn notify_status_changed(&mut self) {}
    fn notify_error_changed(&mut self) {}
}

// ---------------------------------------------------------------------------
// Helper: build an AbstractChat with a given transport
// ---------------------------------------------------------------------------

fn build_chat(transport: impl ChatTransport + 'static) -> AbstractChat<TestChatState> {
    let handle = tokio::runtime::Handle::current();
    let counter_mutex = Mutex::new(0u64);

    AbstractChat::new(ChatInit {
        id: "test-chat".into(),
        state: TestChatState::new(),
        transport: Box::new(transport),
        runtime_handle: handle,
        generate_id: Some(Box::new(move || {
            let mut c = counter_mutex.lock().unwrap();
            *c += 1;
            format!("id-{}", *c)
        })),
        initial_messages: vec![],
        on_tool_call: None,
        on_finish: None,
        on_error: None,
        send_automatically_when: None,
    })
}

fn build_chat_with_auto_send(
    transport: impl ChatTransport + 'static,
) -> AbstractChat<TestChatState> {
    let handle = tokio::runtime::Handle::current();
    let counter_mutex = Mutex::new(0u64);

    AbstractChat::new(ChatInit {
        id: "test-chat".into(),
        state: TestChatState::new(),
        transport: Box::new(transport),
        runtime_handle: handle,
        generate_id: Some(Box::new(move || {
            let mut c = counter_mutex.lock().unwrap();
            *c += 1;
            format!("id-{}", *c)
        })),
        initial_messages: vec![],
        on_tool_call: None,
        on_finish: None,
        on_error: None,
        send_automatically_when: Some(Box::new(|_msg| true)),
    })
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

/// Helper: standard chunks for a simple "Hello" text response.
fn hello_chunks() -> Vec<UIMessageChunk> {
    vec![
        UIMessageChunk::Start {
            message_id: None,
            message_metadata: None,
        },
        UIMessageChunk::TextStart {
            id: "t1".into(),
        },
        UIMessageChunk::TextDelta {
            id: "t1".into(),
            delta: "Hello".into(),
        },
        UIMessageChunk::TextEnd {
            id: "t1".into(),
        },
        UIMessageChunk::Finish {
            finish_reason: FinishReason::Stop,
            usage: None,
        },
    ]
}

#[tokio::test]
async fn test_send_message_basic() {
    let chat = build_chat(MockTransport {
        chunks: hello_chunks(),
    });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // After completion we should have 2 messages: user + assistant.
    let messages = chat.with_state(|s| s.messages());
    assert_eq!(messages.len(), 2, "expected 2 messages (user + assistant)");

    // First message is the user message.
    assert_eq!(messages[0].role, UIMessageRole::User);
    match &messages[0].parts[0] {
        UIMessagePart::Text { text, .. } => assert_eq!(text, "Hi"),
        other => panic!("expected Text part, got {:?}", other),
    }

    // Second message is the assistant message.
    assert_eq!(messages[1].role, UIMessageRole::Assistant);
    assert!(!messages[1].parts.is_empty(), "assistant should have parts");

    match &messages[1].parts[0] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "Hello");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text part, got {:?}", other),
    }

    // Status should be Ready.
    let status = chat.with_state(|s| s.status());
    assert_eq!(status, ChatStatus::Ready);
}

#[tokio::test]
async fn test_send_message_status_transitions() {
    let chat = build_chat(MockTransport {
        chunks: hello_chunks(),
    });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // Check that we went through Submitted → Streaming → Ready.
    let history = chat.with_state(|s| s.status_history.clone());
    // history[0] = Ready (initial), then Submitted, Streaming, Ready.
    assert!(
        history.contains(&ChatStatus::Submitted),
        "status history should contain Submitted, got {:?}",
        history
    );
    assert!(
        history.contains(&ChatStatus::Streaming),
        "status history should contain Streaming, got {:?}",
        history
    );
    // The last entry should be Ready.
    assert_eq!(
        history.last().unwrap(),
        &ChatStatus::Ready,
        "final status should be Ready"
    );
}

#[tokio::test]
async fn test_stop_aborts() {
    let chat = Arc::new(build_chat(SlowTransport));

    let chat_clone = chat.clone();
    let send_handle = tokio::spawn(async move {
        chat_clone
            .send_message(
                vec![UIMessagePart::Text {
                    text: "Hi".into(),
                    state: None,
                }],
                SendOptions::default(),
            )
            .await;
    });

    // Give the stream a moment to start.
    tokio::time::sleep(std::time::Duration::from_millis(100)).await;

    // Stop the chat.
    chat.stop().await;

    // The send should complete relatively quickly after stop (not wait 10s).
    let result = tokio::time::timeout(std::time::Duration::from_secs(3), send_handle).await;
    assert!(result.is_ok(), "send_message should complete after stop");

    // Status should be Ready (stop sets it).
    let status = chat.with_state(|s| s.status());
    assert_eq!(status, ChatStatus::Ready);
}

#[tokio::test]
async fn test_tool_approval_request() {
    let chunks = vec![
        UIMessageChunk::Start {
            message_id: None,
            message_metadata: None,
        },
        UIMessageChunk::ToolInputStart {
            id: "tool-1".into(),
            tool_name: "my_tool".into(),
            title: None,
        },
        UIMessageChunk::ToolInputAvailable {
            id: "tool-1".into(),
            input: serde_json::json!({"key": "value"}),
        },
        UIMessageChunk::ToolApprovalRequest {
            id: "tool-1".into(),
        },
        UIMessageChunk::Finish {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ];

    let chat = build_chat(MockTransport { chunks });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "run tool".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    let messages = chat.with_state(|s| s.messages());
    assert_eq!(messages.len(), 2);

    let assistant = &messages[1];
    assert_eq!(assistant.role, UIMessageRole::Assistant);

    // Find the Tool part.
    let tool_part = assistant
        .parts
        .iter()
        .find(|p| matches!(p, UIMessagePart::Tool { .. }));
    assert!(tool_part.is_some(), "should have a Tool part");

    match tool_part.unwrap() {
        UIMessagePart::Tool { id, state, tool_name, input, .. } => {
            assert_eq!(id, "tool-1");
            assert_eq!(tool_name, "my_tool");
            assert_eq!(*state, ToolState::ApprovalRequested);
            assert_eq!(*input, serde_json::json!({"key": "value"}));
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn test_add_tool_output() {
    let chunks = vec![
        UIMessageChunk::Start {
            message_id: None,
            message_metadata: None,
        },
        UIMessageChunk::ToolInputStart {
            id: "tool-1".into(),
            tool_name: "my_tool".into(),
            title: None,
        },
        UIMessageChunk::ToolInputAvailable {
            id: "tool-1".into(),
            input: serde_json::json!({"key": "value"}),
        },
        UIMessageChunk::Finish {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ];

    let chat = build_chat(MockTransport {
        chunks: chunks.clone(),
    });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "run tool".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // The tool should be in InputAvailable state.
    let messages = chat.with_state(|s| s.messages());
    let tool_part = messages[1]
        .parts
        .iter()
        .find(|p| matches!(p, UIMessagePart::Tool { .. }))
        .unwrap();
    match tool_part {
        UIMessagePart::Tool { state, .. } => {
            assert_eq!(*state, ToolState::InputAvailable);
        }
        _ => unreachable!(),
    }

    // Now add tool output.
    let output = serde_json::json!({"result": "success"});
    chat.add_tool_output("tool-1", output.clone()).await;

    // Verify the tool part was updated.
    let messages = chat.with_state(|s| s.messages());
    let tool_part = messages[1]
        .parts
        .iter()
        .find(|p| matches!(p, UIMessagePart::Tool { .. }))
        .unwrap();
    match tool_part {
        UIMessagePart::Tool {
            state,
            output: tool_output,
            ..
        } => {
            assert_eq!(*state, ToolState::OutputAvailable);
            assert_eq!(*tool_output, Some(output));
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn test_add_tool_approval_response() {
    let chunks = vec![
        UIMessageChunk::Start {
            message_id: None,
            message_metadata: None,
        },
        UIMessageChunk::ToolInputStart {
            id: "tool-1".into(),
            tool_name: "my_tool".into(),
            title: None,
        },
        UIMessageChunk::ToolInputAvailable {
            id: "tool-1".into(),
            input: serde_json::json!({}),
        },
        UIMessageChunk::ToolApprovalRequest {
            id: "tool-1".into(),
        },
        UIMessageChunk::Finish {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ];

    let chat = build_chat(MockTransport { chunks });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "run".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // Approve the tool call.
    chat.add_tool_approval_response("tool-1", true);

    let messages = chat.with_state(|s| s.messages());
    let tool_part = messages[1]
        .parts
        .iter()
        .find(|p| matches!(p, UIMessagePart::Tool { .. }))
        .unwrap();
    match tool_part {
        UIMessagePart::Tool { state, .. } => {
            assert_eq!(*state, ToolState::ApprovalResponded);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn test_add_tool_approval_denied() {
    let chunks = vec![
        UIMessageChunk::Start {
            message_id: None,
            message_metadata: None,
        },
        UIMessageChunk::ToolInputStart {
            id: "tool-1".into(),
            tool_name: "my_tool".into(),
            title: None,
        },
        UIMessageChunk::ToolInputAvailable {
            id: "tool-1".into(),
            input: serde_json::json!({}),
        },
        UIMessageChunk::ToolApprovalRequest {
            id: "tool-1".into(),
        },
        UIMessageChunk::Finish {
            finish_reason: FinishReason::ToolCalls,
            usage: None,
        },
    ];

    let chat = build_chat(MockTransport { chunks });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "run".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // Deny the tool call.
    chat.add_tool_approval_response("tool-1", false);

    let messages = chat.with_state(|s| s.messages());
    let tool_part = messages[1]
        .parts
        .iter()
        .find(|p| matches!(p, UIMessagePart::Tool { .. }))
        .unwrap();
    match tool_part {
        UIMessagePart::Tool { state, .. } => {
            assert_eq!(*state, ToolState::OutputDenied);
        }
        _ => unreachable!(),
    }
}

#[tokio::test]
async fn test_clear_error() {
    // ErrorTransport causes send_messages to fail.
    let chat = build_chat(ErrorTransport);

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // Status should be Error.
    let status = chat.with_state(|s| s.status());
    assert_eq!(status, ChatStatus::Error);

    // Error should be set.
    let error = chat.with_state(|s| s.error());
    assert!(error.is_some(), "error should be set");

    // Clear error.
    chat.clear_error();

    let status = chat.with_state(|s| s.status());
    assert_eq!(status, ChatStatus::Ready);

    let error = chat.with_state(|s| s.error());
    assert!(error.is_none(), "error should be cleared");
}

#[tokio::test]
async fn test_regenerate() {
    let chat = build_chat(MockTransport {
        chunks: hello_chunks(),
    });

    // Send initial message.
    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    let messages = chat.with_state(|s| s.messages());
    assert_eq!(messages.len(), 2);

    // Regenerate — removes last assistant message, then re-runs.
    chat.regenerate(RegenerateOptions::default()).await;

    // After regenerate, we should have 2 messages again (user + new assistant).
    let messages = chat.with_state(|s| s.messages());
    assert_eq!(
        messages.len(),
        2,
        "should have 2 messages after regenerate"
    );
    assert_eq!(messages[0].role, UIMessageRole::User);
    assert_eq!(messages[1].role, UIMessageRole::Assistant);
}

#[tokio::test]
async fn test_chat_id() {
    let chat = build_chat(MockTransport {
        chunks: hello_chunks(),
    });
    assert_eq!(chat.id(), "test-chat");
}

#[tokio::test]
async fn test_on_finish_callback() {
    let finish_called = Arc::new(Mutex::new(false));
    let finish_called_clone = finish_called.clone();

    let handle = tokio::runtime::Handle::current();
    let counter_mutex = Mutex::new(0u64);

    let chat = AbstractChat::new(ChatInit {
        id: "test-chat".into(),
        state: TestChatState::new(),
        transport: Box::new(MockTransport {
            chunks: hello_chunks(),
        }),
        runtime_handle: handle,
        generate_id: Some(Box::new(move || {
            let mut c = counter_mutex.lock().unwrap();
            *c += 1;
            format!("id-{}", *c)
        })),
        initial_messages: vec![],
        on_tool_call: None,
        on_finish: Some(Box::new(move |info: FinishInfo| {
            *finish_called_clone.lock().unwrap() = true;
            assert_eq!(info.finish_reason, FinishReason::Stop);
        })),
        on_error: None,
        send_automatically_when: None,
    });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    assert!(
        *finish_called.lock().unwrap(),
        "on_finish should have been called"
    );
}

#[tokio::test]
async fn test_on_error_callback() {
    let error_called = Arc::new(Mutex::new(false));
    let error_called_clone = error_called.clone();

    let handle = tokio::runtime::Handle::current();
    let counter_mutex = Mutex::new(0u64);

    let chat = AbstractChat::new(ChatInit {
        id: "test-chat".into(),
        state: TestChatState::new(),
        transport: Box::new(ErrorTransport),
        runtime_handle: handle,
        generate_id: Some(Box::new(move || {
            let mut c = counter_mutex.lock().unwrap();
            *c += 1;
            format!("id-{}", *c)
        })),
        initial_messages: vec![],
        on_tool_call: None,
        on_finish: None,
        on_error: Some(Box::new(move |_err: ChatError| {
            *error_called_clone.lock().unwrap() = true;
        })),
        send_automatically_when: None,
    });

    chat.send_message(
        vec![UIMessagePart::Text {
            text: "Hi".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    assert!(
        *error_called.lock().unwrap(),
        "on_error should have been called"
    );
}

/// Transport that returns tool-call chunks on first call, then text chunks on second.
struct AutoSendTransport {
    call_count: AtomicUsize,
}

#[async_trait]
impl ChatTransport for AutoSendTransport {
    async fn send_messages(
        &self,
        _request: ChatRequest,
    ) -> Result<
        Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>,
        TransportError,
    > {
        let n = self.call_count.fetch_add(1, Ordering::SeqCst);
        if n == 0 {
            // First call: return a tool call.
            let items: Vec<Result<UIMessageChunk, StreamError>> = vec![
                Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
                Ok(UIMessageChunk::ToolInputStart { id: "tool-1".into(), tool_name: "calc".into(), title: None }),
                Ok(UIMessageChunk::ToolInputAvailable { id: "tool-1".into(), input: serde_json::json!({}) }),
                Ok(UIMessageChunk::Finish { finish_reason: FinishReason::ToolCalls, usage: None }),
            ];
            Ok(Box::pin(stream::iter(items)))
        } else {
            // Second call: return text.
            let items: Vec<Result<UIMessageChunk, StreamError>> = vec![
                Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
                Ok(UIMessageChunk::TextStart { id: "t1".into() }),
                Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "Done".into() }),
                Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
                Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
            ];
            Ok(Box::pin(stream::iter(items)))
        }
    }

    async fn reconnect(
        &self,
        _chat_id: &str,
    ) -> Result<
        Option<Pin<Box<dyn futures::Stream<Item = Result<UIMessageChunk, StreamError>> + Send>>>,
        TransportError,
    > {
        Ok(None)
    }
}

#[tokio::test]
async fn test_auto_send_after_tool_output() {
    let chat = build_chat_with_auto_send(AutoSendTransport {
        call_count: AtomicUsize::new(0),
    });

    // Send a message. The transport returns a tool call.
    chat.send_message(
        vec![UIMessagePart::Text {
            text: "compute".into(),
            state: None,
        }],
        SendOptions::default(),
    )
    .await;

    // We should have user + assistant(tool call) = 2 messages.
    let messages = chat.with_state(|s| s.messages());
    assert_eq!(messages.len(), 2);

    // Now add tool output. This should trigger auto-send, which will
    // call the transport again with the second (text) response.
    chat.add_tool_output("tool-1", serde_json::json!({"result": 42}))
        .await;

    // After auto-send completes, we should have 3 messages:
    // user + assistant(tool) + assistant(text)
    let messages = chat.with_state(|s| s.messages());
    assert_eq!(
        messages.len(),
        3,
        "auto-send should have added a third message, got {:?}",
        messages.iter().map(|m| &m.role).collect::<Vec<_>>()
    );

    // The third message should be the text response.
    assert_eq!(messages[2].role, UIMessageRole::Assistant);
    match &messages[2].parts[0] {
        UIMessagePart::Text { text, .. } => assert_eq!(text, "Done"),
        other => panic!("expected Text part, got {:?}", other),
    }
}
