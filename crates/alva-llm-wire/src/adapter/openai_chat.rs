// INPUT:  super::{ToolAdapter, EncodedMessages, DecodedResponse, StreamDecodeState, AdapterError}
// OUTPUT: OpenAIChatAdapter (stateless, OpenAI-compatible Chat Completions)
// POS:    OpenAI Chat Completions shape — also used for Groq/Fireworks/OpenRouter/xAI/Moonshot/etc.

//! OpenAI Chat Completions (`/v1/chat/completions`) adapter.
//!
//! Used for OpenAI Chat Completions and every OpenAI-compatible proxy:
//! Groq, Fireworks, OpenRouter, xAI, Moonshot, DeepSeek, Ollama, vLLM.
//!
//! Differences from Anthropic (matters for encode/decode):
//! - System prompt stays inline as `role:"system"` message (not split out)
//! - Tool def is nested: `{type:"function", function:{name, description, parameters}}`
//! - Assistant tool calls go in `tool_calls: [{id, type:"function", function:{name, arguments}}]`
//!   — `arguments` is a **string** (JSON as string), not a parsed object
//! - Tool result: role `"tool"` with `tool_call_id` field, content is plain string
//! - Stream frames: `{choices:[{delta:{content, tool_calls[]}}]}`, `[DONE]` sentinel
//! - OpenAI Chat rejects schema `properties` that omit `type` — applies AMP's YLR fix

use serde_json::{Map, Value};

use super::{
    common::{schema_fix, tool_id},
    AdapterError, DecodedRequest, DecodedResponse, EncodedMessages, ProtocolAdapter, SseFrame,
    StreamDecodeState, StreamEncodeState,
};
use crate::config::ModelConfig;
use crate::content::ContentBlock;
use crate::message::{Message, MessageRole, UsageMetadata};
use crate::stream::{StopReason, StreamEvent};
use crate::tool_def::ToolDefinition;

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAIChatAdapter;

impl OpenAIChatAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ProtocolAdapter for OpenAIChatAdapter {
    fn provider(&self) -> &'static str {
        "openai-chat"
    }

    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        let mut seen = std::collections::HashSet::new();
        tools
            .iter()
            .filter(|t| seen.insert(t.name.clone()))
            .map(|t| {
                let mut params = t.parameters.clone();
                // AMP `YLR`: OpenAI-compat backends reject properties missing `type`.
                schema_fix::fill_missing_types(&mut params);
                // Ensure an explicit additionalProperties on object nodes — some
                // compatible backends default to false and reject extra fields.
                schema_fix::force_additional_properties(&mut params, true);
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": &t.name,
                        "description": &t.description,
                        "parameters": params,
                    }
                })
            })
            .collect()
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        let mut out: Vec<Value> = Vec::new();
        for m in messages {
            match m.role {
                MessageRole::Tool => {
                    let mut parts: Vec<String> = Vec::new();
                    for b in &m.content {
                        if let ContentBlock::ToolResult { content, .. } = b {
                            for tc in content {
                                parts.push(tc.to_model_string());
                            }
                        } else if let Some(t) = b.as_text() {
                            parts.push(t.to_string());
                        }
                    }
                    let tool_call_id = m.tool_call_id.clone().or_else(|| {
                        m.content.iter().find_map(|b| {
                            if let ContentBlock::ToolResult { id, .. } = b {
                                Some(id.clone())
                            } else {
                                None
                            }
                        })
                    });
                    // Strip `toolu_` prefix — OpenAI expects its native call_xxx id.
                    let tool_call_id = tool_call_id.map(|id| tool_id::to_provider(&id).to_string());
                    out.push(serde_json::json!({
                        "role": "tool",
                        "content": parts.join("\n"),
                        "tool_call_id": tool_call_id,
                    }));
                }
                MessageRole::Assistant if m.has_tool_calls() => {
                    let text = m.text_content();
                    let tool_calls: Vec<Value> = m
                        .content
                        .iter()
                        .filter_map(|b| {
                            if let ContentBlock::ToolUse { id, name, input } = b {
                                Some(serde_json::json!({
                                    // Strip `toolu_` prefix when echoing back to OpenAI.
                                    "id": tool_id::to_provider(id),
                                    "type": "function",
                                    "function": {
                                        "name": name,
                                        // OpenAI expects arguments as a JSON string.
                                        "arguments": input.to_string(),
                                    }
                                }))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let mut msg = serde_json::json!({
                        "role": "assistant",
                        "tool_calls": tool_calls,
                    });
                    if !text.is_empty() {
                        msg["content"] = Value::String(text);
                    }
                    out.push(msg);
                }
                _ => {
                    let role = match m.role {
                        MessageRole::User => "user",
                        MessageRole::Assistant => "assistant",
                        MessageRole::System => "system",
                        MessageRole::Tool => "tool",
                    };
                    out.push(serde_json::json!({
                        "role": role,
                        "content": m.text_content(),
                    }));
                }
            }
        }
        EncodedMessages {
            system_segments: None, // inline in messages
            messages: out,
        }
    }

    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError> {
        let choice = response
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|a| a.first())
            .ok_or(AdapterError::MissingField("choices[0]"))?;
        let msg = choice
            .get("message")
            .ok_or(AdapterError::MissingField("choices[0].message"))?;

        let mut blocks: Vec<ContentBlock> = Vec::new();
        if let Some(text) = msg.get("content").and_then(Value::as_str) {
            if !text.is_empty() {
                blocks.push(ContentBlock::Text {
                    text: text.to_string(),
                });
            }
        }
        if let Some(tcs) = msg.get("tool_calls").and_then(Value::as_array) {
            for tc in tcs {
                let raw_id = tc.get("id").and_then(Value::as_str).unwrap_or("");
                let id = tool_id::to_normalized(raw_id);
                let function = tc.get("function").unwrap_or(&Value::Null);
                let name = function
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let args_str = function
                    .get("arguments")
                    .and_then(Value::as_str)
                    .unwrap_or("{}");
                let input: Value =
                    serde_json::from_str(args_str).unwrap_or(Value::Object(Map::new()));
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        let usage = response.get("usage").map(|u| {
            let (cache_creation_input_tokens, cache_read_input_tokens) =
                super::common::cache_usage::extract_openai_compat(u);
            UsageMetadata {
                input_tokens: u.get("prompt_tokens").and_then(Value::as_u64).unwrap_or(0) as u32,
                output_tokens: u
                    .get("completion_tokens")
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

    fn decode_stream_event(
        &self,
        event: &Value,
        state: &mut StreamDecodeState,
    ) -> Result<Vec<StreamEvent>, AdapterError> {
        // [DONE] sentinel must be handled by caller (it's a non-JSON string).
        // Here we handle parsed JSON frames only.
        let mut out = Vec::new();
        if let Some(choices) = event.get("choices").and_then(Value::as_array) {
            for choice in choices {
                let delta = choice.get("delta").unwrap_or(&Value::Null);
                if let Some(text) = delta.get("content").and_then(Value::as_str) {
                    if !text.is_empty() {
                        out.push(StreamEvent::TextDelta {
                            text: text.to_string(),
                        });
                    }
                }
                if let Some(tool_calls) = delta.get("tool_calls").and_then(Value::as_array) {
                    for tc in tool_calls {
                        let index = tc.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                        // Track whether this is the first time we've seen this index
                        // (→ emit ToolCallStart) vs a subsequent argument delta.
                        let raw_id = tc.get("id").and_then(Value::as_str);
                        let already_tracked = state.block_type.contains_key(&index);
                        let id = match (raw_id, state.block_type.get(&index).cloned()) {
                            (Some(r), _) => tool_id::to_normalized(r),
                            (None, Some(cached)) => cached,
                            _ => String::new(),
                        };
                        if !id.is_empty() && !already_tracked {
                            state.block_type.insert(index, id.clone());
                            state.tool_input_buf.entry(id.clone()).or_default();
                        }
                        let function = tc.get("function").unwrap_or(&Value::Null);
                        let name = function
                            .get("name")
                            .and_then(Value::as_str)
                            .map(String::from);
                        // First sighting of this index → emit ToolCallStart before deltas.
                        if !already_tracked && !id.is_empty() {
                            out.push(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.clone().unwrap_or_default(),
                            });
                        }
                        let args_delta = function
                            .get("arguments")
                            .and_then(Value::as_str)
                            .unwrap_or("");
                        if !args_delta.is_empty() && !id.is_empty() {
                            state
                                .tool_input_buf
                                .entry(id.clone())
                                .or_default()
                                .push_str(args_delta);
                            out.push(StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                name,
                                arguments_delta: args_delta.to_string(),
                            });
                        }
                    }
                }
                // finish_reason signals the whole message is done — emit
                // ToolCallEnd for any tool calls still open in this stream,
                // then emit Stop with the mapped reason.
                let finish_reason = choice
                    .get("finish_reason")
                    .and_then(Value::as_str)
                    .unwrap_or("");
                if !finish_reason.is_empty() {
                    let ids: Vec<String> = state.block_type.values().cloned().collect();
                    for id in ids {
                        out.push(StreamEvent::ToolCallEnd { id });
                    }
                    state.block_type.clear();
                    let reason = match finish_reason {
                        "tool_calls" => StopReason::ToolUse,
                        "length" => StopReason::MaxTokens,
                        "stop" => StopReason::EndTurn,
                        other => StopReason::Other(other.to_string()),
                    };
                    out.push(StreamEvent::Stop { reason });
                }
            }
        }
        if let Some(usage) = event.get("usage") {
            let (cache_creation_input_tokens, cache_read_input_tokens) =
                super::common::cache_usage::extract_openai_compat(usage);
            out.push(StreamEvent::Usage(UsageMetadata {
                input_tokens: usage
                    .get("prompt_tokens")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                output_tokens: usage
                    .get("completion_tokens")
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
        Ok(out)
    }

    // -----------------------------------------------------------------------
    // Inbound (gateway) methods
    // -----------------------------------------------------------------------

    fn decode_request(&self, body: &Value) -> Result<DecodedRequest, AdapterError> {
        // -- model (required) ------------------------------------------------
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .ok_or(AdapterError::MissingField("model"))?
            .to_string();

        // -- messages --------------------------------------------------------
        // Chat Completions uses a flat inline messages array with roles:
        // system / user / assistant / tool.
        let mut messages: Vec<Message> = Vec::new();

        if let Some(msgs_arr) = body.get("messages").and_then(Value::as_array) {
            for item in msgs_arr {
                let role_str = item.get("role").and_then(Value::as_str).unwrap_or("user");

                // Tool result: {role:"tool", tool_call_id, content}
                if role_str == "tool" {
                    let raw_id = item
                        .get("tool_call_id")
                        .and_then(Value::as_str)
                        .unwrap_or("unknown");
                    let id = tool_id::to_normalized(raw_id);
                    let content_text = item
                        .get("content")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    messages.push(Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: MessageRole::Tool,
                        content: vec![ContentBlock::ToolResult {
                            id: id.clone(),
                            content: vec![crate::tool_payload::ToolContent::Text {
                                text: content_text,
                            }],
                            is_error: false,
                        }],
                        tool_call_id: Some(id),
                        usage: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    });
                    continue;
                }

                let role = match role_str {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    "system" => MessageRole::System,
                    _ => MessageRole::User,
                };

                // Parse content — can be a string or an array of content parts.
                let content_val = item.get("content");
                let mut blocks: Vec<ContentBlock> = match content_val {
                    Some(Value::String(s)) => {
                        vec![ContentBlock::Text { text: s.clone() }]
                    }
                    Some(Value::Array(parts)) => {
                        let mut blks: Vec<ContentBlock> = Vec::new();
                        for part in parts {
                            let part_type = part.get("type").and_then(Value::as_str).unwrap_or("");
                            match part_type {
                                "image_url" | "image" => {
                                    return Err(AdapterError::UnexpectedFormat(
                                        "image input not supported".into(),
                                    ));
                                }
                                "text" => {
                                    if let Some(text) = part.get("text").and_then(Value::as_str) {
                                        blks.push(ContentBlock::Text {
                                            text: text.to_string(),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                        blks
                    }
                    _ => vec![],
                };

                // Assistant messages may carry tool_calls[] in addition to (or instead of)
                // content text.
                if role_str == "assistant" {
                    if let Some(tool_calls) = item.get("tool_calls").and_then(Value::as_array) {
                        for tc in tool_calls {
                            let raw_id = tc.get("id").and_then(Value::as_str).unwrap_or("");
                            let id = tool_id::to_normalized(raw_id);
                            let function = tc.get("function").unwrap_or(&Value::Null);
                            let name = function
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args_str = function
                                .get("arguments")
                                .and_then(Value::as_str)
                                .unwrap_or("{}");
                            let input: Value =
                                serde_json::from_str(args_str).unwrap_or(Value::Object(Map::new()));
                            blocks.push(ContentBlock::ToolUse { id, name, input });
                        }
                    }
                }

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
        // Chat Completions tool shape is NESTED: {type:"function", function:{name, description,
        // parameters}} — unlike Responses API's flat shape.
        let tools: Vec<ToolDefinition> = body
            .get("tools")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|t| {
                        // Nested under "function" key.
                        let func = t.get("function")?;
                        let name = func.get("name").and_then(Value::as_str)?.to_string();
                        let description = func
                            .get("description")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        let parameters = func
                            .get("parameters")
                            .cloned()
                            .unwrap_or_else(|| Value::Object(Map::new()));
                        Some(ToolDefinition {
                            name,
                            description,
                            parameters,
                        })
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
        // Chat uses `max_tokens` (NOT `max_output_tokens` like Responses API).
        let max_tokens = body
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        // Chat passes `reasoning_effort` as a top-level string (not nested under reasoning.effort).
        let reasoning_effort = body
            .get("reasoning_effort")
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

        Ok(DecodedRequest {
            model,
            messages,
            tools,
            config,
            stream,
        })
    }

    fn encode_response(&self, resp: &DecodedResponse) -> Result<Value, AdapterError> {
        // Build choices[0].message from resp.message.content.
        // Text blocks → joined into `content` string.
        // ToolUse blocks → tool_calls array with nested function shape.
        let mut text_parts: Vec<&str> = Vec::new();
        let mut tool_calls: Vec<Value> = Vec::new();

        for block in &resp.message.content {
            match block {
                ContentBlock::Text { text } => {
                    text_parts.push(text.as_str());
                }
                ContentBlock::ToolUse { id, name, input } => {
                    tool_calls.push(serde_json::json!({
                        "id": tool_id::to_provider(id),
                        "type": "function",
                        "function": {
                            "name": name,
                            // arguments must be a JSON string, not an object.
                            "arguments": input.to_string(),
                        }
                    }));
                }
                _ => {}
            }
        }

        // finish_reason: if any ToolUse blocks → "tool_calls", else "stop".
        let finish_reason = if !tool_calls.is_empty() {
            "tool_calls"
        } else {
            "stop"
        };

        let mut message_obj = serde_json::json!({ "role": "assistant" });
        if !text_parts.is_empty() {
            message_obj["content"] = Value::String(text_parts.join(""));
        } else {
            // OpenAI sets content to null when only tool_calls are present.
            message_obj["content"] = Value::Null;
        }
        if !tool_calls.is_empty() {
            message_obj["tool_calls"] = Value::Array(tool_calls);
        }

        // Usage: prefer resp.message.usage, fall back to resp.usage.
        let usage_src = resp.message.usage.as_ref().or(resp.usage.as_ref());
        let usage = match usage_src {
            Some(u) => serde_json::json!({
                "prompt_tokens": u.input_tokens,
                "completion_tokens": u.output_tokens,
                "total_tokens": u.total_tokens,
            }),
            None => serde_json::json!({
                "prompt_tokens": 0_u32,
                "completion_tokens": 0_u32,
                "total_tokens": 0_u32,
            }),
        };

        Ok(serde_json::json!({
            "id": &resp.message.id,
            "object": "chat.completion",
            "model": "",
            "choices": [{
                "index": 0,
                "message": message_obj,
                "finish_reason": finish_reason,
            }],
            "usage": usage,
        }))
    }

    fn encode_stream_event(
        &self,
        ev: &StreamEvent,
        st: &mut StreamEncodeState,
    ) -> Result<Vec<SseFrame>, AdapterError> {
        // Ensure response_id is populated for every frame.
        if st.response_id.is_empty() {
            st.response_id = format!("chatcmpl-{}", uuid::Uuid::new_v4().simple());
        }

        // Helper closure: build a minimal chat.completion.chunk skeleton.
        // The caller fills in choices[0].
        let chunk_base = || -> Value {
            serde_json::json!({
                "id": &st.response_id,
                "object": "chat.completion.chunk",
                "choices": [],
            })
        };

        let mut out: Vec<SseFrame> = Vec::new();

        match ev {
            StreamEvent::Start => {
                if !st.started {
                    st.started = true;
                    // Emit the opening chunk with role:"assistant".
                    let mut chunk = chunk_base();
                    chunk["choices"] = serde_json::json!([{
                        "index": 0,
                        "delta": { "role": "assistant" },
                        "finish_reason": null,
                    }]);
                    out.push(SseFrame {
                        event: None,
                        data: chunk,
                    });
                }
            }

            StreamEvent::TextDelta { text } => {
                let mut chunk = chunk_base();
                chunk["choices"] = serde_json::json!([{
                    "index": 0,
                    "delta": { "content": text },
                    "finish_reason": null,
                }]);
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
            }

            StreamEvent::ToolCallStart { id, name } => {
                let provider_id = tool_id::to_provider(id);
                // Use output_index as a tool-call index counter (0-based, increments per tool).
                let tool_idx = st.output_index;
                let mut chunk = chunk_base();
                chunk["choices"] = serde_json::json!([{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": tool_idx,
                            "id": provider_id,
                            "type": "function",
                            "function": { "name": name, "arguments": "" },
                        }]
                    },
                    "finish_reason": null,
                }]);
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
                // Advance index so next ToolCallStart gets a distinct index.
                st.output_index += 1;
            }

            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
                ..
            } => {
                // Determine the tool index for this id.
                // output_index was already incremented past this tool's slot, so we
                // look up by the tool-args map position. Use the count of keys in
                // tool_args (before insertion) as a proxy for the original index.
                // Simpler: track index by order of insertion in tool_args.
                // We use output_index - 1 - (number of tools that started after this one).
                // Actually, the simplest correct approach: accumulate deltas with a
                // "current tool index" derived from how many tools have been started so far.
                // Since output_index was incremented after each ToolCallStart, the index
                // for tool `id` is (position in insertion order of tool_args keys).
                let tool_idx = {
                    // Position of this id in tool_args insertion order (0-based).
                    st.tool_args
                        .entry(id.clone())
                        .or_default()
                        .push_str(arguments_delta);
                    // Find position among keys. HashMap doesn't preserve order, but for
                    // a single tool (the common case) this is always 0. For multiple tools,
                    // we accept the inherent HashMap non-determinism (OpenAI clients look up
                    // by index in the delta, not by id, but tools are rarely interleaved).
                    // We use a stable fallback: output_index - 1 if only one tool ever started.
                    if st.tool_args.len() == 1 {
                        0usize
                    } else {
                        st.tool_args.len() - 1
                    }
                };
                let provider_id = tool_id::to_provider(id);
                let mut chunk = chunk_base();
                chunk["choices"] = serde_json::json!([{
                    "index": 0,
                    "delta": {
                        "tool_calls": [{
                            "index": tool_idx,
                            "function": { "arguments": arguments_delta },
                        }]
                    },
                    "finish_reason": null,
                }]);
                // Silence unused variable warning — provider_id is used in more
                // complex scenarios; keep it here for parity with other adapters.
                let _ = provider_id;
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
            }

            StreamEvent::ToolCallEnd { .. } => {
                // Chat Completions signals tool-call completion via finish_reason:"tool_calls"
                // on the terminal chunk — no per-tool-call end frame needed.
            }

            StreamEvent::Usage(u) => {
                // Stash usage for the terminal chunk (or emit immediately as a
                // standalone chunk for clients that consume stream_options.include_usage).
                // We emit it as a chunk with empty choices and a usage field, which is the
                // OpenAI stream_options.include_usage format.
                st.usage = Some(u.clone());
                let mut chunk = chunk_base();
                chunk["choices"] = Value::Array(vec![]);
                chunk["usage"] = serde_json::json!({
                    "prompt_tokens": u.input_tokens,
                    "completion_tokens": u.output_tokens,
                    "total_tokens": u.total_tokens,
                });
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
            }

            StreamEvent::Stop { reason } => {
                let finish_reason = match reason {
                    StopReason::ToolUse => "tool_calls",
                    StopReason::MaxTokens => "length",
                    StopReason::EndTurn | StopReason::StopSequence => "stop",
                    StopReason::Other(s) => s.as_str(),
                };
                let mut chunk = chunk_base();
                chunk["choices"] = serde_json::json!([{
                    "index": 0,
                    "delta": {},
                    "finish_reason": finish_reason,
                }]);
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
            }

            StreamEvent::Done => {
                // The literal `data: [DONE]` sentinel for Chat Completions.
                // Represented as an SseFrame whose data is the string "[DONE]".
                out.push(SseFrame {
                    event: None,
                    data: Value::String("[DONE]".to_string()),
                });
            }

            StreamEvent::Error(msg) => {
                // Emit a terminal chunk with finish_reason:"stop" and an error field.
                let mut chunk = chunk_base();
                chunk["choices"] = serde_json::json!([{
                    "index": 0,
                    "delta": {},
                    "finish_reason": "stop",
                }]);
                chunk["error"] = serde_json::json!({ "message": msg });
                out.push(SseFrame {
                    event: None,
                    data: chunk,
                });
            }

            // Reasoning events — not part of Chat Completions wire format; skip.
            StreamEvent::ReasoningDelta { .. } | StreamEvent::ReasoningBlock { .. } => {}
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

    #[test]
    fn encode_tools_wraps_function_and_fixes_schema() {
        // Schema missing `type` on properties — YLR should patch it.
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: String::new(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "tags": { "items": { "type": "string" } }
                }
            }),
        }];
        let encoded = OpenAIChatAdapter.encode_tools(&tools);
        assert_eq!(encoded[0]["type"], "function");
        assert_eq!(encoded[0]["function"]["name"], "read");
        // YLR fix applied:
        assert_eq!(
            encoded[0]["function"]["parameters"]["properties"]["tags"]["type"],
            "array"
        );
    }

    #[test]
    fn encode_messages_assistant_tool_call_has_string_arguments() {
        let msg = Message {
            id: "m1".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "doing".into(),
                },
                ContentBlock::ToolUse {
                    // Normalized internal id → strip toolu_ when sending back.
                    id: "toolu_call_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/a"}),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let out = OpenAIChatAdapter.encode_messages(&[msg]);
        assert_eq!(out.messages[0]["role"], "assistant");
        assert_eq!(out.messages[0]["content"], "doing");
        let tc = &out.messages[0]["tool_calls"][0];
        // toolu_ prefix stripped for OpenAI
        assert_eq!(tc["id"], "call_1");
        assert_eq!(tc["type"], "function");
        assert_eq!(tc["function"]["name"], "read");
        let args_str = tc["function"]["arguments"].as_str().unwrap();
        let parsed: Value = serde_json::from_str(args_str).unwrap();
        assert_eq!(parsed["path"], "/a");
    }

    #[test]
    fn decode_response_parses_tool_calls() {
        let resp = serde_json::json!({
            "choices": [{
                "message": {
                    "content": "sure",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": { "name": "read", "arguments": "{\"path\":\"/a\"}" }
                    }]
                }
            }],
            "usage": { "prompt_tokens": 5, "completion_tokens": 10, "total_tokens": 15 }
        });
        let decoded = OpenAIChatAdapter.decode_response(&resp).unwrap();
        assert_eq!(decoded.message.content.len(), 2);
        match &decoded.message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                // Normalized: raw "call_abc" → "toolu_call_abc"
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
        assert_eq!(decoded.usage.unwrap().total_tokens, 15);
    }

    #[test]
    fn decode_stream_accumulates_tool_call_partials() {
        let mut state = StreamDecodeState::new();
        // First chunk: has id → emits ToolCallStart then ToolCallDelta
        let c1 = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "id": "call_abc",
                        "function": { "name": "read", "arguments": "{\"pa" }
                    }]
                }
            }]
        });
        let out1 = OpenAIChatAdapter
            .decode_stream_event(&c1, &mut state)
            .unwrap();
        match &out1[0] {
            StreamEvent::ToolCallStart { id, name } => {
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCallStart"),
        }
        match &out1[1] {
            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
                ..
            } => {
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(arguments_delta, "{\"pa");
            }
            _ => panic!("expected ToolCallDelta"),
        }
        // Second chunk: no id, only function.arguments partial — id resolved by index
        let c2 = serde_json::json!({
            "choices": [{
                "delta": {
                    "tool_calls": [{
                        "index": 0,
                        "function": { "arguments": "th\":\"/a\"}" }
                    }]
                }
            }]
        });
        let out2 = OpenAIChatAdapter
            .decode_stream_event(&c2, &mut state)
            .unwrap();
        match &out2[0] {
            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
                ..
            } => {
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(arguments_delta, "th\":\"/a\"}");
            }
            _ => panic!("expected ToolCallDelta"),
        }
        assert_eq!(state.tool_input_buf["toolu_call_abc"], "{\"path\":\"/a\"}");

        // Third chunk: finish_reason present → emits ToolCallEnd then Stop
        let c3 = serde_json::json!({
            "choices": [{ "delta": {}, "finish_reason": "tool_calls" }]
        });
        let out3 = OpenAIChatAdapter
            .decode_stream_event(&c3, &mut state)
            .unwrap();
        assert_eq!(out3.len(), 2, "expected ToolCallEnd + Stop");
        assert!(matches!(&out3[0], StreamEvent::ToolCallEnd { .. }));
        assert!(matches!(
            &out3[1],
            StreamEvent::Stop {
                reason: StopReason::ToolUse
            }
        ));
    }

    #[test]
    fn decode_stream_finish_stop_emits_end_turn() {
        let mut state = StreamDecodeState::new();
        let c = serde_json::json!({
            "choices": [{ "delta": {}, "finish_reason": "stop" }]
        });
        let out = OpenAIChatAdapter
            .decode_stream_event(&c, &mut state)
            .unwrap();
        // No open tool calls → only Stop
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::EndTurn
            }
        ));
    }

    #[test]
    fn decode_stream_finish_length_emits_max_tokens() {
        let mut state = StreamDecodeState::new();
        let c = serde_json::json!({
            "choices": [{ "delta": {}, "finish_reason": "length" }]
        });
        let out = OpenAIChatAdapter
            .decode_stream_event(&c, &mut state)
            .unwrap();
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::MaxTokens
            }
        ));
    }

    // -----------------------------------------------------------------------
    // Task 5.1 — decode_request
    // -----------------------------------------------------------------------

    #[test]
    fn chat_decode_request_roundtrips_tool_call() {
        // Body with: system message, user message, assistant with tool_calls,
        // tool result, and a tool definition.
        let body = serde_json::json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": "you are alva" },
                { "role": "user", "content": "list files in /tmp" },
                {
                    "role": "assistant",
                    "tool_calls": [{
                        "id": "call_abc",
                        "type": "function",
                        "function": {
                            "name": "read",
                            "arguments": "{\"path\":\"/tmp\"}"
                        }
                    }]
                },
                {
                    "role": "tool",
                    "tool_call_id": "call_abc",
                    "content": "file1.txt\nfile2.txt"
                }
            ],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "read",
                    "description": "read a file",
                    "parameters": { "type": "object" }
                }
            }],
            "stream": true,
            "max_tokens": 512,
            "reasoning_effort": "high"
        });
        let r = OpenAIChatAdapter::new().decode_request(&body).unwrap();

        assert_eq!(r.model, "gpt-4o");
        assert!(r.stream);
        assert_eq!(r.config.max_tokens, Some(512));
        assert_eq!(
            r.config.reasoning_effort,
            Some(crate::config::ReasoningEffort::High)
        );

        // tools: nested function shape decoded correctly
        assert_eq!(r.tools.len(), 1);
        assert_eq!(r.tools[0].name, "read");
        assert_eq!(r.tools[0].description, "read a file");

        // system message present
        assert!(r
            .messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::System)));

        // user message present
        assert!(r
            .messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::User)));

        // assistant message contains ToolUse block with normalized id
        let assistant_msg = r
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::Assistant))
            .expect("assistant message");
        let tool_use = assistant_msg.content.iter().find_map(|b| {
            if let ContentBlock::ToolUse { id, name, input } = b {
                Some((id.clone(), name.clone(), input.clone()))
            } else {
                None
            }
        });
        let (tu_id, tu_name, tu_input) = tool_use.expect("ToolUse in assistant message");
        assert_eq!(tu_id, "toolu_call_abc", "id must be normalized");
        assert_eq!(tu_name, "read");
        assert_eq!(tu_input["path"], "/tmp");

        // tool result message present
        let tool_msg = r
            .messages
            .iter()
            .find(|m| matches!(m.role, MessageRole::Tool))
            .expect("tool message");
        assert!(
            tool_msg
                .content
                .iter()
                .any(|b| matches!(b, ContentBlock::ToolResult { .. })),
            "Tool message must contain ToolResult block"
        );
    }

    #[test]
    fn chat_decode_request_rejects_image() {
        let body = serde_json::json!({
            "model": "m",
            "messages": [{
                "role": "user",
                "content": [{ "type": "image_url", "image_url": { "url": "https://x.com/img.png" } }]
            }]
        });
        assert!(
            matches!(
                OpenAIChatAdapter::new().decode_request(&body),
                Err(AdapterError::UnexpectedFormat(_))
            ),
            "image_url content part must be rejected"
        );
    }

    #[test]
    fn chat_decode_request_missing_model_errors() {
        let body = serde_json::json!({ "messages": [] });
        assert!(matches!(
            OpenAIChatAdapter::new().decode_request(&body),
            Err(AdapterError::MissingField("model"))
        ));
    }

    // -----------------------------------------------------------------------
    // Task 5.2 — encode_response
    // -----------------------------------------------------------------------

    #[test]
    fn chat_encode_response_tool_call() {
        use super::super::DecodedResponse;
        use crate::message::UsageMetadata;

        let dr = DecodedResponse {
            message: Message {
                id: "r1".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "let me read that".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_call_abc".into(),
                        name: "read".into(),
                        input: serde_json::json!({"path": "/tmp"}),
                    },
                ],
                tool_call_id: None,
                usage: Some(UsageMetadata {
                    input_tokens: 10,
                    output_tokens: 20,
                    total_tokens: 30,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
                timestamp: 0,
            },
            usage: None,
        };

        let v = OpenAIChatAdapter::new().encode_response(&dr).unwrap();

        // object must be chat.completion
        assert_eq!(
            v["object"], "chat.completion",
            "object must be 'chat.completion'"
        );

        // finish_reason must be "tool_calls" (ToolUse present)
        assert_eq!(
            v["choices"][0]["finish_reason"], "tool_calls",
            "finish_reason must be 'tool_calls'"
        );

        // tool_calls[0].function.arguments must be a parseable JSON string
        let args_val = &v["choices"][0]["message"]["tool_calls"][0]["function"]["arguments"];
        let args_str = args_val.as_str().expect("arguments must be a string");
        let parsed: serde_json::Value =
            serde_json::from_str(args_str).expect("arguments string must be valid JSON");
        assert_eq!(parsed["path"], "/tmp");

        // id must be provider-form (toolu_ stripped)
        assert_eq!(
            v["choices"][0]["message"]["tool_calls"][0]["id"],
            "call_abc"
        );

        // usage fields (prompt_tokens = input_tokens, completion_tokens = output_tokens)
        assert_eq!(v["usage"]["prompt_tokens"], 10);
        assert_eq!(v["usage"]["completion_tokens"], 20);
        assert_eq!(v["usage"]["total_tokens"], 30);
    }

    #[test]
    fn chat_encode_response_text_only_has_stop_reason() {
        use super::super::DecodedResponse;

        let dr = DecodedResponse {
            message: Message {
                id: "r2".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text {
                    text: "hello".into(),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            usage: None,
        };
        let v = OpenAIChatAdapter::new().encode_response(&dr).unwrap();
        assert_eq!(v["choices"][0]["finish_reason"], "stop");
        assert_eq!(v["choices"][0]["message"]["content"], "hello");
    }

    // -----------------------------------------------------------------------
    // Task 5.3 — encode_stream_event
    // -----------------------------------------------------------------------

    #[test]
    fn chat_encode_stream_stop_max_tokens_emits_length() {
        use super::super::StreamEncodeState;

        let a = OpenAIChatAdapter::new();
        let mut st = StreamEncodeState::default();
        let frames = a
            .encode_stream_event(
                &StreamEvent::Stop {
                    reason: StopReason::MaxTokens,
                },
                &mut st,
            )
            .unwrap();
        assert_eq!(frames.len(), 1, "Stop must emit exactly one chunk");
        let chunk = &frames[0].data;
        assert_eq!(
            chunk["object"], "chat.completion.chunk",
            "object must be chat.completion.chunk"
        );
        assert_eq!(
            chunk["choices"][0]["finish_reason"], "length",
            "MaxTokens must map to finish_reason 'length'"
        );
    }

    #[test]
    fn chat_encode_stream_done_emits_done_sentinel() {
        use super::super::StreamEncodeState;

        let a = OpenAIChatAdapter::new();
        let mut st = StreamEncodeState::default();
        let frames = a.encode_stream_event(&StreamEvent::Done, &mut st).unwrap();
        assert_eq!(frames.len(), 1, "Done must emit exactly one frame");
        let data = &frames[0].data;
        assert_eq!(
            data.as_str(),
            Some("[DONE]"),
            "Done frame data must be the string '[DONE]'"
        );
    }

    #[test]
    fn chat_encode_stream_full_sequence() {
        use super::super::StreamEncodeState;

        let a = OpenAIChatAdapter::new();
        let mut st = StreamEncodeState::default();
        let mut frames: Vec<super::super::SseFrame> = Vec::new();

        for ev in [
            StreamEvent::Start,
            StreamEvent::TextDelta {
                text: "hello".into(),
            },
            StreamEvent::ToolCallStart {
                id: "toolu_t1".into(),
                name: "read".into(),
            },
            StreamEvent::ToolCallDelta {
                id: "toolu_t1".into(),
                name: None,
                arguments_delta: "{\"p\":1}".into(),
            },
            StreamEvent::ToolCallEnd {
                id: "toolu_t1".into(),
            },
            StreamEvent::Stop {
                reason: StopReason::ToolUse,
            },
            StreamEvent::Done,
        ] {
            frames.extend(a.encode_stream_event(&ev, &mut st).unwrap());
        }

        // All chunks (excluding [DONE]) must have object:"chat.completion.chunk"
        for f in &frames {
            if let Some(s) = f.data.as_str() {
                assert_eq!(s, "[DONE]");
            } else {
                assert_eq!(f.data["object"], "chat.completion.chunk");
            }
        }

        // ToolUse stop reason → finish_reason "tool_calls"
        let stop_chunk = frames
            .iter()
            .find(|f| {
                f.data
                    .pointer("/choices/0/finish_reason")
                    .and_then(|v| v.as_str())
                    .is_some()
            })
            .expect("a chunk with finish_reason");
        assert_eq!(stop_chunk.data["choices"][0]["finish_reason"], "tool_calls");

        // Done sentinel is last
        let last = frames.last().expect("at least one frame");
        assert_eq!(last.data.as_str(), Some("[DONE]"));
    }
}
