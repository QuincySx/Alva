use serde_json::json;
use srow_core::{UIMessage, UIMessagePart, UIMessageRole, TextPartState, ToolState};

#[test]
fn test_text_part_serialize_deserialize() {
    let msg = UIMessage {
        id: "msg-1".to_string(),
        role: UIMessageRole::Assistant,
        parts: vec![UIMessagePart::Text {
            text: "hello".to_string(),
            state: None,
        }],
        metadata: None,
    };

    let serialized = serde_json::to_value(&msg).unwrap();

    // Verify the part has the correct tag and fields
    let part = &serialized["parts"][0];
    assert_eq!(part["type"], "text");
    assert_eq!(part["text"], "hello");
    // state is None so it should be absent
    assert!(part.get("state").is_none());

    // Round-trip
    let deserialized: UIMessage = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized.id, "msg-1");
    assert_eq!(deserialized.role, UIMessageRole::Assistant);
    assert_eq!(deserialized.parts.len(), 1);
}

#[test]
fn test_tool_part_all_states_round_trip() {
    let all_states = vec![
        ToolState::InputStreaming,
        ToolState::InputAvailable,
        ToolState::ApprovalRequested,
        ToolState::ApprovalResponded,
        ToolState::OutputAvailable,
        ToolState::OutputError,
        ToolState::OutputDenied,
    ];

    for state in all_states {
        let msg = UIMessage {
            id: "msg-tool".to_string(),
            role: UIMessageRole::Assistant,
            parts: vec![UIMessagePart::Tool {
                id: "call-1".to_string(),
                tool_name: "read_file".to_string(),
                input: json!({"path": "/tmp/test.txt"}),
                state: state.clone(),
                output: Some(json!({"content": "file data"})),
                error: None,
                title: Some("Read file".to_string()),
            }],
            metadata: None,
        };

        let json_str = serde_json::to_string(&msg).unwrap();
        let round_tripped: UIMessage = serde_json::from_str(&json_str).unwrap();

        if let UIMessagePart::Tool { state: rt_state, .. } = &round_tripped.parts[0] {
            assert_eq!(*rt_state, state);
        } else {
            panic!("Expected Tool part after round-trip");
        }
    }
}

#[test]
fn test_mixed_parts_round_trip() {
    let msg = UIMessage {
        id: "msg-mixed".to_string(),
        role: UIMessageRole::Assistant,
        parts: vec![
            UIMessagePart::Text {
                text: "Let me think...".to_string(),
                state: Some(TextPartState::Done),
            },
            UIMessagePart::Reasoning {
                text: "The user wants X".to_string(),
                state: Some(TextPartState::Streaming),
            },
            UIMessagePart::Tool {
                id: "call-2".to_string(),
                tool_name: "bash".to_string(),
                input: json!({"command": "ls"}),
                state: ToolState::OutputAvailable,
                output: Some(json!("file1\nfile2")),
                error: None,
                title: None,
            },
            UIMessagePart::File {
                media_type: "image/png".to_string(),
                data: "iVBORw0KGgo=".to_string(),
            },
            UIMessagePart::StepStart,
        ],
        metadata: Some(json!({"model": "claude-opus-4-20250514"})),
    };

    let serialized = serde_json::to_value(&msg).unwrap();
    let parts = serialized["parts"].as_array().unwrap();
    assert_eq!(parts.len(), 5);
    assert_eq!(parts[0]["type"], "text");
    assert_eq!(parts[1]["type"], "reasoning");
    assert_eq!(parts[2]["type"], "tool");
    assert_eq!(parts[3]["type"], "file");
    assert_eq!(parts[4]["type"], "step-start");

    // Round-trip
    let deserialized: UIMessage = serde_json::from_value(serialized).unwrap();
    assert_eq!(deserialized.parts.len(), 5);
    assert_eq!(deserialized.metadata, Some(json!({"model": "claude-opus-4-20250514"})));
}

#[test]
fn test_text_part_state_serialization() {
    let streaming = serde_json::to_value(&TextPartState::Streaming).unwrap();
    assert_eq!(streaming, json!("streaming"));

    let done = serde_json::to_value(&TextPartState::Done).unwrap();
    assert_eq!(done, json!("done"));

    // Deserialize back
    let rt: TextPartState = serde_json::from_value(json!("streaming")).unwrap();
    assert_eq!(rt, TextPartState::Streaming);

    let rt: TextPartState = serde_json::from_value(json!("done")).unwrap();
    assert_eq!(rt, TextPartState::Done);
}

#[test]
fn test_tool_state_kebab_case_serialization() {
    let cases = vec![
        (ToolState::InputStreaming, "input-streaming"),
        (ToolState::InputAvailable, "input-available"),
        (ToolState::ApprovalRequested, "approval-requested"),
        (ToolState::ApprovalResponded, "approval-responded"),
        (ToolState::OutputAvailable, "output-available"),
        (ToolState::OutputError, "output-error"),
        (ToolState::OutputDenied, "output-denied"),
    ];

    for (variant, expected_str) in cases {
        let serialized = serde_json::to_value(&variant).unwrap();
        assert_eq!(serialized, json!(expected_str), "Serialization mismatch for {:?}", variant);

        let deserialized: ToolState = serde_json::from_value(json!(expected_str)).unwrap();
        assert_eq!(deserialized, variant, "Deserialization mismatch for {}", expected_str);
    }
}

#[test]
fn test_role_serialization() {
    assert_eq!(serde_json::to_value(&UIMessageRole::System).unwrap(), json!("system"));
    assert_eq!(serde_json::to_value(&UIMessageRole::User).unwrap(), json!("user"));
    assert_eq!(serde_json::to_value(&UIMessageRole::Assistant).unwrap(), json!("assistant"));
}

#[test]
fn test_optional_fields_omitted_when_none() {
    let msg = UIMessage {
        id: "msg-sparse".to_string(),
        role: UIMessageRole::User,
        parts: vec![
            UIMessagePart::Text {
                text: "hi".to_string(),
                state: None,
            },
            UIMessagePart::SourceUrl {
                url: "https://example.com".to_string(),
                title: None,
            },
            UIMessagePart::Tool {
                id: "c1".to_string(),
                tool_name: "test".to_string(),
                input: json!({}),
                state: ToolState::InputAvailable,
                output: None,
                error: None,
                title: None,
            },
        ],
        metadata: None,
    };

    let val = serde_json::to_value(&msg).unwrap();

    // metadata should be absent
    assert!(val.get("metadata").is_none());

    // Text part: state absent
    assert!(val["parts"][0].get("state").is_none());

    // SourceUrl: title absent
    assert!(val["parts"][1].get("title").is_none());

    // Tool part: output, error, title absent
    let tool = &val["parts"][2];
    assert!(tool.get("output").is_none());
    assert!(tool.get("error").is_none());
    assert!(tool.get("title").is_none());
}
