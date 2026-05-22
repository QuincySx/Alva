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
    SseFrame, StreamDecodeState, StreamEncodeState,
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

    fn encode_response(&self, resp: &DecodedResponse) -> Result<Value, AdapterError> {
        // Collect all text blocks into one message item; each ToolUse becomes
        // a separate function_call item — mirroring decode_response's parsing.
        let mut text_parts: Vec<Value> = Vec::new();
        let mut output: Vec<Value> = Vec::new();

        for block in &resp.message.content {
            match block {
                ContentBlock::Text { text } => {
                    text_parts.push(serde_json::json!({
                        "type": "output_text",
                        "text": text,
                    }));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    output.push(serde_json::json!({
                        "type": "function_call",
                        "call_id": tool_id::to_provider(id),
                        "name": name,
                        "arguments": input.to_string(),
                    }));
                }
                // Reasoning: skip for v1 (no Responses API reasoning output_item yet).
                // ToolResult / Image: not expected in an assistant response; skip.
                _ => {}
            }
        }

        // Prepend the message item (containing all text parts) if there is any text.
        if !text_parts.is_empty() {
            output.insert(
                0,
                serde_json::json!({
                    "type": "message",
                    "role": "assistant",
                    "content": text_parts,
                }),
            );
        }

        // Usage: prefer resp.message.usage, fall back to resp.usage, emit zeros if absent.
        let usage_src = resp.message.usage.as_ref().or(resp.usage.as_ref());
        let usage = match usage_src {
            Some(u) => serde_json::json!({
                "input_tokens": u.input_tokens,
                "output_tokens": u.output_tokens,
                "total_tokens": u.total_tokens,
            }),
            None => serde_json::json!({
                "input_tokens": 0_u32,
                "output_tokens": 0_u32,
                "total_tokens": 0_u32,
            }),
        };

        Ok(serde_json::json!({
            "id": &resp.message.id,
            "object": "response",
            "status": "completed",
            "model": "",
            "output": output,
            "usage": usage,
        }))
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
                // Mirror response.completed: emit Usage (if present), Stop, then Done
                // so consumers waiting for Done are not left hanging.
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
                out.push(StreamEvent::Stop { reason: StopReason::MaxTokens });
                out.push(StreamEvent::Done);
            }
            "response.failed" => {
                out.push(StreamEvent::Stop { reason: StopReason::Other("failed".to_string()) });
            }
            _ => {}
        }
        Ok(out)
    }

    fn encode_stream_event(
        &self,
        ev: &StreamEvent,
        st: &mut StreamEncodeState,
    ) -> Result<Vec<SseFrame>, AdapterError> {
        // Helper: emit one named SSE frame, bumping st.seq each time so
        // sequence_number is strictly monotonic across all frames in a stream.
        macro_rules! frame {
            ($event_name:expr, $data:expr) => {{
                let seq = st.seq;
                st.seq += 1;
                let mut data: serde_json::Value = $data;
                // Inject sequence_number into every frame's data object.
                if let Some(obj) = data.as_object_mut() {
                    obj.insert("sequence_number".to_string(), serde_json::Value::from(seq));
                }
                SseFrame {
                    event: Some($event_name.to_string()),
                    data,
                }
            }};
        }

        let mut out: Vec<SseFrame> = Vec::new();

        match ev {
            StreamEvent::Start => {
                if !st.started {
                    st.started = true;
                    st.response_id = format!("resp_{}", uuid::Uuid::new_v4());
                    out.push(frame!(
                        "response.created",
                        serde_json::json!({
                            "type": "response.created",
                            "response": {
                                "id": &st.response_id,
                                "object": "response",
                                "status": "in_progress",
                            }
                        })
                    ));
                }
            }

            StreamEvent::TextDelta { text } => {
                // FIX 3: open a new text/message item if none is currently open.
                if !st.text_item_open {
                    st.text_item_index = st.output_index;
                    st.text_item_open = true;
                }
                out.push(frame!(
                    "response.output_text.delta",
                    serde_json::json!({
                        "type": "response.output_text.delta",
                        "output_index": st.text_item_index,
                        "content_index": 0,
                        "delta": text,
                    })
                ));
            }

            StreamEvent::ReasoningDelta { text } => {
                // Responses API uses `response.reasoning_summary_text.delta` for
                // streaming reasoning summaries. Emit it so callers that care can
                // forward it; consumers that don't know this event name will ignore it.
                out.push(frame!(
                    "response.reasoning_summary_text.delta",
                    serde_json::json!({
                        "type": "response.reasoning_summary_text.delta",
                        "output_index": st.output_index,
                        "delta": text,
                    })
                ));
            }

            StreamEvent::ReasoningBlock { .. } => {
                // Completed reasoning block — no direct Responses API equivalent
                // for a "done" reasoning event in streaming; skip silently.
            }

            StreamEvent::ToolCallStart { id, name } => {
                let provider_id = tool_id::to_provider(id);
                // FIX 3: if a text item was open, close it and advance output_index
                // so the tool item gets a distinct index.
                if st.text_item_open {
                    st.text_item_open = false;
                    st.output_index += 1;
                }
                let tool_index = st.output_index;
                out.push(frame!(
                    "response.output_item.added",
                    serde_json::json!({
                        "type": "response.output_item.added",
                        "output_index": tool_index,
                        "item": {
                            "type": "function_call",
                            "call_id": &provider_id,
                            "name": name,
                            "arguments": "",
                        }
                    })
                ));
                // Advance past the tool item so any subsequent text gets a fresh index.
                st.output_index += 1;
            }

            StreamEvent::ToolCallDelta { id, name, arguments_delta } => {
                let provider_id = tool_id::to_provider(id);
                // FIX 4: accumulate argument fragments for this tool call.
                st.tool_args
                    .entry(id.clone())
                    .or_default()
                    .push_str(arguments_delta);
                out.push(frame!(
                    "response.function_call_arguments.delta",
                    serde_json::json!({
                        "type": "response.function_call_arguments.delta",
                        "call_id": &provider_id,
                        "name": name,
                        "delta": arguments_delta,
                    })
                ));
            }

            StreamEvent::ToolCallEnd { id } => {
                let provider_id = tool_id::to_provider(id);
                // FIX 4: read the accumulated arguments and clear the buffer.
                let accumulated_args = st.tool_args.remove(id).unwrap_or_default();
                out.push(frame!(
                    "response.function_call_arguments.done",
                    serde_json::json!({
                        "type": "response.function_call_arguments.done",
                        "call_id": &provider_id,
                        "arguments": &accumulated_args,
                    })
                ));
                out.push(frame!(
                    "response.output_item.done",
                    serde_json::json!({
                        "type": "response.output_item.done",
                        "item": {
                            "type": "function_call",
                            "call_id": &provider_id,
                            "arguments": &accumulated_args,
                        }
                    })
                ));
            }

            StreamEvent::Usage(u) => {
                // Stash usage so it can be embedded in the response.completed frame.
                // The OpenAI Responses API carries usage inside the completed event's
                // response object rather than as a standalone event.
                st.usage = Some(u.clone());
                // Return empty — no frame emitted here.
            }

            StreamEvent::Stop { reason } => {
                // FIX 5: ensure response_id is never empty (Start may have been skipped).
                if st.response_id.is_empty() {
                    st.response_id = format!("resp_{}", uuid::Uuid::new_v4());
                }

                match reason {
                    StopReason::Other(msg) => {
                        // FIX 1: StopReason::Other represents an upstream failure.
                        // Emit response.failed with the error message.
                        out.push(frame!(
                            "response.failed",
                            serde_json::json!({
                                "type": "response.failed",
                                "response": {
                                    "id": &st.response_id,
                                    "object": "response",
                                    "status": "failed",
                                    "error": {
                                        "message": msg,
                                    }
                                }
                            })
                        ));
                    }
                    _ => {
                        let (event_name, status) = match reason {
                            StopReason::EndTurn
                            | StopReason::ToolUse
                            | StopReason::StopSequence => ("response.completed", "completed"),
                            StopReason::MaxTokens => ("response.incomplete", "incomplete"),
                            // Other is handled above; this branch is unreachable.
                            StopReason::Other(_) => unreachable!(),
                        };

                        let usage_val = match &st.usage {
                            Some(u) => serde_json::json!({
                                "input_tokens": u.input_tokens,
                                "output_tokens": u.output_tokens,
                                "total_tokens": u.total_tokens,
                            }),
                            None => serde_json::json!({
                                "input_tokens": 0_u32,
                                "output_tokens": 0_u32,
                                "total_tokens": 0_u32,
                            }),
                        };

                        out.push(frame!(
                            event_name,
                            serde_json::json!({
                                "type": event_name,
                                "response": {
                                    "id": &st.response_id,
                                    "object": "response",
                                    "status": status,
                                    "usage": usage_val,
                                }
                            })
                        ));
                    }
                }
            }

            StreamEvent::Done => {
                // No frame emitted — the SSE connection is closed by the transport layer.
            }

            StreamEvent::Error(msg) => {
                out.push(frame!(
                    "response.failed",
                    serde_json::json!({
                        "type": "response.failed",
                        "error": {
                            "message": msg,
                        }
                    })
                ));
            }
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
        // FIX 2: response.incomplete now emits Stop{MaxTokens} + Done (was: Stop only).
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.incomplete".into());
        let ev = serde_json::json!({});
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 2, "expected Stop + Done (len 2), got: {out:?}");
        assert!(matches!(&out[0], StreamEvent::Stop { reason: StopReason::MaxTokens }));
        assert!(matches!(out[1], StreamEvent::Done));
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

    #[test]
    fn responses_encode_stream_text_then_stop() {
        use crate::stream::{StreamEvent, StopReason};
        use crate::message::UsageMetadata;
        use super::super::StreamEncodeState;
        let a = OpenAIResponsesAdapter::new();
        let mut st = StreamEncodeState::default();
        let mut frames = vec![];
        for ev in [
            StreamEvent::Start,
            StreamEvent::TextDelta { text: "hi".into() },
            StreamEvent::Usage(UsageMetadata { input_tokens:1, output_tokens:1, total_tokens:2, cache_creation_input_tokens:None, cache_read_input_tokens:None }),
            StreamEvent::Stop { reason: StopReason::EndTurn },
            StreamEvent::Done,
        ] {
            frames.extend(a.encode_stream_event(&ev, &mut st).unwrap());
        }
        let names: Vec<_> = frames.iter().filter_map(|f| f.event.as_deref()).collect();
        assert!(names.contains(&"response.created"));
        assert!(names.contains(&"response.output_text.delta"));
        assert!(names.contains(&"response.completed"));
        let seqs: Vec<i64> = frames.iter().filter_map(|f| f.data.get("sequence_number").and_then(|v| v.as_i64())).collect();
        assert!(seqs.windows(2).all(|w| w[0] < w[1]), "sequence_number must be monotonic: {seqs:?}");
    }

    #[test]
    fn responses_encode_response_text_and_tool() {
        use crate::content::ContentBlock;
        use crate::message::{Message, MessageRole, UsageMetadata};
        use super::DecodedResponse;
        let dr = DecodedResponse {
            message: Message {
                id: "r1".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text { text: "hello".into() },
                    ContentBlock::ToolUse {
                        id: "toolu_a".into(),
                        name: "read".into(),
                        input: serde_json::json!({"p": "/x"}),
                    },
                ],
                tool_call_id: None,
                usage: Some(UsageMetadata {
                    input_tokens: 3,
                    output_tokens: 4,
                    total_tokens: 7,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
                timestamp: 0,
            },
            usage: None,
        };
        let v = OpenAIResponsesAdapter::new().encode_response(&dr).unwrap();
        assert_eq!(v["object"], "response");
        assert_eq!(v["status"], "completed");
        let outs = v["output"].as_array().unwrap();
        assert!(outs.iter().any(|o| o["type"] == "function_call" && o["name"] == "read"));
        assert!(outs.iter().any(|o| o["type"] == "message"));
        assert_eq!(v["usage"]["input_tokens"], 3);
    }

    // -----------------------------------------------------------------------
    // FIX 1: StopReason::Other must produce response.failed, not response.completed
    // -----------------------------------------------------------------------

    #[test]
    fn encode_stop_other_emits_response_failed_not_completed() {
        use crate::stream::{StreamEvent, StopReason};
        use super::super::StreamEncodeState;
        let a = OpenAIResponsesAdapter::new();
        let mut st = StreamEncodeState::default();
        // Feed a Start first so response_id is set.
        a.encode_stream_event(&StreamEvent::Start, &mut st).unwrap();
        let frames = a
            .encode_stream_event(&StreamEvent::Stop { reason: StopReason::Other("boom".into()) }, &mut st)
            .unwrap();
        let event_names: Vec<_> = frames.iter().filter_map(|f| f.event.as_deref()).collect();
        assert!(
            event_names.contains(&"response.failed"),
            "expected response.failed frame, got: {event_names:?}"
        );
        assert!(
            !event_names.contains(&"response.completed"),
            "must NOT emit response.completed for Other stop, got: {event_names:?}"
        );
        // The failed frame must carry the error message.
        let failed_frame = frames.iter().find(|f| f.event.as_deref() == Some("response.failed")).unwrap();
        let error_msg = failed_frame
            .data
            .pointer("/response/error/message")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        assert_eq!(error_msg, "boom", "error.message must carry the Other reason string");
    }

    // -----------------------------------------------------------------------
    // FIX 2: response.incomplete must also emit StreamEvent::Done
    // -----------------------------------------------------------------------

    #[test]
    fn decode_stream_incomplete_emits_stop_and_done() {
        let mut state = StreamDecodeState::new();
        state.event_type = Some("response.incomplete".into());
        // Include usage to verify it is extracted too (mirrors response.completed behaviour).
        let ev = serde_json::json!({
            "response": { "usage": { "input_tokens": 5, "output_tokens": 10, "total_tokens": 15 } }
        });
        let out = OpenAIResponsesAdapter.decode_stream_event(&ev, &mut state).unwrap();
        // Expect: [Usage, Stop{MaxTokens}, Done]
        assert_eq!(out.len(), 3, "expected Usage + Stop + Done (len 3), got: {out:?}");
        assert!(matches!(out[0], StreamEvent::Usage(_)));
        assert!(
            matches!(&out[1], StreamEvent::Stop { reason: StopReason::MaxTokens }),
            "second event must be Stop{{MaxTokens}}, got: {:?}", out[1]
        );
        assert!(matches!(out[2], StreamEvent::Done), "last event must be Done");
    }

    // -----------------------------------------------------------------------
    // FIX 3: output_index is distinct for text and tool items (and non-decreasing)
    // -----------------------------------------------------------------------

    #[test]
    fn encode_interleaved_text_tool_text_output_indices_are_distinct() {
        use crate::stream::{StreamEvent, StopReason};
        use super::super::StreamEncodeState;

        let a = OpenAIResponsesAdapter::new();
        let mut st = StreamEncodeState::default();
        let mut all_frames: Vec<SseFrame> = Vec::new();

        // Sequence: Start → TextDelta("a") → ToolCallStart → ToolCallDelta → ToolCallEnd → TextDelta("b")
        for ev in [
            StreamEvent::Start,
            StreamEvent::TextDelta { text: "a".into() },
            StreamEvent::ToolCallStart { id: "t1".into(), name: "read".into() },
            StreamEvent::ToolCallDelta { id: "t1".into(), name: None, arguments_delta: "{\"p\":1}".into() },
            StreamEvent::ToolCallEnd { id: "t1".into() },
            StreamEvent::TextDelta { text: "b".into() },
        ] {
            all_frames.extend(a.encode_stream_event(&ev, &mut st).unwrap());
        }

        // Collect output_indices from text-delta frames and the tool output_item.added frame.
        let first_text_index = all_frames
            .iter()
            .find(|f| f.event.as_deref() == Some("response.output_text.delta")
                && f.data.get("delta").and_then(|v| v.as_str()) == Some("a"))
            .and_then(|f| f.data.get("output_index").and_then(|v| v.as_u64()))
            .expect("first text delta frame");

        let tool_index = all_frames
            .iter()
            .find(|f| f.event.as_deref() == Some("response.output_item.added"))
            .and_then(|f| f.data.get("output_index").and_then(|v| v.as_u64()))
            .expect("output_item.added frame");

        let second_text_index = all_frames
            .iter()
            .filter(|f| f.event.as_deref() == Some("response.output_text.delta"))
            .last()  // the last text delta is "b"
            .and_then(|f| f.data.get("output_index").and_then(|v| v.as_u64()))
            .expect("second text delta frame");

        assert_ne!(
            first_text_index, tool_index,
            "first text delta (index {first_text_index}) and tool item (index {tool_index}) must not share output_index"
        );
        assert_ne!(
            second_text_index, tool_index,
            "second text delta (index {second_text_index}) and tool item (index {tool_index}) must not share output_index"
        );
        // All indices must be non-decreasing across frames (not just these three).
        let indices: Vec<u64> = all_frames
            .iter()
            .filter_map(|f| f.data.get("output_index").and_then(|v| v.as_u64()))
            .collect();
        assert!(
            indices.windows(2).all(|w| w[0] <= w[1]),
            "output_index must be non-decreasing: {indices:?}"
        );

        // Also verify the existing monotonic sequence_number test still passes.
        let seqs: Vec<i64> = all_frames
            .iter()
            .filter_map(|f| f.data.get("sequence_number").and_then(|v| v.as_i64()))
            .collect();
        assert!(
            seqs.windows(2).all(|w| w[0] < w[1]),
            "sequence_number must be strictly monotonic: {seqs:?}"
        );
    }

    // -----------------------------------------------------------------------
    // FIX 4: accumulated arguments are emitted in response.output_item.done
    // -----------------------------------------------------------------------

    #[test]
    fn encode_tool_call_end_includes_accumulated_arguments() {
        use crate::stream::StreamEvent;
        use super::super::StreamEncodeState;

        let a = OpenAIResponsesAdapter::new();
        let mut st = StreamEncodeState::default();
        let mut all_frames: Vec<SseFrame> = Vec::new();

        for ev in [
            StreamEvent::ToolCallStart { id: "t1".into(), name: "read".into() },
            StreamEvent::ToolCallDelta {
                id: "t1".into(),
                name: None,
                arguments_delta: "{\"p\":".into(),
            },
            StreamEvent::ToolCallDelta {
                id: "t1".into(),
                name: None,
                arguments_delta: "1}".into(),
            },
            StreamEvent::ToolCallEnd { id: "t1".into() },
        ] {
            all_frames.extend(a.encode_stream_event(&ev, &mut st).unwrap());
        }

        let done_frame = all_frames
            .iter()
            .find(|f| f.event.as_deref() == Some("response.output_item.done"))
            .expect("response.output_item.done frame must be emitted");

        let args = done_frame
            .data
            .pointer("/item/arguments")
            .and_then(|v| v.as_str())
            .expect("item.arguments must be present in output_item.done");

        assert_eq!(args, "{\"p\":1}", "accumulated arguments must equal the concatenated deltas");
    }

    // -----------------------------------------------------------------------
    // FIX 5: response_id fallback when Start was skipped
    // -----------------------------------------------------------------------

    #[test]
    fn encode_stop_without_start_generates_nonempty_response_id() {
        use crate::stream::{StreamEvent, StopReason};
        use super::super::StreamEncodeState;

        let a = OpenAIResponsesAdapter::new();
        // Fresh state — no Start event fed, response_id is empty string.
        let mut st = StreamEncodeState::default();
        assert!(st.response_id.is_empty(), "precondition: response_id starts empty");

        let frames = a
            .encode_stream_event(&StreamEvent::Stop { reason: StopReason::EndTurn }, &mut st)
            .unwrap();

        let completed_frame = frames
            .iter()
            .find(|f| f.event.as_deref() == Some("response.completed"))
            .expect("response.completed frame must be emitted");

        let response_id = completed_frame
            .data
            .pointer("/response/id")
            .and_then(|v| v.as_str())
            .expect("response.id must be present in the completed frame");

        assert!(
            !response_id.is_empty(),
            "response.id must not be empty when Start was skipped"
        );
        assert!(
            response_id.starts_with("resp_"),
            "generated id must start with 'resp_', got: {response_id}"
        );
        // The state should also be updated so subsequent frames use the same id.
        assert_eq!(st.response_id, response_id, "st.response_id must match the emitted id");
    }
}
