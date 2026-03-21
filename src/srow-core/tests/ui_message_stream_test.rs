use srow_core::ui_message_stream::{UIMessageChunk, FinishReason, ChatStatus, TokenUsage};
use srow_core::ui_message_stream::processor::{process_ui_message_stream, UIMessageStreamUpdate};
use srow_core::ui_message_stream::sse::{parse_sse_stream, chunk_to_sse, sse_done};
use srow_core::ui_message::convert::llm_stream_to_ui_chunks;
use srow_core::ui_message::{UIMessage, UIMessagePart, UIMessageRole, TextPartState, ToolState};
use srow_core::ports::llm_provider::{StreamChunk, LLMResponse, StopReason, TokenUsage as LLMTokenUsage};
use srow_core::domain::message::LLMContent;
use srow_core::error::StreamError;
use bytes::Bytes;
use futures::stream;
use futures::StreamExt;
use tokio::sync::mpsc;

#[test]
fn start_chunk_with_all_fields() {
    let chunk = UIMessageChunk::Start {
        message_id: Some("msg-1".into()),
        message_metadata: Some(serde_json::json!({"key": "value"})),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "start");
    assert_eq!(json["message_id"], "msg-1");
    assert_eq!(json["message_metadata"]["key"], "value");

    // Round-trip
    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::Start { message_id, message_metadata } => {
            assert_eq!(message_id.unwrap(), "msg-1");
            assert!(message_metadata.is_some());
        }
        _ => panic!("expected Start"),
    }
}

#[test]
fn start_chunk_without_optional_fields() {
    let chunk = UIMessageChunk::Start {
        message_id: None,
        message_metadata: None,
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "start");
    assert!(json.get("message_id").is_none());
    assert!(json.get("message_metadata").is_none());

    // Round-trip
    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::Start { message_id, message_metadata } => {
            assert!(message_id.is_none());
            assert!(message_metadata.is_none());
        }
        _ => panic!("expected Start"),
    }
}

#[test]
fn text_delta_chunk_serialization() {
    let chunk = UIMessageChunk::TextDelta {
        id: "t1".into(),
        delta: "hello".into(),
    };
    let json_str = serde_json::to_string(&chunk).unwrap();
    let json: serde_json::Value = serde_json::from_str(&json_str).unwrap();
    assert_eq!(json["type"], "text-delta");
    assert_eq!(json["id"], "t1");
    assert_eq!(json["delta"], "hello");

    // Round-trip
    let decoded: UIMessageChunk = serde_json::from_str(&json_str).unwrap();
    match decoded {
        UIMessageChunk::TextDelta { id, delta } => {
            assert_eq!(id, "t1");
            assert_eq!(delta, "hello");
        }
        _ => panic!("expected TextDelta"),
    }
}

#[test]
fn tool_input_start_chunk() {
    let chunk = UIMessageChunk::ToolInputStart {
        id: "tc-1".into(),
        tool_name: "read_file".into(),
        title: Some("Reading file".into()),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "tool-input-start");
    assert_eq!(json["id"], "tc-1");
    assert_eq!(json["tool_name"], "read_file");
    assert_eq!(json["title"], "Reading file");

    // Round-trip
    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::ToolInputStart { id, tool_name, title } => {
            assert_eq!(id, "tc-1");
            assert_eq!(tool_name, "read_file");
            assert_eq!(title.unwrap(), "Reading file");
        }
        _ => panic!("expected ToolInputStart"),
    }
}

#[test]
fn tool_approval_request_chunk() {
    let chunk = UIMessageChunk::ToolApprovalRequest {
        id: "tc-2".into(),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "tool-approval-request");
    assert_eq!(json["id"], "tc-2");

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::ToolApprovalRequest { id } => assert_eq!(id, "tc-2"),
        _ => panic!("expected ToolApprovalRequest"),
    }
}

#[test]
fn finish_chunk_with_stop() {
    let chunk = UIMessageChunk::Finish {
        finish_reason: FinishReason::Stop,
        usage: Some(TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
        }),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "finish");
    assert_eq!(json["finish_reason"], "stop");
    assert_eq!(json["usage"]["input_tokens"], 100);
    assert_eq!(json["usage"]["output_tokens"], 50);

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::Finish { finish_reason, usage } => {
            assert_eq!(finish_reason, FinishReason::Stop);
            let u = usage.unwrap();
            assert_eq!(u.input_tokens, 100);
            assert_eq!(u.output_tokens, 50);
        }
        _ => panic!("expected Finish"),
    }
}

#[test]
fn error_chunk() {
    let chunk = UIMessageChunk::Error {
        error: "something went wrong".into(),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "error");
    assert_eq!(json["error"], "something went wrong");

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::Error { error } => assert_eq!(error, "something went wrong"),
        _ => panic!("expected Error"),
    }
}

#[test]
fn token_usage_chunk() {
    let chunk = UIMessageChunk::TokenUsage {
        usage: TokenUsage {
            input_tokens: 200,
            output_tokens: 300,
        },
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "token-usage");
    assert_eq!(json["usage"]["input_tokens"], 200);
    assert_eq!(json["usage"]["output_tokens"], 300);

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::TokenUsage { usage } => {
            assert_eq!(usage.input_tokens, 200);
            assert_eq!(usage.output_tokens, 300);
        }
        _ => panic!("expected TokenUsage"),
    }
}

#[test]
fn data_chunk() {
    let chunk = UIMessageChunk::Data {
        name: "context".into(),
        data: serde_json::json!({"files": ["a.rs", "b.rs"]}),
    };
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "data");
    assert_eq!(json["name"], "context");
    assert_eq!(json["data"]["files"][0], "a.rs");

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    match decoded {
        UIMessageChunk::Data { name, data } => {
            assert_eq!(name, "context");
            assert_eq!(data["files"][1], "b.rs");
        }
        _ => panic!("expected Data"),
    }
}

#[test]
fn chat_status_serialization() {
    let cases = vec![
        (ChatStatus::Ready, "ready"),
        (ChatStatus::Submitted, "submitted"),
        (ChatStatus::Streaming, "streaming"),
        (ChatStatus::Error, "error"),
    ];
    for (status, expected_str) in cases {
        let json = serde_json::to_value(&status).unwrap();
        assert_eq!(json, expected_str, "ChatStatus::{:?} should serialize to {:?}", status, expected_str);
        let decoded: ChatStatus = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, status);
    }
}

#[test]
fn finish_reason_serialization() {
    let cases = vec![
        (FinishReason::Stop, "stop"),
        (FinishReason::ToolCalls, "tool-calls"),
        (FinishReason::MaxTokens, "max-tokens"),
        (FinishReason::Error, "error"),
        (FinishReason::Abort, "abort"),
    ];
    for (reason, expected_str) in cases {
        let json = serde_json::to_value(&reason).unwrap();
        assert_eq!(json, expected_str, "FinishReason::{:?} should serialize to {:?}", reason, expected_str);
        let decoded: FinishReason = serde_json::from_value(json).unwrap();
        assert_eq!(decoded, reason);
    }
}

#[test]
fn abort_chunk() {
    let chunk = UIMessageChunk::Abort;
    let json = serde_json::to_value(&chunk).unwrap();
    assert_eq!(json["type"], "abort");

    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    assert!(matches!(decoded, UIMessageChunk::Abort));
}

#[test]
fn step_chunks() {
    let start = UIMessageChunk::StartStep;
    let json = serde_json::to_value(&start).unwrap();
    assert_eq!(json["type"], "start-step");
    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    assert!(matches!(decoded, UIMessageChunk::StartStep));

    let finish = UIMessageChunk::FinishStep;
    let json = serde_json::to_value(&finish).unwrap();
    assert_eq!(json["type"], "finish-step");
    let decoded: UIMessageChunk = serde_json::from_value(json).unwrap();
    assert!(matches!(decoded, UIMessageChunk::FinishStep));
}

// ---------------------------------------------------------------------------
// Processor tests
// ---------------------------------------------------------------------------

fn make_initial_message() -> UIMessage {
    UIMessage {
        id: "msg-init".into(),
        role: UIMessageRole::Assistant,
        parts: vec![],
        metadata: None,
    }
}

/// Collect all updates from the channel into a Vec.
fn collect_updates(mut rx: mpsc::UnboundedReceiver<UIMessageStreamUpdate>) -> Vec<UIMessageStreamUpdate> {
    let mut updates = vec![];
    while let Ok(u) = rx.try_recv() {
        updates.push(u);
    }
    updates
}

#[tokio::test]
async fn processor_text_only_stream() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: Some("msg-1".into()), message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "Hello".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: " world".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();

    // Final message assertions
    assert_eq!(result.message.id, "msg-1");
    assert_eq!(result.message.parts.len(), 1);
    match &result.message.parts[0] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "Hello world");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text part, got {:?}", other),
    }
    assert_eq!(result.finish_reason, Some(FinishReason::Stop));
    assert!(result.active_text_parts.is_empty());

    // Updates assertions
    let updates = collect_updates(rx);
    // Should have: FirstWrite (TextStart), MessageChanged (TextDelta x2), MessageChanged (TextEnd), Finished
    assert!(updates.len() >= 3, "expected at least 3 updates, got {}", updates.len());
    assert!(matches!(&updates[0], UIMessageStreamUpdate::FirstWrite(_)));
    // All middle ones should be MessageChanged
    for u in &updates[1..updates.len()-1] {
        assert!(matches!(u, UIMessageStreamUpdate::MessageChanged(_)));
    }
    assert!(matches!(&updates[updates.len()-1], UIMessageStreamUpdate::Finished { .. }));

    // Verify Finished carries correct data
    if let UIMessageStreamUpdate::Finished { message, finish_reason, usage } = &updates[updates.len()-1] {
        assert_eq!(message.parts.len(), 1);
        assert_eq!(*finish_reason, Some(FinishReason::Stop));
        assert!(usage.is_none());
    }
}

#[tokio::test]
async fn processor_tool_call_stream() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "Hi".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::ToolInputStart {
            id: "tc1".into(),
            tool_name: "shell".into(),
            title: Some("Running shell".into()),
        }),
        Ok(UIMessageChunk::ToolInputDelta { id: "tc1".into(), delta: r#"{"cmd":"#.into() }),
        Ok(UIMessageChunk::ToolInputDelta { id: "tc1".into(), delta: r#""ls"}"#.into() }),
        Ok(UIMessageChunk::ToolInputAvailable {
            id: "tc1".into(),
            input: serde_json::json!({"cmd": "ls"}),
        }),
        Ok(UIMessageChunk::ToolOutputAvailable {
            id: "tc1".into(),
            output: serde_json::json!({"files": ["a.txt"]}),
        }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();

    assert_eq!(result.message.parts.len(), 2);

    // Text part
    match &result.message.parts[0] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "Hi");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text, got {:?}", other),
    }

    // Tool part
    match &result.message.parts[1] {
        UIMessagePart::Tool { id, tool_name, input, state, output, error, title } => {
            assert_eq!(id, "tc1");
            assert_eq!(tool_name, "shell");
            assert_eq!(*input, serde_json::json!({"cmd": "ls"}));
            assert_eq!(*state, ToolState::OutputAvailable);
            assert_eq!(*output, Some(serde_json::json!({"files": ["a.txt"]})));
            assert!(error.is_none());
            assert_eq!(*title, Some("Running shell".into()));
        }
        other => panic!("expected Tool, got {:?}", other),
    }

    // partial_tool_calls should be cleared after ToolInputAvailable
    assert!(result.partial_tool_calls.is_empty());

    let updates = collect_updates(rx);
    assert!(matches!(&updates[0], UIMessageStreamUpdate::FirstWrite(_)));
    assert!(matches!(&updates[updates.len()-1], UIMessageStreamUpdate::Finished { .. }));
}

#[tokio::test]
async fn processor_approval_flow() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::ToolInputStart {
            id: "tc1".into(),
            tool_name: "delete_file".into(),
            title: None,
        }),
        Ok(UIMessageChunk::ToolInputAvailable {
            id: "tc1".into(),
            input: serde_json::json!({"path": "/tmp/x"}),
        }),
        Ok(UIMessageChunk::ToolApprovalRequest { id: "tc1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::ToolCalls, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();

    assert_eq!(result.message.parts.len(), 1);
    match &result.message.parts[0] {
        UIMessagePart::Tool { state, .. } => {
            assert_eq!(*state, ToolState::ApprovalRequested);
        }
        other => panic!("expected Tool, got {:?}", other),
    }
    assert_eq!(result.finish_reason, Some(FinishReason::ToolCalls));
}

#[tokio::test]
async fn processor_multi_step_with_finish_step() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "Step one".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::FinishStep),
        Ok(UIMessageChunk::TextStart { id: "t2".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t2".into(), delta: "Step two".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t2".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();

    // Should have two text parts
    assert_eq!(result.message.parts.len(), 2);

    match &result.message.parts[0] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "Step one");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text, got {:?}", other),
    }
    match &result.message.parts[1] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "Step two");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text, got {:?}", other),
    }

    // active_text_parts should be empty (cleared by FinishStep + TextEnd)
    assert!(result.active_text_parts.is_empty());
}

#[tokio::test]
async fn processor_first_write_sent_only_once() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "A".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "B".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "C".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, rx) = mpsc::unbounded_channel();

    let _ = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();

    let updates = collect_updates(rx);

    // Exactly one FirstWrite
    let first_write_count = updates.iter().filter(|u| matches!(u, UIMessageStreamUpdate::FirstWrite(_))).count();
    assert_eq!(first_write_count, 1, "expected exactly 1 FirstWrite, got {}", first_write_count);

    // First content update is FirstWrite
    assert!(matches!(&updates[0], UIMessageStreamUpdate::FirstWrite(_)));

    // Remaining content updates are MessageChanged
    let message_changed_count = updates.iter().filter(|u| matches!(u, UIMessageStreamUpdate::MessageChanged(_))).count();
    // TextStart(1 FirstWrite) + TextDelta x3 + TextEnd = 4 MessageChanged
    assert_eq!(message_changed_count, 4, "expected 4 MessageChanged, got {}", message_changed_count);

    // Last update is Finished
    assert!(matches!(&updates[updates.len()-1], UIMessageStreamUpdate::Finished { .. }));
}

#[tokio::test]
async fn processor_error_chunk_returns_error() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::Error { error: "server failure".into() }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await;
    assert!(result.is_err());
    match result.unwrap_err() {
        StreamError::InvalidChunk(msg) => assert_eq!(msg, "server failure"),
        other => panic!("expected InvalidChunk, got {:?}", other),
    }
}

#[tokio::test]
async fn processor_abort_chunk_returns_error() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::Abort),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamError::Aborted));
}

#[tokio::test]
async fn processor_token_usage_carried_in_finished() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "ok".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::TokenUsage { usage: TokenUsage { input_tokens: 10, output_tokens: 20 } }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, rx) = mpsc::unbounded_channel();

    let _ = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    let updates = collect_updates(rx);
    if let UIMessageStreamUpdate::Finished { usage, .. } = &updates[updates.len()-1] {
        let u = usage.as_ref().expect("usage should be present");
        assert_eq!(u.input_tokens, 10);
        assert_eq!(u.output_tokens, 20);
    } else {
        panic!("last update should be Finished");
    }
}

#[tokio::test]
async fn processor_finish_usage_overrides_token_usage() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::TokenUsage { usage: TokenUsage { input_tokens: 10, output_tokens: 20 } }),
        Ok(UIMessageChunk::Finish {
            finish_reason: FinishReason::Stop,
            usage: Some(TokenUsage { input_tokens: 100, output_tokens: 200 }),
        }),
    ];
    let s = stream::iter(chunks);
    let (tx, rx) = mpsc::unbounded_channel();

    let _ = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    let updates = collect_updates(rx);
    if let UIMessageStreamUpdate::Finished { usage, .. } = &updates[updates.len()-1] {
        let u = usage.as_ref().expect("usage should be present");
        assert_eq!(u.input_tokens, 100);
        assert_eq!(u.output_tokens, 200);
    } else {
        panic!("last update should be Finished");
    }
}

#[tokio::test]
async fn processor_tool_input_error() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::ToolInputStart {
            id: "tc1".into(),
            tool_name: "shell".into(),
            title: None,
        }),
        Ok(UIMessageChunk::ToolInputDelta { id: "tc1".into(), delta: "bad json".into() }),
        Ok(UIMessageChunk::ToolInputError { id: "tc1".into(), error: "parse error".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Error, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    match &result.message.parts[0] {
        UIMessagePart::Tool { state, error, .. } => {
            assert_eq!(*state, ToolState::OutputError);
            assert_eq!(*error, Some("parse error".into()));
        }
        other => panic!("expected Tool, got {:?}", other),
    }
}

#[tokio::test]
async fn processor_tool_output_denied() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::ToolInputStart {
            id: "tc1".into(),
            tool_name: "rm".into(),
            title: None,
        }),
        Ok(UIMessageChunk::ToolInputAvailable {
            id: "tc1".into(),
            input: serde_json::json!({"path": "/"}),
        }),
        Ok(UIMessageChunk::ToolApprovalRequest { id: "tc1".into() }),
        Ok(UIMessageChunk::ToolOutputDenied { id: "tc1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    match &result.message.parts[0] {
        UIMessagePart::Tool { state, .. } => {
            assert_eq!(*state, ToolState::OutputDenied);
        }
        other => panic!("expected Tool, got {:?}", other),
    }
}

#[tokio::test]
async fn processor_tool_output_error() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::ToolInputStart {
            id: "tc1".into(),
            tool_name: "shell".into(),
            title: None,
        }),
        Ok(UIMessageChunk::ToolInputAvailable {
            id: "tc1".into(),
            input: serde_json::json!({"cmd": "fail"}),
        }),
        Ok(UIMessageChunk::ToolOutputError { id: "tc1".into(), error: "exit code 1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    match &result.message.parts[0] {
        UIMessagePart::Tool { state, error, .. } => {
            assert_eq!(*state, ToolState::OutputError);
            assert_eq!(*error, Some("exit code 1".into()));
        }
        other => panic!("expected Tool, got {:?}", other),
    }
}

#[tokio::test]
async fn processor_reasoning_stream() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::ReasoningStart { id: "r1".into() }),
        Ok(UIMessageChunk::ReasoningDelta { id: "r1".into(), delta: "thinking...".into() }),
        Ok(UIMessageChunk::ReasoningEnd { id: "r1".into() }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "answer".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    assert_eq!(result.message.parts.len(), 2);

    match &result.message.parts[0] {
        UIMessagePart::Reasoning { text, state } => {
            assert_eq!(text, "thinking...");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Reasoning, got {:?}", other),
    }
    match &result.message.parts[1] {
        UIMessagePart::Text { text, state } => {
            assert_eq!(text, "answer");
            assert_eq!(*state, Some(TextPartState::Done));
        }
        other => panic!("expected Text, got {:?}", other),
    }
    assert!(result.active_reasoning_parts.is_empty());
}

#[tokio::test]
async fn processor_data_and_source_chunks() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::Data { name: "ctx".into(), data: serde_json::json!(42) }),
        Ok(UIMessageChunk::SourceUrl { id: "s1".into(), url: "https://example.com".into(), title: Some("Example".into()) }),
        Ok(UIMessageChunk::SourceDocument { id: "s2".into(), title: "Doc".into(), source_type: Some("pdf".into()) }),
        Ok(UIMessageChunk::File { id: "f1".into(), media_type: "image/png".into(), data: "base64data".into() }),
        Ok(UIMessageChunk::Custom { id: "c1".into(), data: serde_json::json!({"key": "val"}) }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    assert_eq!(result.message.parts.len(), 5);

    assert!(matches!(&result.message.parts[0], UIMessagePart::Data { name, data } if name == "ctx" && *data == serde_json::json!(42)));
    assert!(matches!(&result.message.parts[1], UIMessagePart::SourceUrl { url, title } if url == "https://example.com" && *title == Some("Example".into())));
    assert!(matches!(&result.message.parts[2], UIMessagePart::SourceDocument { title, source_type, .. } if title == "Doc" && *source_type == Some("pdf".into())));
    assert!(matches!(&result.message.parts[3], UIMessagePart::File { media_type, data } if media_type == "image/png" && data == "base64data"));
    assert!(matches!(&result.message.parts[4], UIMessagePart::Custom { id, data } if id == "c1" && *data == serde_json::json!({"key": "val"})));
}

#[tokio::test]
async fn processor_message_metadata_update() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: Some("m1".into()), message_metadata: Some(serde_json::json!({"a": 1})) }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::MessageMetadata { metadata: serde_json::json!({"b": 2}) }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    // MessageMetadata chunk should override the initial Start metadata
    assert_eq!(result.message.metadata, Some(serde_json::json!({"b": 2})));
}

#[tokio::test]
async fn processor_start_step_adds_step_start_part() {
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::StartStep),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Ok(UIMessageChunk::TextDelta { id: "t1".into(), delta: "hello".into() }),
        Ok(UIMessageChunk::TextEnd { id: "t1".into() }),
        Ok(UIMessageChunk::Finish { finish_reason: FinishReason::Stop, usage: None }),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await.unwrap();
    assert_eq!(result.message.parts.len(), 2);
    assert!(matches!(&result.message.parts[0], UIMessagePart::StepStart));
    assert!(matches!(&result.message.parts[1], UIMessagePart::Text { .. }));
}

#[tokio::test]
async fn processor_stream_transport_error() {
    // Test that a transport-level error (Result::Err in stream) propagates
    let chunks: Vec<Result<UIMessageChunk, StreamError>> = vec![
        Ok(UIMessageChunk::Start { message_id: None, message_metadata: None }),
        Ok(UIMessageChunk::TextStart { id: "t1".into() }),
        Err(StreamError::Interrupted),
    ];
    let s = stream::iter(chunks);
    let (tx, _rx) = mpsc::unbounded_channel();

    let result = process_ui_message_stream(s, make_initial_message(), tx).await;
    assert!(result.is_err());
    assert!(matches!(result.unwrap_err(), StreamError::Interrupted));
}

// ---------------------------------------------------------------------------
// SSE parser tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_parse_sse_basic() {
    let raw = "data: {\"type\":\"text-delta\",\"id\":\"t1\",\"delta\":\"hi\"}\n\ndata: [DONE]\n\n";
    let byte_stream = stream::once(async move { Ok(Bytes::from(raw)) });

    let chunks: Vec<Result<UIMessageChunk, StreamError>> =
        parse_sse_stream(byte_stream).collect().await;

    assert_eq!(chunks.len(), 1);
    let chunk = chunks[0].as_ref().unwrap();
    match chunk {
        UIMessageChunk::TextDelta { id, delta } => {
            assert_eq!(id, "t1");
            assert_eq!(delta, "hi");
        }
        other => panic!("expected TextDelta, got {:?}", other),
    }
}

#[tokio::test]
async fn test_parse_sse_multiple_events() {
    let raw = concat!(
        "data: {\"type\":\"text-start\",\"id\":\"t1\"}\n\n",
        "data: {\"type\":\"text-delta\",\"id\":\"t1\",\"delta\":\"hello\"}\n\n",
        "data: {\"type\":\"text-end\",\"id\":\"t1\"}\n\n",
        "data: [DONE]\n\n",
    );
    let byte_stream = stream::once(async move { Ok(Bytes::from(raw)) });

    let chunks: Vec<Result<UIMessageChunk, StreamError>> =
        parse_sse_stream(byte_stream).collect().await;

    assert_eq!(chunks.len(), 3);
    assert!(matches!(chunks[0].as_ref().unwrap(), UIMessageChunk::TextStart { id } if id == "t1"));
    assert!(matches!(chunks[1].as_ref().unwrap(), UIMessageChunk::TextDelta { id, delta } if id == "t1" && delta == "hello"));
    assert!(matches!(chunks[2].as_ref().unwrap(), UIMessageChunk::TextEnd { id } if id == "t1"));
}

#[tokio::test]
async fn test_chunk_to_sse_round_trip() {
    let original = UIMessageChunk::TextDelta {
        id: "t1".into(),
        delta: "round trip".into(),
    };

    let sse_text = chunk_to_sse(&original);

    // Parse it back through SSE parser
    let full_sse = format!("{}{}", sse_text, sse_done());
    let byte_stream = stream::once(async move { Ok(Bytes::from(full_sse)) });

    let chunks: Vec<Result<UIMessageChunk, StreamError>> =
        parse_sse_stream(byte_stream).collect().await;

    assert_eq!(chunks.len(), 1);
    let chunk = chunks[0].as_ref().unwrap();
    match chunk {
        UIMessageChunk::TextDelta { id, delta } => {
            assert_eq!(id, "t1");
            assert_eq!(delta, "round trip");
        }
        other => panic!("expected TextDelta, got {:?}", other),
    }
}

#[tokio::test]
async fn test_parse_sse_split_across_boundaries() {
    // Simulate data arriving in two separate byte chunks, splitting an event in the middle
    let part1 = "data: {\"type\":\"text-del";
    let part2 = "ta\",\"id\":\"t1\",\"delta\":\"split\"}\n\ndata: [DONE]\n\n";

    let byte_stream = stream::iter(vec![
        Ok(Bytes::from(part1)),
        Ok(Bytes::from(part2)),
    ]);

    let chunks: Vec<Result<UIMessageChunk, StreamError>> =
        parse_sse_stream(byte_stream).collect().await;

    assert_eq!(chunks.len(), 1);
    match chunks[0].as_ref().unwrap() {
        UIMessageChunk::TextDelta { id, delta } => {
            assert_eq!(id, "t1");
            assert_eq!(delta, "split");
        }
        other => panic!("expected TextDelta, got {:?}", other),
    }
}

#[tokio::test]
async fn test_parse_sse_ignores_comments() {
    let raw = ": this is a comment\n\ndata: {\"type\":\"text-delta\",\"id\":\"t1\",\"delta\":\"ok\"}\n\ndata: [DONE]\n\n";
    let byte_stream = stream::once(async move { Ok(Bytes::from(raw)) });

    let chunks: Vec<Result<UIMessageChunk, StreamError>> =
        parse_sse_stream(byte_stream).collect().await;

    assert_eq!(chunks.len(), 1);
    match chunks[0].as_ref().unwrap() {
        UIMessageChunk::TextDelta { id, delta } => {
            assert_eq!(id, "t1");
            assert_eq!(delta, "ok");
        }
        other => panic!("expected TextDelta, got {:?}", other),
    }
}

#[tokio::test]
async fn test_sse_done_format() {
    assert_eq!(sse_done(), "data: [DONE]\n\n");
}

// ---------------------------------------------------------------------------
// LLM stream -> UI chunks conversion tests
// ---------------------------------------------------------------------------

#[tokio::test]
async fn test_llm_text_stream_to_ui_chunks() {
    let llm_stream = stream::iter(vec![
        StreamChunk::TextDelta("Hello".into()),
        StreamChunk::TextDelta(" world".into()),
        StreamChunk::Done(LLMResponse {
            content: vec![LLMContent::Text { text: "Hello world".into() }],
            stop_reason: StopReason::EndTurn,
            usage: LLMTokenUsage {
                input_tokens: 10,
                output_tokens: 5,
            },
        }),
    ]);

    let chunks: Vec<UIMessageChunk> = llm_stream_to_ui_chunks(llm_stream).collect().await;

    // Should produce: Start, TextStart, TextDelta("Hello"), TextDelta(" world"), TextEnd, Finish
    assert!(chunks.len() >= 5, "expected at least 5 chunks, got {}: {:?}", chunks.len(), chunks);

    assert!(matches!(&chunks[0], UIMessageChunk::Start { .. }));
    assert!(matches!(&chunks[1], UIMessageChunk::TextStart { .. }));
    assert!(matches!(&chunks[2], UIMessageChunk::TextDelta { delta, .. } if delta == "Hello"));
    assert!(matches!(&chunks[3], UIMessageChunk::TextDelta { delta, .. } if delta == " world"));
    assert!(matches!(&chunks[4], UIMessageChunk::TextEnd { .. }));
    assert!(matches!(&chunks[chunks.len() - 1], UIMessageChunk::Finish { finish_reason: FinishReason::Stop, .. }));
}

#[tokio::test]
async fn test_llm_thinking_stream_to_ui_chunks() {
    let llm_stream = stream::iter(vec![
        StreamChunk::ThinkingDelta("hmm...".into()),
        StreamChunk::TextDelta("The answer is 42.".into()),
        StreamChunk::Done(LLMResponse {
            content: vec![LLMContent::Text { text: "The answer is 42.".into() }],
            stop_reason: StopReason::EndTurn,
            usage: LLMTokenUsage {
                input_tokens: 20,
                output_tokens: 10,
            },
        }),
    ]);

    let chunks: Vec<UIMessageChunk> = llm_stream_to_ui_chunks(llm_stream).collect().await;

    // Should have: Start, ReasoningStart, ReasoningDelta, TextStart, TextDelta, ReasoningEnd, TextEnd, Finish
    assert!(matches!(&chunks[0], UIMessageChunk::Start { .. }));
    assert!(matches!(&chunks[1], UIMessageChunk::ReasoningStart { .. }));
    assert!(matches!(&chunks[2], UIMessageChunk::ReasoningDelta { delta, .. } if delta == "hmm..."));
    assert!(matches!(&chunks[3], UIMessageChunk::TextStart { .. }));
    assert!(matches!(&chunks[4], UIMessageChunk::TextDelta { delta, .. } if delta == "The answer is 42."));

    // On Done: text and reasoning should be closed
    let has_reasoning_end = chunks.iter().any(|c| matches!(c, UIMessageChunk::ReasoningEnd { .. }));
    let has_text_end = chunks.iter().any(|c| matches!(c, UIMessageChunk::TextEnd { .. }));
    assert!(has_reasoning_end, "expected ReasoningEnd chunk");
    assert!(has_text_end, "expected TextEnd chunk");

    assert!(matches!(&chunks[chunks.len() - 1], UIMessageChunk::Finish { finish_reason: FinishReason::Stop, .. }));
}

#[tokio::test]
async fn test_llm_tool_call_stream_to_ui_chunks() {
    let llm_stream = stream::iter(vec![
        StreamChunk::TextDelta("Let me check.".into()),
        StreamChunk::ToolCallDelta {
            id: "tc1".into(),
            name: "read_file".into(),
            input_delta: r#"{"path":"/tmp"}"#.into(),
        },
        StreamChunk::Done(LLMResponse {
            content: vec![
                LLMContent::Text { text: "Let me check.".into() },
                LLMContent::ToolUse {
                    id: "tc1".into(),
                    name: "read_file".into(),
                    input: serde_json::json!({"path": "/tmp"}),
                },
            ],
            stop_reason: StopReason::ToolUse,
            usage: LLMTokenUsage {
                input_tokens: 30,
                output_tokens: 15,
            },
        }),
    ]);

    let chunks: Vec<UIMessageChunk> = llm_stream_to_ui_chunks(llm_stream).collect().await;

    // Should have Start, TextStart, TextDelta, TextEnd, ToolInputStart, ToolInputAvailable, Finish
    assert!(matches!(&chunks[0], UIMessageChunk::Start { .. }));

    let has_tool_input_start = chunks.iter().any(|c| matches!(c, UIMessageChunk::ToolInputStart { tool_name, .. } if tool_name == "read_file"));
    let has_tool_input_available = chunks.iter().any(|c| matches!(c, UIMessageChunk::ToolInputAvailable { input, .. } if input["path"] == "/tmp"));
    assert!(has_tool_input_start, "expected ToolInputStart");
    assert!(has_tool_input_available, "expected ToolInputAvailable");

    assert!(matches!(&chunks[chunks.len() - 1], UIMessageChunk::Finish { finish_reason: FinishReason::ToolCalls, .. }));
}

#[tokio::test]
async fn test_llm_max_tokens_finish_reason() {
    let llm_stream = stream::iter(vec![
        StreamChunk::TextDelta("cut off".into()),
        StreamChunk::Done(LLMResponse {
            content: vec![LLMContent::Text { text: "cut off".into() }],
            stop_reason: StopReason::MaxTokens,
            usage: LLMTokenUsage {
                input_tokens: 100,
                output_tokens: 4096,
            },
        }),
    ]);

    let chunks: Vec<UIMessageChunk> = llm_stream_to_ui_chunks(llm_stream).collect().await;

    assert!(matches!(&chunks[chunks.len() - 1], UIMessageChunk::Finish { finish_reason: FinishReason::MaxTokens, .. }));
}
