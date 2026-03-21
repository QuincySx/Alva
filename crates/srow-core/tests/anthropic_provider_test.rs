// Integration tests for the Anthropic Messages API provider.

use srow_core::adapters::llm::anthropic::AnthropicLanguageModel;
use srow_core::ports::provider::content::LanguageModelContent;
use srow_core::ports::provider::language_model::{
    LanguageModelCallOptions, LanguageModelStreamPart, UnifiedFinishReason,
};
use srow_core::ports::provider::prompt::{
    AssistantContentPart, DataContent, LanguageModelMessage, ToolContentPart, UserContentPart,
};
use srow_core::ports::provider::tool_types::{
    FunctionTool, LanguageModelTool, ToolChoice, ToolResultOutput,
};

use futures::StreamExt;
use std::collections::HashMap;

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

fn provider() -> AnthropicLanguageModel {
    AnthropicLanguageModel::new("test-key", "claude-sonnet-4-20250514")
}

// ---------------------------------------------------------------------------
// Helper: produce SSE stream parts from a raw SSE text string
// ---------------------------------------------------------------------------

async fn sse_text_to_parts(
    provider: &AnthropicLanguageModel,
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
// Test 1: Message merging — consecutive user + tool messages merged
// ===========================================================================

#[test]
fn test_message_merging() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![
        LanguageModelMessage::User {
            content: vec![UserContentPart::Text {
                text: "Hello".to_string(),
                provider_options: None,
            }],
            provider_options: None,
        },
        LanguageModelMessage::Assistant {
            content: vec![AssistantContentPart::ToolCall {
                tool_call_id: "tc1".to_string(),
                tool_name: "search".to_string(),
                input: serde_json::json!({"q": "test"}),
                provider_options: None,
            }],
            provider_options: None,
        },
        // Tool message should merge into a user message
        LanguageModelMessage::Tool {
            content: vec![ToolContentPart::ToolResult {
                tool_call_id: "tc1".to_string(),
                tool_name: "search".to_string(),
                output: ToolResultOutput::Text {
                    value: "result data".to_string(),
                },
                provider_options: None,
            }],
            provider_options: None,
        },
        // Another user message — should merge with the tool (both are "user" role)
        LanguageModelMessage::User {
            content: vec![UserContentPart::Text {
                text: "Follow up".to_string(),
                provider_options: None,
            }],
            provider_options: None,
        },
    ];

    let body = p.build_request_body(&opts, false).unwrap();
    let messages = body["messages"].as_array().unwrap();

    // Should be: user, assistant, user (merged tool + user)
    assert_eq!(messages.len(), 3);
    assert_eq!(messages[0]["role"], "user");
    assert_eq!(messages[1]["role"], "assistant");
    assert_eq!(messages[2]["role"], "user");

    // The merged user message should have both tool_result and text blocks
    let merged_content = messages[2]["content"].as_array().unwrap();
    assert_eq!(merged_content.len(), 2);
    assert_eq!(merged_content[0]["type"], "tool_result");
    assert_eq!(merged_content[1]["type"], "text");
    assert_eq!(merged_content[1]["text"], "Follow up");
}

// ===========================================================================
// Test 2: Basic request body structure
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
    opts.top_k = Some(40);
    opts.stop_sequences = Some(vec!["STOP".to_string()]);

    let body = p.build_request_body(&opts, false).unwrap();

    assert_eq!(body["model"], "claude-sonnet-4-20250514");
    assert_eq!(body["max_tokens"], 1024);
    // Float comparison with tolerance (f32 → f64 rounding)
    let temp = body["temperature"].as_f64().unwrap();
    assert!((temp - 0.7).abs() < 0.001, "temperature: {}", temp);
    let top_p_val = body["top_p"].as_f64().unwrap();
    assert!((top_p_val - 0.9).abs() < 0.001, "top_p: {}", top_p_val);
    assert_eq!(body["top_k"], 40);

    // System is a top-level array of content blocks
    let system = body["system"].as_array().unwrap();
    assert_eq!(system.len(), 1);
    assert_eq!(system[0]["type"], "text");
    assert_eq!(system[0]["text"], "You are helpful.");

    // Messages
    let messages = body["messages"].as_array().unwrap();
    assert_eq!(messages.len(), 1);
    assert_eq!(messages[0]["role"], "user");

    // Stop sequences
    let stop = body["stop_sequences"].as_array().unwrap();
    assert_eq!(stop.len(), 1);
    assert_eq!(stop[0], "STOP");

    // No stream flag
    assert!(body.get("stream").is_none());
}

// ===========================================================================
// Test 3: Request body with tools — input_schema (not parameters), tool_choice
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

    // Tools should use input_schema, not parameters
    let tools = body["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0]["name"], "web_search");
    assert_eq!(tools[0]["description"], "Search the web");
    assert!(tools[0].get("input_schema").is_some());
    assert!(tools[0].get("parameters").is_none());

    // Tool choice
    assert_eq!(body["tool_choice"]["type"], "auto");

    // Test Required → "any"
    let mut opts2 = opts;
    opts2.tool_choice = Some(ToolChoice::Required);
    let body2 = p.build_request_body(&opts2, false).unwrap();
    assert_eq!(body2["tool_choice"]["type"], "any");
}

// ===========================================================================
// Test 4: Request body with image content
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

    // Base64 image
    assert_eq!(content[1]["type"], "image");
    assert_eq!(content[1]["source"]["type"], "base64");
    assert_eq!(content[1]["source"]["media_type"], "image/png");
    assert_eq!(content[1]["source"]["data"], "iVBORw0KGgo=");

    // URL image
    assert_eq!(content[2]["type"], "image");
    assert_eq!(content[2]["source"]["type"], "url");
    assert_eq!(content[2]["source"]["url"], "https://example.com/cat.jpg");
}

// ===========================================================================
// Test 5: Response parsing — basic text response
// ===========================================================================

#[test]
fn test_response_parsing() {
    let p = provider();
    let json = serde_json::json!({
        "id": "msg_01abc",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "text", "text": "Hello, world!"}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 25,
            "output_tokens": 50,
            "cache_creation_input_tokens": 0,
            "cache_read_input_tokens": 0
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.content.len(), 1);
    match &result.content[0] {
        LanguageModelContent::Text { text, .. } => assert_eq!(text, "Hello, world!"),
        _ => panic!("Expected Text content"),
    }

    assert_eq!(result.finish_reason.unified, UnifiedFinishReason::Stop);
    assert_eq!(result.finish_reason.raw.as_deref(), Some("end_turn"));
    assert_eq!(result.usage.input_tokens.total, Some(25));
    assert_eq!(result.usage.output_tokens.total, Some(50));

    let resp = result.response.unwrap();
    assert_eq!(resp.id.as_deref(), Some("msg_01abc"));
    assert_eq!(resp.model_id.as_deref(), Some("claude-sonnet-4-20250514"));
}

// ===========================================================================
// Test 6: Response parsing with tool_use content blocks
// ===========================================================================

#[test]
fn test_response_with_tool_use() {
    let p = provider();
    let json = serde_json::json!({
        "id": "msg_02xyz",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "text", "text": "Let me search that."},
            {
                "type": "tool_use",
                "id": "toolu_01abc",
                "name": "web_search",
                "input": {"query": "cats"}
            }
        ],
        "stop_reason": "tool_use",
        "usage": {
            "input_tokens": 30,
            "output_tokens": 60
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.content.len(), 2);

    match &result.content[0] {
        LanguageModelContent::Text { text, .. } => {
            assert_eq!(text, "Let me search that.")
        }
        _ => panic!("Expected Text content"),
    }

    match &result.content[1] {
        LanguageModelContent::ToolCall {
            tool_call_id,
            tool_name,
            input,
            ..
        } => {
            assert_eq!(tool_call_id, "toolu_01abc");
            assert_eq!(tool_name, "web_search");
            // input is stringified JSON
            let parsed: serde_json::Value = serde_json::from_str(&input).unwrap();
            assert_eq!(parsed["query"], "cats");
        }
        _ => panic!("Expected ToolCall content"),
    }

    assert_eq!(result.finish_reason.unified, UnifiedFinishReason::ToolCalls);
}

// ===========================================================================
// Test 7: Response parsing with thinking blocks
// ===========================================================================

#[test]
fn test_response_with_thinking() {
    let p = provider();
    let json = serde_json::json!({
        "id": "msg_03def",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {
                "type": "thinking",
                "thinking": "Let me reason about this...",
                "signature": "sig123"
            },
            {"type": "text", "text": "The answer is 42."}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 10,
            "output_tokens": 40
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.content.len(), 2);

    match &result.content[0] {
        LanguageModelContent::Reasoning { text, .. } => {
            assert_eq!(text, "Let me reason about this...")
        }
        _ => panic!("Expected Reasoning content"),
    }

    match &result.content[1] {
        LanguageModelContent::Text { text, .. } => assert_eq!(text, "The answer is 42."),
        _ => panic!("Expected Text content"),
    }
}

// ===========================================================================
// Test 8: SSE stream — text generation flow
// ===========================================================================

#[tokio::test]
async fn test_sse_stream_text() {
    let p = provider();

    let sse_text = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_s1\",\"usage\":{\"input_tokens\":20}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\" world\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":30}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let parts = sse_text_to_parts(&p, sse_text).await;

    // Verify event sequence
    let mut found_text_start = false;
    let mut found_text_delta_hello = false;
    let mut found_text_delta_world = false;
    let mut found_text_end = false;
    let mut found_finish = false;

    for part in &parts {
        match part {
            LanguageModelStreamPart::TextStart { id } => {
                assert_eq!(id, "0");
                found_text_start = true;
            }
            LanguageModelStreamPart::TextDelta { id, delta } => {
                assert_eq!(id, "0");
                if delta == "Hello" {
                    found_text_delta_hello = true;
                }
                if delta == " world" {
                    found_text_delta_world = true;
                }
            }
            LanguageModelStreamPart::TextEnd { id } => {
                assert_eq!(id, "0");
                found_text_end = true;
            }
            LanguageModelStreamPart::Finish {
                usage,
                finish_reason,
                ..
            } => {
                assert_eq!(finish_reason.unified, UnifiedFinishReason::Stop);
                assert_eq!(usage.input_tokens.total, Some(20));
                assert_eq!(usage.output_tokens.total, Some(30));
                found_finish = true;
            }
            _ => {}
        }
    }

    assert!(found_text_start, "Missing TextStart");
    assert!(found_text_delta_hello, "Missing TextDelta 'Hello'");
    assert!(found_text_delta_world, "Missing TextDelta ' world'");
    assert!(found_text_end, "Missing TextEnd");
    assert!(found_finish, "Missing Finish");
}

// ===========================================================================
// Test 9: SSE stream — tool call flow
// ===========================================================================

#[tokio::test]
async fn test_sse_stream_tool_call() {
    let p = provider();

    let sse_text = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_t1\",\"usage\":{\"input_tokens\":15}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"tool_use\",\"id\":\"toolu_abc\",\"name\":\"web_search\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"{\\\"q\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"input_json_delta\",\"partial_json\":\"\\\":\\\"cats\\\"}\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"tool_use\"},\"usage\":{\"output_tokens\":25}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
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
                assert_eq!(id, "toolu_abc");
                assert_eq!(tool_name, "web_search");
                found_tool_start = true;
            }
            LanguageModelStreamPart::ToolInputDelta { id, .. } => {
                assert_eq!(id, "toolu_abc");
                found_tool_delta = true;
            }
            LanguageModelStreamPart::ToolInputEnd { id } => {
                assert_eq!(id, "toolu_abc");
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
// Test 10: SSE stream — thinking/reasoning flow
// ===========================================================================

#[tokio::test]
async fn test_sse_stream_thinking() {
    let p = provider();

    let sse_text = concat!(
        "event: message_start\n",
        "data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_r1\",\"usage\":{\"input_tokens\":10}}}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"thinking\",\"thinking\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\"Let me think\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"thinking_delta\",\"thinking\":\" about this\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":0}\n\n",
        "event: content_block_start\n",
        "data: {\"type\":\"content_block_start\",\"index\":1,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n",
        "event: content_block_delta\n",
        "data: {\"type\":\"content_block_delta\",\"index\":1,\"delta\":{\"type\":\"text_delta\",\"text\":\"Answer\"}}\n\n",
        "event: content_block_stop\n",
        "data: {\"type\":\"content_block_stop\",\"index\":1}\n\n",
        "event: message_delta\n",
        "data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\"},\"usage\":{\"output_tokens\":40}}\n\n",
        "event: message_stop\n",
        "data: {\"type\":\"message_stop\"}\n\n",
    );

    let parts = sse_text_to_parts(&p, sse_text).await;

    let mut found_reasoning_start = false;
    let mut found_reasoning_delta = false;
    let mut found_reasoning_end = false;
    let mut found_text_start = false;
    let mut found_text_delta = false;

    for part in &parts {
        match part {
            LanguageModelStreamPart::ReasoningStart { id } => {
                assert_eq!(id, "0");
                found_reasoning_start = true;
            }
            LanguageModelStreamPart::ReasoningDelta { id, delta } => {
                assert_eq!(id, "0");
                assert!(
                    delta == "Let me think" || delta == " about this",
                    "Unexpected reasoning delta: {}",
                    delta
                );
                found_reasoning_delta = true;
            }
            LanguageModelStreamPart::ReasoningEnd { id } => {
                assert_eq!(id, "0");
                found_reasoning_end = true;
            }
            LanguageModelStreamPart::TextStart { id } => {
                assert_eq!(id, "1");
                found_text_start = true;
            }
            LanguageModelStreamPart::TextDelta { id, delta } => {
                assert_eq!(id, "1");
                assert_eq!(delta, "Answer");
                found_text_delta = true;
            }
            _ => {}
        }
    }

    assert!(found_reasoning_start, "Missing ReasoningStart");
    assert!(found_reasoning_delta, "Missing ReasoningDelta");
    assert!(found_reasoning_end, "Missing ReasoningEnd");
    assert!(found_text_start, "Missing TextStart after reasoning");
    assert!(found_text_delta, "Missing TextDelta after reasoning");
}

// ===========================================================================
// Test 11: Usage with cache tokens
// ===========================================================================

#[test]
fn test_usage_with_cache() {
    let p = provider();
    let json = serde_json::json!({
        "id": "msg_cache",
        "model": "claude-sonnet-4-20250514",
        "content": [
            {"type": "text", "text": "Cached response."}
        ],
        "stop_reason": "end_turn",
        "usage": {
            "input_tokens": 100,
            "output_tokens": 50,
            "cache_creation_input_tokens": 30,
            "cache_read_input_tokens": 70
        }
    });

    let result = p.parse_response(&json).unwrap();

    assert_eq!(result.usage.input_tokens.total, Some(100));
    assert_eq!(result.usage.input_tokens.cache_read, Some(70));
    assert_eq!(result.usage.input_tokens.cache_write, Some(30));
    assert_eq!(result.usage.output_tokens.total, Some(50));

    // Raw usage should be preserved
    assert!(result.usage.raw.is_some());
    let raw = result.usage.raw.unwrap();
    assert_eq!(raw["cache_creation_input_tokens"], 30);
    assert_eq!(raw["cache_read_input_tokens"], 70);
}

// ===========================================================================
// Test 12: Thinking config in request body
// ===========================================================================

#[test]
fn test_request_body_with_thinking() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Think deeply".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];
    opts.max_output_tokens = Some(4096);

    // Set thinking via provider_options
    let mut anthropic_map = serde_json::Map::new();
    anthropic_map.insert(
        "thinking".to_string(),
        serde_json::json!({"type": "enabled", "budget_tokens": 10000}),
    );
    let mut provider_opts = HashMap::new();
    provider_opts.insert("anthropic".to_string(), anthropic_map);
    opts.provider_options = Some(provider_opts);

    let body = p.build_request_body(&opts, false).unwrap();

    // max_tokens should be budget + max_output = 10000 + 4096 = 14096
    assert_eq!(body["max_tokens"], 14096);

    // Thinking config should be present
    assert_eq!(body["thinking"]["type"], "enabled");
    assert_eq!(body["thinking"]["budget_tokens"], 10000);
}

// ===========================================================================
// Test 13: Tool choice None — no tools sent
// ===========================================================================

#[test]
fn test_tool_choice_none_omits_tools() {
    let p = provider();
    let mut opts = default_options();
    opts.prompt = vec![LanguageModelMessage::User {
        content: vec![UserContentPart::Text {
            text: "Hi".to_string(),
            provider_options: None,
        }],
        provider_options: None,
    }];
    opts.tools = Some(vec![LanguageModelTool::Function(FunctionTool {
        name: "search".to_string(),
        description: Some("Search".to_string()),
        input_schema: serde_json::json!({"type": "object"}),
        strict: None,
        provider_options: None,
    })]);
    opts.tool_choice = Some(ToolChoice::None);

    let body = p.build_request_body(&opts, false).unwrap();

    // Tools and tool_choice should not be present
    assert!(body.get("tools").is_none());
    assert!(body.get("tool_choice").is_none());
}

// ===========================================================================
// Test 14: Stream flag is set when streaming
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

    let body_stream = p.build_request_body(&opts, true).unwrap();
    assert_eq!(body_stream["stream"], true);
}
