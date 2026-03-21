// Provider V4 type system tests — serde round-trips, construction, Display formatting.

use srow_core::ports::provider::*;

// ---------------------------------------------------------------------------
// ProviderWarning serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn provider_warning_unsupported_serde_roundtrip() {
    let warning = ProviderWarning::Unsupported {
        feature: "json_mode".to_string(),
        details: Some("Not supported by this model".to_string()),
    };
    let json = serde_json::to_string(&warning).unwrap();
    let deserialized: ProviderWarning = serde_json::from_str(&json).unwrap();
    match deserialized {
        ProviderWarning::Unsupported { feature, details } => {
            assert_eq!(feature, "json_mode");
            assert_eq!(details, Some("Not supported by this model".to_string()));
        }
        _ => panic!("Expected Unsupported variant"),
    }
}

#[test]
fn provider_warning_compatibility_serde_roundtrip() {
    let warning = ProviderWarning::Compatibility {
        feature: "tool_choice".to_string(),
        details: None,
    };
    let json = serde_json::to_string(&warning).unwrap();
    assert!(!json.contains("details")); // skip_serializing_if = None
    let deserialized: ProviderWarning = serde_json::from_str(&json).unwrap();
    match deserialized {
        ProviderWarning::Compatibility { feature, details } => {
            assert_eq!(feature, "tool_choice");
            assert!(details.is_none());
        }
        _ => panic!("Expected Compatibility variant"),
    }
}

#[test]
fn provider_warning_other_serde_roundtrip() {
    let warning = ProviderWarning::Other {
        message: "Something happened".to_string(),
    };
    let json = serde_json::to_string(&warning).unwrap();
    let deserialized: ProviderWarning = serde_json::from_str(&json).unwrap();
    match deserialized {
        ProviderWarning::Other { message } => {
            assert_eq!(message, "Something happened");
        }
        _ => panic!("Expected Other variant"),
    }
}

// ---------------------------------------------------------------------------
// ToolChoice serde round-trip (all variants)
// ---------------------------------------------------------------------------

#[test]
fn tool_choice_auto_serde_roundtrip() {
    let choice = ToolChoice::Auto;
    let json = serde_json::to_string(&choice).unwrap();
    let deserialized: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ToolChoice::Auto);
}

#[test]
fn tool_choice_none_serde_roundtrip() {
    let choice = ToolChoice::None;
    let json = serde_json::to_string(&choice).unwrap();
    let deserialized: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ToolChoice::None);
}

#[test]
fn tool_choice_required_serde_roundtrip() {
    let choice = ToolChoice::Required;
    let json = serde_json::to_string(&choice).unwrap();
    let deserialized: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized, ToolChoice::Required);
}

#[test]
fn tool_choice_tool_serde_roundtrip() {
    let choice = ToolChoice::Tool {
        tool_name: "my_tool".to_string(),
    };
    let json = serde_json::to_string(&choice).unwrap();
    let deserialized: ToolChoice = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized,
        ToolChoice::Tool {
            tool_name: "my_tool".to_string()
        }
    );
}

// ---------------------------------------------------------------------------
// LanguageModelMessage construction (all 4 roles)
// ---------------------------------------------------------------------------

#[test]
fn language_model_message_system() {
    let msg = LanguageModelMessage::System {
        content: "You are a helpful assistant.".to_string(),
        provider_options: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"system\""));
    assert!(json.contains("You are a helpful assistant."));
}

#[test]
fn language_model_message_user() {
    let msg = LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Hello".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"user\""));
    assert!(json.contains("Hello"));
}

#[test]
fn language_model_message_assistant() {
    let msg = LanguageModelMessage::Assistant {
        content: vec![AssistantContentPart::Text {
            text: "Hi there!".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"assistant\""));
    assert!(json.contains("Hi there!"));
}

#[test]
fn language_model_message_tool() {
    let msg = LanguageModelMessage::Tool {
        content: vec![ToolContentPart::ToolResult {
            tool_call_id: "call_123".to_string(),
            tool_name: "calculator".to_string(),
            output: ToolResultOutput::Text {
                value: "42".to_string(),
            },
            provider_options: None,
        }],
        provider_options: None,
    };
    let json = serde_json::to_string(&msg).unwrap();
    assert!(json.contains("\"role\":\"tool\""));
    assert!(json.contains("call_123"));
    assert!(json.contains("calculator"));
}

// ---------------------------------------------------------------------------
// LanguageModelContent construction
// ---------------------------------------------------------------------------

#[test]
fn language_model_content_text() {
    let content = LanguageModelContent::Text {
        text: "Hello world".to_string(),
        provider_metadata: None,
    };
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("\"type\":\"text\""));
    assert!(json.contains("Hello world"));
}

#[test]
fn language_model_content_tool_call() {
    let content = LanguageModelContent::ToolCall {
        tool_call_id: "tc_1".to_string(),
        tool_name: "search".to_string(),
        input: r#"{"query":"test"}"#.to_string(),
        provider_metadata: None,
    };
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("\"type\":\"tool-call\""));
    assert!(json.contains("tc_1"));
    assert!(json.contains("search"));
}

#[test]
fn language_model_content_reasoning() {
    let content = LanguageModelContent::Reasoning {
        text: "Let me think about this...".to_string(),
        provider_metadata: None,
    };
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("\"type\":\"reasoning\""));
}

#[test]
fn language_model_content_source_url() {
    let content = LanguageModelContent::Source(LanguageModelSource::Url {
        id: "src_1".to_string(),
        url: "https://example.com".to_string(),
        title: Some("Example".to_string()),
        provider_metadata: None,
    });
    let json = serde_json::to_string(&content).unwrap();
    assert!(json.contains("https://example.com"));
}

// ---------------------------------------------------------------------------
// UnifiedFinishReason serde
// ---------------------------------------------------------------------------

#[test]
fn unified_finish_reason_serde_roundtrip() {
    let reasons = vec![
        UnifiedFinishReason::Stop,
        UnifiedFinishReason::Length,
        UnifiedFinishReason::ContentFilter,
        UnifiedFinishReason::ToolCalls,
        UnifiedFinishReason::Error,
        UnifiedFinishReason::Other,
    ];
    let expected_json = vec![
        "\"stop\"",
        "\"length\"",
        "\"content-filter\"",
        "\"tool-calls\"",
        "\"error\"",
        "\"other\"",
    ];
    for (reason, expected) in reasons.iter().zip(expected_json.iter()) {
        let json = serde_json::to_string(reason).unwrap();
        assert_eq!(json, *expected, "Failed for {:?}", reason);
        let deserialized: UnifiedFinishReason = serde_json::from_str(&json).unwrap();
        assert_eq!(&deserialized, reason);
    }
}

// ---------------------------------------------------------------------------
// ProviderError Display formatting
// ---------------------------------------------------------------------------

#[test]
fn provider_error_display_api_call() {
    let err = ProviderError::ApiCall {
        message: "timeout".to_string(),
        url: "https://api.example.com".to_string(),
        status_code: Some(504),
        response_body: None,
        is_retryable: true,
    };
    assert_eq!(err.to_string(), "API call error: timeout");
}

#[test]
fn provider_error_display_empty_response() {
    let err = ProviderError::EmptyResponseBody;
    assert_eq!(err.to_string(), "Empty response body");
}

#[test]
fn provider_error_display_invalid_argument() {
    let err = ProviderError::InvalidArgument {
        argument: "temperature".to_string(),
        message: "must be between 0 and 2".to_string(),
    };
    assert_eq!(
        err.to_string(),
        "Invalid argument 'temperature': must be between 0 and 2"
    );
}

#[test]
fn provider_error_display_no_such_model() {
    let err = ProviderError::NoSuchModel {
        model_id: "gpt-5".to_string(),
        model_type: "language model".to_string(),
    };
    assert_eq!(err.to_string(), "No such language model: gpt-5");
}

#[test]
fn provider_error_display_unsupported() {
    let err = ProviderError::UnsupportedFunctionality("image generation".to_string());
    assert_eq!(err.to_string(), "Unsupported: image generation");
}

#[test]
fn provider_error_display_rate_limited() {
    let err = ProviderError::RateLimited {
        retry_after_ms: Some(5000),
    };
    assert_eq!(err.to_string(), "Rate limited");
}

#[test]
fn provider_error_display_json_parse() {
    let err = ProviderError::JsonParse {
        message: "unexpected token".to_string(),
        text: "{bad json".to_string(),
    };
    assert_eq!(err.to_string(), "JSON parse error: unexpected token");
}

#[test]
fn provider_error_display_too_many_embeddings() {
    let err = ProviderError::TooManyEmbeddingValues {
        count: 3000,
        max: 2048,
    };
    assert_eq!(err.to_string(), "Too many embedding values: 3000 > 2048");
}

// ---------------------------------------------------------------------------
// DataContent serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn data_content_base64_serde_roundtrip() {
    let data = DataContent::Base64 {
        data: "SGVsbG8=".to_string(),
    };
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("\"type\":\"base64\""));
    let deserialized: DataContent = serde_json::from_str(&json).unwrap();
    match deserialized {
        DataContent::Base64 { data } => assert_eq!(data, "SGVsbG8="),
        _ => panic!("Expected Base64 variant"),
    }
}

#[test]
fn data_content_url_serde_roundtrip() {
    let data = DataContent::Url {
        url: "https://example.com/file.pdf".to_string(),
    };
    let json = serde_json::to_string(&data).unwrap();
    assert!(json.contains("\"type\":\"url\""));
    let deserialized: DataContent = serde_json::from_str(&json).unwrap();
    match deserialized {
        DataContent::Url { url } => assert_eq!(url, "https://example.com/file.pdf"),
        _ => panic!("Expected Url variant"),
    }
}

// ---------------------------------------------------------------------------
// ToolResultOutput serde round-trip
// ---------------------------------------------------------------------------

#[test]
fn tool_result_output_text_serde_roundtrip() {
    let output = ToolResultOutput::Text {
        value: "result text".to_string(),
    };
    let json = serde_json::to_string(&output).unwrap();
    assert!(json.contains("\"type\":\"text\""));
    let deserialized: ToolResultOutput = serde_json::from_str(&json).unwrap();
    match deserialized {
        ToolResultOutput::Text { value } => assert_eq!(value, "result text"),
        _ => panic!("Expected Text variant"),
    }
}

#[test]
fn tool_result_output_json_serde_roundtrip() {
    let output = ToolResultOutput::Json {
        value: serde_json::json!({"answer": 42}),
    };
    let json = serde_json::to_string(&output).unwrap();
    let deserialized: ToolResultOutput = serde_json::from_str(&json).unwrap();
    match deserialized {
        ToolResultOutput::Json { value } => {
            assert_eq!(value["answer"], 42);
        }
        _ => panic!("Expected Json variant"),
    }
}

// ---------------------------------------------------------------------------
// Default impls
// ---------------------------------------------------------------------------

#[test]
fn language_model_usage_default() {
    let usage = LanguageModelUsage::default();
    assert!(usage.input_tokens.total.is_none());
    assert!(usage.output_tokens.total.is_none());
    assert!(usage.raw.is_none());
}
