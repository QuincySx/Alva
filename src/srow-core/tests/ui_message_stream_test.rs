use srow_core::ui_message_stream::{UIMessageChunk, FinishReason, ChatStatus, TokenUsage};

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
