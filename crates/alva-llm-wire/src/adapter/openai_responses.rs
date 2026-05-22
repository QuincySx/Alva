// INPUT:  super::{ToolAdapter, EncodedMessages, DecodedResponse, StreamDecodeState, AdapterError}
// OUTPUT: OpenAIResponsesAdapter (OpenAI Responses API `/v1/responses`)
// POS:    Flat function tool spec + typed input items + named-SSE stream (event: + data:).

//! OpenAI Responses API (`/v1/responses`) adapter.
//!
//! Key differences from Chat Completions:
//! - System prompt → separate `instructions` field (similar to Anthropic)
//! - Input is a sequence of typed items: `message` / `function_call` /
//!   `function_call_output` (tool call and its output are sibling items,
//!   not nested in an assistant message)
//! - Tool def is **flat**: `{type:"function", name, description, parameters}` —
//!   unlike Chat Completions' nested `{type:"function", function:{...}}`
//! - Streaming uses named SSE events: `event: response.output_text.delta`
//!   comes on one line, `data: {...}` on the next. The provider tracks
//!   the current event type via [`StreamDecodeState::event_type`].
//! - AMP `N3T` emits `strict: false, additionalProperties: true` — matches
//!   here (lenient schema; structured output is a separate path).

use serde_json::{Map, Value};

use super::{
    common::{schema_fix, tool_id},
    AdapterError, DecodedRequest, DecodedResponse, EncodedMessages, ProtocolAdapter,
    StreamDecodeState,
};
use crate::config::ModelConfig;
use crate::content::ContentBlock;
use crate::message::{Message, MessageRole, UsageMetadata};
use crate::stream::{StopReason, StreamEvent};
use crate::tool_def::ToolDefinition;

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAIResponsesAdapter;

impl OpenAIResponsesAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ProtocolAdapter for OpenAIResponsesAdapter {
    fn provider(&self) -> &'static str {
        "openai-responses"
    }

    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        tools
            .iter()
            .map(|t| {
                let mut params = t.parameters.clone();
                // AMP `N3T` defaults: additionalProperties: true, strict: false.
                schema_fix::force_additional_properties(&mut params, true);
                serde_json::json!({
                    "type": "function",
                    "name": &t.name,
                    "description": &t.description,
                    "parameters": params,
                    "strict": false,
                })
            })
            .collect()
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        let mut instruction_segments: Vec<String> = Vec::new();
        let mut input: Vec<Value> = Vec::new();

        for m in messages {
            match m.role {
                MessageRole::System => {
                    let text = m.text_content();
                    if !text.is_empty() {
                        instruction_segments.push(text);
                    }
                }
                MessageRole::User => {
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": "user",
                        "content": m.text_content(),
                    }));
                }
                MessageRole::Assistant => {
                    let text = m.text_content();
                    if !text.is_empty() {
                        input.push(serde_json::json!({
                            "type": "message",
                            "role": "assistant",
                            "content": text,
                        }));
                    }
                    for b in &m.content {
                        if let ContentBlock::ToolUse { id, name, input: args } = b {
                            input.push(serde_json::json!({
                                "type": "function_call",
                                "call_id": tool_id::to_provider(id),
                                "name": name,
                                "arguments": args.to_string(),
                            }));
                        }
                    }
                }
                MessageRole::Tool => {
                    let mut parts: Vec<String> = Vec::new();
                    let mut call_id = m.tool_call_id.clone();
                    for b in &m.content {
                        if let ContentBlock::ToolResult { id, content, .. } = b {
                            if call_id.is_none() {
                                call_id = Some(id.clone());
                            }
                            for tc in content {
                                parts.push(tc.to_model_string());
                            }
                        } else if let Some(t) = b.as_text() {
                            parts.push(t.to_string());
                        }
                    }
                    let output = parts.join("\n");
                    let call_id = call_id.unwrap_or_else(|| "unknown".to_string());
                    input.push(serde_json::json!({
                        "type": "function_call_output",
                        "call_id": tool_id::to_provider(&call_id),
                        "output": output,
                    }));
                }
            }
        }

        EncodedMessages {
            system_segments: if instruction_segments.is_empty() {
                None
            } else {
                Some(instruction_segments)
            },
            messages: input,
        }
    }

    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError> {
        let output_arr = response
            .get("output")
            .and_then(Value::as_array)
            .ok_or(AdapterError::MissingField("output"))?;

        let mut blocks: Vec<ContentBlock> = Vec::new();
        for item in output_arr {
            let item_type = item
                .get("type")
                .and_then(Value::as_str)
                .ok_or(AdapterError::MissingField("output[].type"))?;
            match item_type {
                "message" => {
                    if let Some(content_parts) = item.get("content").and_then(Value::as_array) {
                        for part in content_parts {
                            let part_type =
                                part.get("type").and_then(Value::as_str).unwrap_or("");
                            match part_type {
                                "output_text" | "text" => {
                                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                                        if !text.is_empty() {
                                            blocks.push(ContentBlock::Text {
                                                text: text.to_string(),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                }
                "function_call" => {
                    let raw_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                    let id = tool_id::to_normalized(raw_id);
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_str = item
                        .get("arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input: Value =
                        serde_json::from_str(args_str).unwrap_or(Value::Object(Map::new()));
                    blocks.push(ContentBlock::ToolUse { id, name, input });
                }
                _ => {}
            }
        }

        let usage = response.get("usage").map(|u| {
            let (cache_creation_input_tokens, cache_read_input_tokens) =
                super::common::cache_usage::extract_openai_compat(u);
            UsageMetadata {
                input_tokens: u
                    .get("input_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                output_tokens: u
                    .get("output_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                total_tokens: u.get("total_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
                cache_creation_input_tokens,
                cache_read_input_tokens,
            }
        });

        Ok(DecodedResponse {
            message: Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: blocks,
                tool_call_id: None,
                usage: usage.clone(),
                timestamp: chrono::Utc::now().timestamp_millis(),
            },
            usage,
        })
    }

    fn decode_request(&self, body: &Value) -> Result<DecodedRequest, AdapterError> {
        // -- model (required) ------------------------------------------------
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .ok_or(AdapterError::MissingField("model"))?
            .to_string();

        // -- messages --------------------------------------------------------
        let mut messages: Vec<Message> = Vec::new();

        // instructions → system message (prepended)
        if let Some(instructions) = body.get("instructions").and_then(Value::as_str) {
            if !instructions.is_empty() {
                messages.push(Message::system(instructions));
            }
        }

        // input[] → messages
        if let Some(input_arr) = body.get("input").and_then(Value::as_array) {
            for item in input_arr {
                let role_str = item.get("role").and_then(Value::as_str).unwrap_or("user");
                let role = match role_str {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    "tool" => MessageRole::Tool,
                    _ => MessageRole::User,
                };

                // Handle function_call items → ToolUse content block
                let item_type = item.get("type").and_then(Value::as_str).unwrap_or("message");
                if item_type == "function_call" {
                    let raw_id =
                        item.get("call_id").and_then(Value::as_str).unwrap_or("unknown");
                    let id = tool_id::to_normalized(raw_id);
                    let name = item
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let args_str =
                        item.get("arguments").and_then(Value::as_str).unwrap_or("{}");
                    let input: Value =
                        serde_json::from_str(args_str).unwrap_or(Value::Object(Map::new()));
                    messages.push(Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: MessageRole::Assistant,
                        content: vec![ContentBlock::ToolUse { id, name, input }],
                        tool_call_id: None,
                        usage: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    });
                    continue;
                }

                if item_type == "function_call_output" {
                    let raw_id =
                        item.get("call_id").and_then(Value::as_str).unwrap_or("unknown");
                    let id = tool_id::to_normalized(raw_id);
                    let output =
                        item.get("output").and_then(Value::as_str).unwrap_or("").to_string();
                    messages.push(Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: MessageRole::Tool,
                        content: vec![ContentBlock::ToolResult {
                            id: id.clone(),
                            content: vec![crate::tool_payload::ToolContent::Text {
                                text: output,
                            }],
                            is_error: false,
                        }],
                        tool_call_id: Some(id),
                        usage: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    });
                    continue;
                }

                // Regular message items — parse content field
                let content_val = item.get("content");
                let blocks: Vec<ContentBlock> = match content_val {
                    // content is a plain string
                    Some(Value::String(s)) => {
                        vec![ContentBlock::Text { text: s.clone() }]
                    }
                    // content is an array of typed parts
                    Some(Value::Array(parts)) => {
                        let mut blks: Vec<ContentBlock> = Vec::new();
                        for part in parts {
                            let part_type =
                                part.get("type").and_then(Value::as_str).unwrap_or("");
                            match part_type {
                                "input_image" | "image_url" | "image" => {
                                    return Err(AdapterError::UnexpectedFormat(
                                        "image input not supported".into(),
                                    ));
                                }
                                "input_text" | "output_text" | "text" => {
                                    if let Some(text) =
                                        part.get("text").and_then(Value::as_str)
                                    {
                                        blks.push(ContentBlock::Text {
                                            text: text.to_string(),
                                        });
                                    }
                                }
                                // Skip unknown part types for forward-compat
                                _ => {}
                            }
                        }
                        blks
                    }
                    // No content field — empty
                    _ => vec![],
                };

                messages.push(Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role,
                    content: blocks,
                    tool_call_id: None,
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                });
            }
        }

        // -- tools -----------------------------------------------------------
        // Responses API tool shape is FLAT: { type, name, description, parameters }
        // (NOT nested under a "function" key like Chat Completions).
        let tools: Vec<ToolDefinition> = body
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        let name = t.get("name").and_then(Value::as_str)?.to_string();
                        let description = t
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let parameters = t
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new()));
                        Some(ToolDefinition { name, description, parameters })
                    })
                    .collect()
            })
            .unwrap_or_default();

        // -- config ----------------------------------------------------------
        let temperature = body
            .get("temperature")
            .and_then(Value::as_f64)
            .map(|v| v as f32);
        let top_p = body.get("top_p").and_then(Value::as_f64).map(|v| v as f32);
        let max_tokens = body
            .get("max_output_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let reasoning_effort = body
            .pointer("/reasoning/effort")
            .and_then(Value::as_str)
            .and_then(crate::config::ReasoningEffort::parse);

        let config = ModelConfig {
            temperature,
            top_p,
            max_tokens,
            reasoning_effort,
            ..ModelConfig::default()
        };

        // -- stream ----------------------------------------------------------
        let stream = body.get("stream").and_then(Value::as_bool).unwrap_or(false);

        Ok(DecodedRequest { model, messages, tools, config, stream })
    }

    fn decode_stream_event(
        &self,
        event: &Value,
        state: &mut StreamDecodeState,
    ) -> Result<Vec<StreamEvent>, AdapterError> {
        // Responses API uses named SSE events. The provider sets
        // `state.event_type` from the preceding `event:` line.
        let event_type = state.event_type.as_deref().unwrap_or("");
        let mut out = Vec::new();
        match event_type {
            "response.output_text.delta" => {
                if let Some(delta) = event.get("delta").and_then(Value::as_str) {
                    if !delta.is_empty() {
                        out.push(StreamEvent::TextDelta { text: delta.to_string() });
                    }
                }
            }
            "response.function_call_arguments.delta" => {
                let delta = event.get("delta").and_then(Value::as_str).unwrap_or("");
                let raw_id = event.get("call_id").and_then(Value::as_str).unwrap_or("");
                let id = if raw_id.is_empty() {
                    String::new()
                } else {
                    tool_id::to_normalized(raw_id)
                };
                let name = event.get("name").and_then(Value::as_str).map(String::from);
                if !id.is_empty() {
                    state
                        .tool_input_buf
                        .entry(id.clone())
                        .or_default()
                        .push_str(delta);
                }
                out.push(StreamEvent::ToolCallDelta {
                    id,
                    name,
                    arguments_delta: delta.to_string(),
                });
            }
            "response.output_item.added" => {
                if let Some(item) = event.get("item") {
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                    if item_type == "function_call" {
                        let raw_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let id = tool_id::to_normalized(raw_id);
                        let name = item
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        if !id.is_empty() {
                            state.tool_input_buf.entry(id.clone()).or_default();
                            state.block_type.insert(0, id.clone()); // track open
                            out.push(StreamEvent::ToolCallStart { id, name });
                        }
                    }
                }
            }
            "response.output_item.done" => {
                if let Some(item) = event.get("item") {
                    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("");
                    if item_type == "function_call" {
                        let raw_id = item.get("call_id").and_then(Value::as_str).unwrap_or("");
                        let id = tool_id::to_normalized(raw_id);
                        if !id.is_empty() {
                            out.push(StreamEvent::ToolCallEnd { id });
                        }
                    }
                }
            }
            "response.completed" => {
                if let Some(usage) = event.pointer("/response/usage") {
                    let (cache_creation_input_tokens, cache_read_input_tokens) =
                        super::common::cache_usage::extract_openai_compat(usage);
                    out.push(StreamEvent::Usage(UsageMetadata {
                        input_tokens: usage
                            .get("input_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32,
                        output_tokens: usage
                            .get("output_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32,
                        total_tokens: usage
                            .get("total_tokens")
                            .and_then(Value::as_u64)
                            .unwrap_or(0) as u32,
                        cache_creation_input_tokens,
                        cache_read_input_tokens,
                    }));
                }
                // Map response.completed → EndTurn (natural completion).
                // The response payload does not carry a fine-grained finish_reason
                // equivalent; EndTurn is the correct semantic for a completed response.
                out.push(StreamEvent::Stop { reason: StopReason::EndTurn });
                out.push(StreamEvent::Done);
            }
            "response.incomplete" => {
                // Response was cut short (token limit or timeout).
                out.push(StreamEvent::Stop { reason: StopReason::MaxTokens });
            }
            "response.failed" => {
                out.push(StreamEvent::Stop { reason: StopReason::Other("failed".to_string()) });
            }
            _ => {}
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::ReasoningEffort;
    use crate::message::MessageRole;

    #[test]
    fn responses_decode_request_basic() {
        let body = serde_json::json!({
            "model": "gpt-x",
            "instructions": "be brief",
            "input": [{"role":"user","content":[{"type":"input_text","text":"hi"}]}],
            "tools": [{"type":"function","name":"read","description":"d","parameters":{"type":"object"}}],
            "stream": true,
            "reasoning": {"effort":"high"}
        });
        let r = OpenAIResponsesAdapter::new().decode_request(&body).unwrap();
        assert_eq!(r.model, "gpt-x");
        assert!(r.stream);
        assert_eq!(r.tools[0].name, "read");
        assert_eq!(r.config.reasoning_effort, Some(ReasoningEffort::High));
        assert!(matches!(r.messages[0].role, MessageRole::System));
        assert!(r.messages.iter().any(|m| matches!(m.role, MessageRole::User)));
    }

    #[test]
    fn responses_decode_request_rejects_image() {
        let body = serde_json::json!({"model":"m","input":[{"role":"user","content":[{"type":"input_image","image_url":"x"}]}]});
        assert!(matches!(OpenAIResponsesAdapter::new().decode_request(&body), Err(AdapterError::UnexpectedFormat(_))));
    }

    #[test]
    fn encode_tools_flat_not_nested() {
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: String::new(),
            parameters: serde_json::json!({"type":"object"}),
        }];
        let encoded = OpenAIResponsesAdapter.encode_tools(&tools);
        // Flat spec: no `function: {}` wrapper
        assert_eq!(encoded[0]["type"], "function");
        assert_eq!(encoded[0]["name"], "read");
        assert_eq!(encoded[0]["strict"], false);
    }

    #[test]
    fn encode_messages_splits_instructions() {
        let msgs = vec![
            Message::system("you are alva"),
            Message::user("hi"),
        ];
        let out = OpenAIResponsesAdapter.encode_messages(&msgs);
        assert_eq!(out.system_flat().as_deref(), Some("you are alva"));
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0]["type"], "message");
        assert_eq!(out.messages[0]["role"], "user");
    }

    #[test]
    fn encode_assistant_with_tool_call_produces_sibling_items() {
        let msg = Message {
            id: "m".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text { text: "doing".into() },
                ContentBlock::ToolUse {
                    id: "call_a".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/x"}),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let out = OpenAIResponsesAdapter.encode_messages(&[msg]);
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0]["type"], "message");
        assert_eq!(out.messages[1]["type"], "function_call");
        assert_eq!(out.messages[1]["call_id"], "call_a");
    }

    #[test]
    fn decode_response_handles_message_and_function_call() {
        let resp = serde_json::json!({
            "output": [
                {
                    "type": "message",
                    "content": [
                        { "type": "output_text", "text": "sure" }
                    ]
                },
                {
                    "type": "function_call",
                    "call_id": "call_a",
                    "name": "read",
                    "arguments": "{\"path\":\"/a\"}"
                }
            ],
            "usage": { "input_tokens": 5, "output_tokens": 10, "total_tokens": 15 }
        });
        let decoded = OpenAIResponsesAdapter.decode_response(&resp).unwrap();
        assert_eq!(decoded.message.content.len(), 2);
        match &decoded.message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                // Normalized: "call_a" → "toolu_call_a"
                assert_eq!(id, "toolu_call_a");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
    }

    #[test]
    fn decode_stream_dispatches_via_event_type() {
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.output_text.delta".into());
        let ev = serde_json::json!({ "delta": "hello" });
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        match &out[0] {
            StreamEvent::TextDelta { text } => assert_eq!(text, "hello"),
            _ => panic!("expected TextDelta"),
        }

        state.event_type = Some("response.function_call_arguments.delta".into());
        let ev = serde_json::json!({ "delta": "{\"p\":", "call_id": "call_a", "name": "read" });
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        match &out[0] {
            StreamEvent::ToolCallDelta { id, arguments_delta, .. } => {
                assert_eq!(id, "toolu_call_a");
                assert_eq!(arguments_delta, "{\"p\":");
            }
            _ => panic!("expected ToolCallDelta"),
        }
    }

    #[test]
    fn decode_stream_completed_emits_usage_stop_and_done() {
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.completed".into());
        let ev = serde_json::json!({
            "response": { "usage": { "input_tokens": 1, "output_tokens": 2, "total_tokens": 3 } }
        });
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 3, "expected Usage + Stop + Done");
        assert!(matches!(out[0], StreamEvent::Usage(_)));
        assert!(matches!(&out[1], StreamEvent::Stop { reason: StopReason::EndTurn }));
        assert!(matches!(out[2], StreamEvent::Done));
    }

    #[test]
    fn decode_stream_incomplete_emits_max_tokens_stop() {
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.incomplete".into());
        let ev = serde_json::json!({});
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 1);
        assert!(matches!(&out[0], StreamEvent::Stop { reason: StopReason::MaxTokens }));
    }

    #[test]
    fn decode_stream_failed_emits_other_stop() {
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.failed".into());
        let ev = serde_json::json!({});
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamEvent::Stop { reason: StopReason::Other(s) } => assert_eq!(s, "failed"),
            _ => panic!("expected Stop{{Other(\"failed\")}}"),
        }
    }
}
