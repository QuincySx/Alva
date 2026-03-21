// Integration tests for the OpenAI Chat Completions API provider.

use srow_core::adapters::llm::openai::OpenAILanguageModel;
use srow_core::ports::provider::content::LanguageModelContent;
use srow_core::ports::provider::language_model::{
    LanguageModelCallOptions, LanguageModelStreamPart, ResponseFormat, UnifiedFinishReason,
};
use srow_core::ports::provider::prompt::{
    DataContent, LanguageModelMessage, UserContentPart,
};
use srow_core::ports::provider::tool_types::{
    FunctionTool, LanguageModelTool, ToolChoice,
};

use futures::StreamExt;

// ---------------------------------------------------------------------------
// Helper: default call options with empty prompt
// ---------------------------------------------------------------------------

fn default_options() -> LanguageModelCallOptions {
    LanguageModelCallOptions {
        prompt: Vec::new(),
        max_output_tokens: Some(1024),
        temperature: None,
        stop_sequences: None,
        top_p: None,
        top_k: None,
        presence_penalty: None,
        frequency_penalty: None,
        response_format: None,
        seed: None,
        tools: None,
        tool_choice: None,
        reasoning: None,
        provider_options: None,
        headers: None,
    }
}

fn provider() -> OpenAILanguageModel {
    OpenAILanguageModel::new("test-key", "gpt-4o")
}

// ---------------------------------------------------------------------------
// Helper: produce SSE stream parts from a raw SSE text string
// ---------------------------------------------------------------------------

async fn sse_text_to_parts(
    provider: &OpenAILanguageModel,
    sse_text: &str,
) -> Vec<LanguageModelStreamPart> {
    use bytes::Bytes;
    use srow_core::adapters::llm::http::parse_raw_sse;

    let byte_stream = futures::stream::iter(vec![Ok(Bytes::from(sse_text.to_string()))]);
    let sse_stream = parse_raw_sse(byte_stream);
    let part_stream = provider.sse_to_stream_parts(sse_stream);
    part_stream.collect::<Vec<_>>().await
}

// ===========================================================================
// Test 1: Basic request body structure
// ===========================================================================

#[test]
fn test_request_body_basic() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![
        LanguageModelMessage::System {
            content: "You are helpful.".to_string(),
            provider_options: None,
        },
        LanguageModelMessage::User {
            content: vec![UserContentPart::Text {
                text: "Hi".to_string(),
                provider_options: None,
            }],
            provider_options: None,
        },
    ];
    opts.temperature = Some(0.7);
    opts.top_p = Some(0.9);

    let body = p.build_request_body(&opts, false).unwrap();

    assert_eq!(body["model"], "gpt-4o");
    assert_eq!(body["max_tokens"], 1024);

    // Float comparison with tolerance
    let temp = body["temperature"].as_f64().unwrap();
    assert!((temp - 0.7).abs() < 0.001, "temperature: {}", temp);
    let top_p_val = body["top_p"].as_f64().unwrap();
    assert!((top_p_val - 0.9).abs() < 0.001, "top_p: {}", top_p_val);

    // System message should be a regular message
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0]["role"], "system");
    assert_eq!(messages[0]["content"], "You are helpful.");
    assert_eq!(messages[1]["role"], "user");
    // Single text part should be simplified to a string
    assert_eq!(messages[1]["content"], "Hi");

    // No stream flag
    assert!(body.get("stream").is_none());
    assert!(body.get("stream_options").is_none());
}

// ===========================================================================
// Test 2: Request body with tools — uses `parameters` (not input_schema)
// ===========================================================================

#[test]
fn test_request_body_with_tools() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Search for cats".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];
    opts.tools = Some(vec![LanguageModelTool::Function(FunctionTool {
        name: "web_search".to_string(),
        description: Some("Search the web".to_string()),
        input_schema: serde_json::json!({
            "type": "object",
            "properties": {
                "query": {"type": "string"}
            },
            "required": ["query"]
        }),
        strict: None,
        provider_options: None,
    })]);
    opts.tool_choice = Some(ToolChoice::Auto);

    let body = p.build_request_body(&opts, false).unwrap();

    // Tools should use parameters, NOT input_schema
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["type"], "function");
    assert_eq!(tools[0]["function"]["name"], "web_search");
    assert_eq!(tools[0]["function"]["description"], "Search the web");
    assert!(tools[0]["function"].get("parameters").is_some());
    assert!(tools[0]["function"].get("input_schema").is_none());

    // Tool choice should be a string "auto"
    assert_eq!(body["tool_choice"], "auto");

    // Test Required → "required"
    let mut opts2 = opts;
    opts2.tool_choice = Some(ToolChoice::Required);
    let body2 = p.build_request_body(&opts2, false).unwrap();
    assert_eq!(body2["tool_choice"], "required");
}

// ===========================================================================
// Test 3: Request body with image content
// ===========================================================================

#[test]
fn test_request_body_with_image() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![
            UserContentPart::Text {
                text: "What is in this image?".to_string(),
                provider_options: None,
            },
            UserContentPart::File {
                data: DataContent::Base64 {
                    data: "iVBORw0KGgo=".to_string(),
                },
                media_type: "image/png".to_string(),
                filename: None,
                provider_options: None,
            },
            UserContentPart::File {
                data: DataContent::Url {
                    url: "https://example.com/cat.jpg".to_string(),
                },
                media_type: "image/jpeg".to_string(),
                filename: None,
                provider_options: None,
            },
        ],
        provider_options: None,
    }];

    let body = p.build_request_body(&opts, false).unwrap();
    let content = body["messages"][0]["content"].as_array().unwrap();

    assert_eq!(content.len(), 3);

    // Text part
    assert_eq!(content[0]["type"], "text");
    assert_eq!(content[0]["text"], "What is in this image?");

    // Base64 image → data URL
    assert_eq!(content[1]["type"], "image_url");
    assert_eq!(
        content[1]["image_url"]["url"],
        "data:image/png;base64,iVBORw0KGgo="
    );

    // URL image
    assert_eq!(content[2]["type"], "image_url");
    assert_eq!(
        content[2]["image_url"]["url"],
        "https://example.com/cat.jpg"
    );
}

// ===========================================================================
// Test 4: Request body with response format (json_schema)
// ===========================================================================

#[test]
fn test_request_body_response_format() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Give me JSON".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];

    // Test json_schema format
    opts.response_format = Some(ResponseFormat::Json {
        schema: Some(serde_json::json!({
            "type": "object",
            "properties": {
                "name": {"type": "string"}
            }
        })),
        name: Some("my_schema".to_string()),
        description: None,
    });

    let body = p.build_request_body(&opts, false).unwrap();

    let rf = &body["response_format"];
    assert_eq!(rf["type"], "json_schema");
    assert_eq!(rf["json_schema"]["name"], "my_schema");
    assert_eq!(rf["json_schema"]["strict"], true);
    assert!(rf["json_schema"]["schema"].get("properties").is_some());

    // Test json_object format (no schema)
    opts.response_format = Some(ResponseFormat::Json {
        schema: None,
        name: None,
        description: None,
    });

    let body2 = p.build_request_body(&opts, false).unwrap();
    assert_eq!(body2["response_format"]["type"], "json_object");

    // Test text format
    opts.response_format = Some(ResponseFormat::Text);
    let body3 = p.build_request_body(&opts, false).unwrap();
    assert_eq!(body3["response_format"]["type"], "text");
}

// ===========================================================================
// Test 5: Response parsing — text response
// ===========================================================================

#[test]
fn test_response_parsing_text() {
    let p = provider();
    let json = serde_json::json!({
        "id": "chatcmpl-abc123",
        "model": "gpt-4o",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "Hello, world!"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 10,
            "completion_tokens": 20,
            "total_tokens": 30
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        LanguageModelContent::Text { text, .. } => assert_eq!(text, "Hello, world!"),
        _ => panic!("Expected Text content"),
    }

    assert_eq!(result.finish_reason.unified, UnifiedFinishReason::Stop);
    assert_eq!(result.finish_reason.raw.as_deref(), Some("stop"));

    let resp = result.response.unwrap();
    assert_eq!(resp.id.as_deref(), Some("chatcmpl-abc123"));
    assert_eq!(resp.model_id.as_deref(), Some("gpt-4o"));
}

// ===========================================================================
// Test 6: Response parsing — tool calls
// ===========================================================================

#[test]
fn test_response_parsing_tool_calls() {
    let p = provider();
    let json = serde_json::json!({
        "id": "chatcmpl-xyz789",
        "model": "gpt-4o",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": null,
                "tool_calls": [{
                    "id": "call_abc123",
                    "type": "function",
                    "function": {
                        "name": "search",
                        "arguments": "{\"q\":\"test\"}"
                    }
                }]
            },
            "finish_reason": "tool_calls"
        }],
        "usage": {
            "prompt_tokens": 15,
            "completion_tokens": 25,
            "total_tokens": 40
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        LanguageModelContent::ToolCall {
            tool_call_id,
            tool_name,
            input,
            ..
        } => {
            assert_eq!(tool_call_id, "call_abc123");
            assert_eq!(tool_name, "search");
            // input is the raw arguments string
            let parsed: serde_json::Value = serde_json::from_str(input).unwrap();
            assert_eq!(parsed["q"], "test");
        }
        _ => panic!("Expected ToolCall content"),
    }

    assert_eq!(result.finish_reason.unified, UnifiedFinishReason::ToolCalls);
}

// ===========================================================================
// Test 7: SSE stream — text deltas
// ===========================================================================

#[tokio::test]
async fn test_sse_stream_text() {
    let p = provider();

    let sse_text = concat!(
        "data: {\"id\":\"chatcmpl-s1\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"content\":\"He\"}}]}\n\n",
        "data: {\"id\":\"chatcmpl-s1\",\"choices\":[{\"delta\":{\"content\":\"llo\"}}]}\n\n",
        "data: {\"id\":\"chatcmpl-s1\",\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
        "data: {\"id\":\"chatcmpl-s1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"stop\"}],\"usage\":{\"prompt_tokens\":10,\"completion_tokens\":20}}\n\n",
        "data: [DONE]\n\n",
    );

    let parts = sse_text_to_parts(&p, sse_text).await;

    let mut found_text_start = false;
    let mut found_text_delta_he = false;
    let mut found_text_delta_llo = false;
    let mut found_text_delta_world = false;
    let mut found_text_end = false;
    let mut found_finish = false;

    for part in &parts {
        match part {
            LanguageModelStreamPart::TextStart { id } => {
                assert_eq!(id, "text-0");
                found_text_start = true;
            }
            LanguageModelStreamPart::TextDelta { id, delta } => {
                assert_eq!(id, "text-0");
                if delta == "He" {
                    found_text_delta_he = true;
                }
                if delta == "llo" {
                    found_text_delta_llo = true;
                }
                if delta == " world" {
                    found_text_delta_world = true;
                }
            }
            LanguageModelStreamPart::TextEnd { id } => {
                assert_eq!(id, "text-0");
                found_text_end = true;
            }
            LanguageModelStreamPart::Finish {
                usage,
                finish_reason,
                ..
            } => {
                assert_eq!(finish_reason.unified, UnifiedFinishReason::Stop);
                assert_eq!(usage.input_tokens.total, Some(10));
                assert_eq!(usage.output_tokens.total, Some(20));
                found_finish = true;
            }
            _ => {}
        }
    }

    assert!(found_text_start, "Missing TextStart");
    assert!(found_text_delta_he, "Missing TextDelta 'He'");
    assert!(found_text_delta_llo, "Missing TextDelta 'llo'");
    assert!(found_text_delta_world, "Missing TextDelta ' world'");
    assert!(found_text_end, "Missing TextEnd");
    assert!(found_finish, "Missing Finish");
}

// ===========================================================================
// Test 8: SSE stream — tool call deltas
// ===========================================================================

#[tokio::test]
async fn test_sse_stream_tool_calls() {
    let p = provider();

    let sse_text = concat!(
        "data: {\"id\":\"chatcmpl-t1\",\"choices\":[{\"delta\":{\"role\":\"assistant\",\"tool_calls\":[{\"index\":0,\"id\":\"call_xxx\",\"type\":\"function\",\"function\":{\"name\":\"search\",\"arguments\":\"\"}}]}}]}\n\n",
        "data: {\"id\":\"chatcmpl-t1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\"{\\\"q\\\"\"}}]}}]}\n\n",
        "data: {\"id\":\"chatcmpl-t1\",\"choices\":[{\"delta\":{\"tool_calls\":[{\"index\":0,\"function\":{\"arguments\":\":\\\"test\\\"}\"}}]}}]}\n\n",
        "data: {\"id\":\"chatcmpl-t1\",\"choices\":[{\"delta\":{},\"finish_reason\":\"tool_calls\"}],\"usage\":{\"prompt_tokens\":12,\"completion_tokens\":18}}\n\n",
        "data: [DONE]\n\n",
    );

    let parts = sse_text_to_parts(&p, sse_text).await;

    let mut found_tool_start = false;
    let mut found_tool_delta = false;
    let mut found_tool_end = false;
    let mut found_finish = false;

    for part in &parts {
        match part {
            LanguageModelStreamPart::ToolInputStart {
                id, tool_name, ..
            } => {
                assert_eq!(id, "call_xxx");
                assert_eq!(tool_name, "search");
                found_tool_start = true;
            }
            LanguageModelStreamPart::ToolInputDelta { id, .. } => {
                assert_eq!(id, "call_xxx");
                found_tool_delta = true;
            }
            LanguageModelStreamPart::ToolInputEnd { id } => {
                assert_eq!(id, "call_xxx");
                found_tool_end = true;
            }
            LanguageModelStreamPart::Finish { finish_reason, .. } => {
                assert_eq!(finish_reason.unified, UnifiedFinishReason::ToolCalls);
                found_finish = true;
            }
            _ => {}
        }
    }

    assert!(found_tool_start, "Missing ToolInputStart");
    assert!(found_tool_delta, "Missing ToolInputDelta");
    assert!(found_tool_end, "Missing ToolInputEnd");
    assert!(found_finish, "Missing Finish");
}

// ===========================================================================
// Test 9: Usage parsing
// ===========================================================================

#[test]
fn test_usage_parsing() {
    let p = provider();
    let json = serde_json::json!({
        "id": "chatcmpl-usage",
        "model": "gpt-4o",
        "choices": [{
            "message": {
                "role": "assistant",
                "content": "Hi"
            },
            "finish_reason": "stop"
        }],
        "usage": {
            "prompt_tokens": 100,
            "completion_tokens": 50,
            "total_tokens": 150
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.usage.input_tokens.total, Some(100));
    assert_eq!(result.usage.output_tokens.total, Some(50));

    // Raw usage should be preserved
    assert!(result.usage.raw.is_some());
    let raw = result.usage.raw.unwrap();
    assert_eq!(raw["prompt_tokens"], 100);
    assert_eq!(raw["completion_tokens"], 50);
    assert_eq!(raw["total_tokens"], 150);
}

// ===========================================================================
// Test 10: Tool choice mapping — all 4 variants
// ===========================================================================

#[test]
fn test_tool_choice_mapping() {
    let p = provider();

    // Auto
    let auto = p.convert_tool_choice(&ToolChoice::Auto);
    assert_eq!(auto, serde_json::json!("auto"));

    // None
    let none = p.convert_tool_choice(&ToolChoice::None);
    assert_eq!(none, serde_json::json!("none"));

    // Required
    let required = p.convert_tool_choice(&ToolChoice::Required);
    assert_eq!(required, serde_json::json!("required"));

    // Specific tool
    let specific = p.convert_tool_choice(&ToolChoice::Tool {
        tool_name: "my_tool".to_string(),
    });
    assert_eq!(
        specific,
        serde_json::json!({"type": "function", "function": {"name": "my_tool"}})
    );
}

// ===========================================================================
// Test 11: Stream flag and stream_options
// ===========================================================================

#[test]
fn test_stream_flag() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Hi".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];

    let body_no_stream = p.build_request_body(&opts, false).unwrap();
    assert!(body_no_stream.get("stream").is_none());
    assert!(body_no_stream.get("stream_options").is_none());

    let body_stream = p.build_request_body(&opts, true).unwrap();
    assert_eq!(body_stream["stream"], true);
    assert_eq!(body_stream["stream_options"]["include_usage"], true);
}

// ===========================================================================
// Test 12: Finish reason mapping
// ===========================================================================

#[test]
fn test_finish_reason_mapping() {
    let p = provider();

    // stop → Stop
    let json_stop = serde_json::json!({
        "id": "c1", "model": "gpt-4o",
        "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "stop"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    assert_eq!(
        p.parse_response(&json_stop).unwrap().finish_reason.unified,
        UnifiedFinishReason::Stop
    );

    // length → Length
    let json_length = serde_json::json!({
        "id": "c2", "model": "gpt-4o",
        "choices": [{"message": {"role": "assistant", "content": "hi"}, "finish_reason": "length"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    assert_eq!(
        p.parse_response(&json_length).unwrap().finish_reason.unified,
        UnifiedFinishReason::Length
    );

    // content_filter → ContentFilter
    let json_cf = serde_json::json!({
        "id": "c3", "model": "gpt-4o",
        "choices": [{"message": {"role": "assistant", "content": ""}, "finish_reason": "content_filter"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    assert_eq!(
        p.parse_response(&json_cf).unwrap().finish_reason.unified,
        UnifiedFinishReason::ContentFilter
    );

    // tool_calls → ToolCalls
    let json_tc = serde_json::json!({
        "id": "c4", "model": "gpt-4o",
        "choices": [{"message": {"role": "assistant", "content": null, "tool_calls": [{"id": "c", "type": "function", "function": {"name": "f", "arguments": "{}"}}]}, "finish_reason": "tool_calls"}],
        "usage": {"prompt_tokens": 1, "completion_tokens": 1, "total_tokens": 2}
    });
    assert_eq!(
        p.parse_response(&json_tc).unwrap().finish_reason.unified,
        UnifiedFinishReason::ToolCalls
    );
}

// ===========================================================================
// Test 13: Optional parameters (seed, stop, frequency/presence penalty)
// ===========================================================================

#[test]
fn test_optional_parameters() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Hi".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];
    opts.seed = Some(42);
    opts.stop_sequences = Some(vec!["STOP".to_string(), "END".to_string()]);
    opts.frequency_penalty = Some(0.5);
    opts.presence_penalty = Some(0.3);

    let body = p.build_request_body(&opts, false).unwrap();

    assert_eq!(body["seed"], 42);

    let stop = body["stop"].as_array().unwrap();
    assert_eq!(stop.len(), 2);
    assert_eq!(stop[0], "STOP");
    assert_eq!(stop[1], "END");

    let fp = body["frequency_penalty"].as_f64().unwrap();
    assert!((fp - 0.5).abs() < 0.001);
    let pp = body["presence_penalty"].as_f64().unwrap();
    assert!((pp - 0.3).abs() < 0.001);
}
