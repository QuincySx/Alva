// INPUT:  super::{ToolAdapter, EncodedMessages, DecodedResponse, StreamDecodeState, AdapterError}
// OUTPUT: AnthropicAdapter (stateless unit struct impl ToolAdapter)
// POS:    Anthropic `messages.create` translation — encodes tools/messages, decodes responses/stream.

//! Anthropic Messages API adapter.
//!
//! Covers the shape of `/v1/messages` request/response bodies and the SSE
//! stream. Matches AMP's `Bx` (tool encode) + Anthropic-native response
//! parsing. Anthropic tolerates JSON Schema passthrough — no `YLR`-style
//! patching needed (cf. OpenAI Chat).

use serde_json::{Map, Value};

use super::{
    common::tool_id, AdapterError, DecodedRequest, DecodedResponse, EncodedMessages,
    ProtocolAdapter, SseFrame, StreamDecodeState, StreamEncodeState,
};
use crate::config::{ModelConfig, ReasoningEffort};
use crate::content::ContentBlock;
use crate::message::{Message, MessageRole, UsageMetadata};
use crate::stream::{StopReason, StreamEvent};
use crate::tool_def::ToolDefinition;

// ---------------------------------------------------------------------------
// AnthropicAdapter
// ---------------------------------------------------------------------------

/// Stateless adapter for Anthropic's Messages API (`/v1/messages`).
#[derive(Debug, Default, Clone, Copy)]
pub struct AnthropicAdapter;

impl AnthropicAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ProtocolAdapter for AnthropicAdapter {
    fn provider(&self) -> &'static str {
        "anthropic"
    }

    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        // AMP `Bx`: dedupe by name, schema passthrough.
        let mut seen = std::collections::HashSet::new();
        tools
            .iter()
            .filter(|t| seen.insert(t.name.clone()))
            .map(|t| {
                serde_json::json!({
                    "name": &t.name,
                    "description": &t.description,
                    "input_schema": t.parameters.clone(),
                })
            })
            .collect()
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        // Each System message becomes its own segment. The kernel sends
        // one System message per system_prompt segment in stable→
        // dynamic order, so we preserve that ordering 1:1 — Anthropic's
        // build_body then maps every segment except the last to a
        // `cache_control: ephemeral` block.
        let mut system_segments: Vec<String> = Vec::new();
        let mut api_messages: Vec<Value> = Vec::new();

        for m in messages {
            match m.role {
                MessageRole::System => {
                    let text = m.text_content();
                    if !text.is_empty() {
                        system_segments.push(text);
                    }
                }
                MessageRole::User => {
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": m.text_content(),
                    }));
                }
                MessageRole::Assistant => {
                    let mut blocks: Vec<Value> = Vec::new();
                    for b in &m.content {
                        match b {
                            crate::content::ContentBlock::Text { text } => {
                                blocks.push(serde_json::json!({"type": "text", "text": text}));
                            }
                            crate::content::ContentBlock::Reasoning { text, signature } => {
                                // Echo thinking blocks back verbatim. Anthropic's
                                // extended thinking mode requires the signature
                                // to be present and unchanged; without it the
                                // next turn fails 400. Blocks missing a signature
                                // (e.g. from other providers round-tripped into
                                // Anthropic history) are skipped to avoid
                                // invalid-request errors on the Anthropic side.
                                if let Some(sig) = signature {
                                    blocks.push(serde_json::json!({
                                        "type": "thinking",
                                        "thinking": text,
                                        "signature": sig,
                                    }));
                                }
                            }
                            crate::content::ContentBlock::ToolUse { id, name, input } => {
                                // Anthropic expects its own toolu_* ids; pass through normalized
                                // form (which is already toolu_*-prefixed after decode).
                                blocks.push(serde_json::json!({
                                    "type": "tool_use",
                                    "id": id,
                                    "name": name,
                                    "input": input,
                                }));
                            }
                            _ => {
                                if let Some(t) = b.as_text() {
                                    blocks.push(serde_json::json!({"type": "text", "text": t}));
                                }
                            }
                        }
                    }
                    if blocks.is_empty() {
                        let text = m.text_content();
                        if !text.is_empty() {
                            blocks.push(serde_json::json!({"type": "text", "text": text}));
                        }
                    }
                    api_messages.push(serde_json::json!({
                        "role": "assistant",
                        "content": blocks,
                    }));
                }
                MessageRole::Tool => {
                    // Anthropic encodes tool results as user-role messages
                    // with `tool_result` content blocks.
                    //
                    // CRITICAL: when an assistant turn returns multiple
                    // tool_uses, Alva's run loop appends each tool_result as
                    // its own `MessageRole::Tool` message. But Anthropic
                    // requires ALL tool_result blocks in the SINGLE user
                    // message immediately following the assistant turn — any
                    // tool_use whose result spills into a later message is
                    // rejected as "tool_use ids were found without
                    // tool_result blocks immediately after".
                    //
                    // So: when the previous api_message is already a user
                    // message carrying tool_result blocks, append into it
                    // instead of pushing a new one.
                    let mut blocks: Vec<Value> = Vec::new();
                    for b in &m.content {
                        if let crate::content::ContentBlock::ToolResult {
                            id,
                            content,
                            is_error,
                        } = b
                        {
                            let joined: String = content
                                .iter()
                                .map(|tc| tc.to_model_string())
                                .collect::<Vec<_>>()
                                .join("\n");
                            blocks.push(serde_json::json!({
                                "type": "tool_result",
                                "tool_use_id": id,
                                "content": joined,
                                "is_error": is_error,
                            }));
                        }
                    }
                    if blocks.is_empty() {
                        let text = m.text_content();
                        let id = m.tool_call_id.as_deref().unwrap_or("unknown");
                        blocks.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": text,
                        }));
                    }

                    let appended = api_messages
                        .last_mut()
                        .and_then(|prev| {
                            let prev_role = prev.get("role").and_then(Value::as_str)?;
                            if prev_role != "user" {
                                return None;
                            }
                            let prev_content = prev.get("content")?.as_array()?;
                            let all_tool_results = !prev_content.is_empty()
                                && prev_content.iter().all(|b| {
                                    b.get("type").and_then(Value::as_str) == Some("tool_result")
                                });
                            if !all_tool_results {
                                return None;
                            }
                            let arr = prev.get_mut("content")?.as_array_mut()?;
                            arr.extend(std::mem::take(&mut blocks));
                            Some(())
                        })
                        .is_some();

                    if !appended {
                        api_messages.push(serde_json::json!({
                            "role": "user",
                            "content": blocks,
                        }));
                    }
                }
            }
        }

        EncodedMessages {
            system_segments: if system_segments.is_empty() {
                None
            } else {
                Some(system_segments)
            },
            messages: api_messages,
        }
    }

    // -----------------------------------------------------------------------
    // Task 5.4 — decode_request
    // -----------------------------------------------------------------------

    fn decode_request(&self, body: &Value) -> Result<DecodedRequest, AdapterError> {
        // -- model (required) ------------------------------------------------
        let model = body
            .get("model")
            .and_then(Value::as_str)
            .ok_or(AdapterError::MissingField("model"))?
            .to_string();

        // -- system → prepend as System message ------------------------------
        let mut messages: Vec<Message> = Vec::new();

        if let Some(system_val) = body.get("system") {
            match system_val {
                Value::String(s) if !s.is_empty() => {
                    messages.push(Message::system(s));
                }
                Value::Array(arr) => {
                    // array of {type:"text", text:...} blocks — join their text
                    let joined: String = arr
                        .iter()
                        .filter(|b| b.get("type").and_then(Value::as_str) == Some("text"))
                        .filter_map(|b| b.get("text").and_then(Value::as_str))
                        .collect::<Vec<_>>()
                        .join("\n\n");
                    if !joined.is_empty() {
                        messages.push(Message::system(&joined));
                    }
                }
                _ => {}
            }
        }

        // -- messages[] ------------------------------------------------------
        if let Some(msg_arr) = body.get("messages").and_then(Value::as_array) {
            for m in msg_arr {
                let role_str = m.get("role").and_then(Value::as_str).unwrap_or("user");
                let role = match role_str {
                    "user" => MessageRole::User,
                    "assistant" => MessageRole::Assistant,
                    _ => MessageRole::User,
                };

                let content_val = m.get("content");
                let blocks: Vec<ContentBlock> = match content_val {
                    // content is a plain string → single Text block
                    Some(Value::String(s)) => {
                        vec![ContentBlock::Text { text: s.clone() }]
                    }
                    // content is an array of typed blocks
                    Some(Value::Array(arr)) => {
                        let mut blks: Vec<ContentBlock> = Vec::new();
                        for b in arr {
                            let block_type = b.get("type").and_then(Value::as_str).unwrap_or("");
                            match block_type {
                                "text" => {
                                    if let Some(text) = b.get("text").and_then(Value::as_str) {
                                        blks.push(ContentBlock::Text {
                                            text: text.to_string(),
                                        });
                                    }
                                }
                                "tool_use" => {
                                    // Anthropic ids are already toolu_*-prefixed;
                                    // to_normalized is idempotent for those.
                                    let raw_id = b.get("id").and_then(Value::as_str).unwrap_or("");
                                    let id = tool_id::to_normalized(raw_id);
                                    let name = b
                                        .get("name")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string();
                                    let input = b
                                        .get("input")
                                        .cloned()
                                        .unwrap_or_else(|| Value::Object(Map::new()));
                                    blks.push(ContentBlock::ToolUse { id, name, input });
                                }
                                "tool_result" => {
                                    let tool_use_id =
                                        b.get("tool_use_id").and_then(Value::as_str).unwrap_or("");
                                    let id = tool_id::to_normalized(tool_use_id);
                                    // content can be string or array
                                    let content_str = match b.get("content") {
                                        Some(Value::String(s)) => s.clone(),
                                        Some(v) => v.to_string(),
                                        None => String::new(),
                                    };
                                    let is_error =
                                        b.get("is_error").and_then(Value::as_bool).unwrap_or(false);
                                    blks.push(ContentBlock::ToolResult {
                                        id,
                                        content: vec![crate::tool_payload::ToolContent::Text {
                                            text: content_str,
                                        }],
                                        is_error,
                                    });
                                }
                                "thinking" => {
                                    let text = b
                                        .get("thinking")
                                        .and_then(Value::as_str)
                                        .unwrap_or("")
                                        .to_string();
                                    let signature = b
                                        .get("signature")
                                        .and_then(Value::as_str)
                                        .map(String::from);
                                    blks.push(ContentBlock::Reasoning { text, signature });
                                }
                                "image" => {
                                    return Err(AdapterError::UnexpectedFormat(
                                        "image input not supported".into(),
                                    ));
                                }
                                // Forward-compat: skip unknown block types.
                                _ => {}
                            }
                        }
                        blks
                    }
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

        // -- tools[] ---------------------------------------------------------
        // Anthropic shape: {name, description, input_schema}
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
                            .get("input_schema")
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
        let max_tokens = body
            .get("max_tokens")
            .and_then(Value::as_u64)
            .map(|v| v as u32);
        let stop_sequences: Vec<String> = body
            .get("stop_sequences")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default();

        // thinking.budget_tokens → nearest ReasoningEffort
        let reasoning_effort = body
            .pointer("/thinking/budget_tokens")
            .and_then(Value::as_u64)
            .map(|budget| {
                let budget = budget as u32;
                // Pick the variant whose suggested_token_budget() is closest.
                let candidates = [
                    ReasoningEffort::Minimal,
                    ReasoningEffort::Low,
                    ReasoningEffort::Medium,
                    ReasoningEffort::High,
                    ReasoningEffort::XHigh,
                ];
                candidates
                    .iter()
                    .min_by_key(|e| {
                        let b = e.suggested_token_budget().unwrap_or(0);
                        (b as i64 - budget as i64).unsigned_abs()
                    })
                    .copied()
                    .unwrap_or(ReasoningEffort::Medium)
            });

        let config = ModelConfig {
            temperature,
            top_p,
            max_tokens,
            reasoning_effort,
            stop_sequences,
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

    // -----------------------------------------------------------------------
    // Task 5.5 — encode_response
    // -----------------------------------------------------------------------

    fn encode_response(&self, resp: &DecodedResponse) -> Result<Value, AdapterError> {
        let mut content: Vec<Value> = Vec::new();
        let mut has_tool_use = false;

        for block in &resp.message.content {
            match block {
                ContentBlock::Text { text } => {
                    content.push(serde_json::json!({"type": "text", "text": text}));
                }
                ContentBlock::ToolUse { id, name, input } => {
                    has_tool_use = true;
                    content.push(serde_json::json!({
                        "type": "tool_use",
                        "id": id,
                        "name": name,
                        "input": input,
                    }));
                }
                ContentBlock::Reasoning { text, signature } => {
                    // SIGNATURE RULE: only emit thinking block when signature is Some.
                    // Anthropic 400s on a thinking block without a signature.
                    if let Some(sig) = signature {
                        content.push(serde_json::json!({
                            "type": "thinking",
                            "thinking": text,
                            "signature": sig,
                        }));
                    }
                    // If signature is None, omit the block entirely.
                }
                // ToolResult / Image not expected in an assistant response; skip.
                _ => {}
            }
        }

        let stop_reason = if has_tool_use { "tool_use" } else { "end_turn" };

        let usage_src = resp.message.usage.as_ref().or(resp.usage.as_ref());
        let usage = match usage_src {
            Some(u) => serde_json::json!({
                "input_tokens": u.input_tokens,
                "output_tokens": u.output_tokens,
            }),
            None => serde_json::json!({
                "input_tokens": 0_u32,
                "output_tokens": 0_u32,
            }),
        };

        Ok(serde_json::json!({
            "id": &resp.message.id,
            "type": "message",
            "role": "assistant",
            "model": "",
            "content": content,
            "stop_reason": stop_reason,
            "stop_sequence": null,
            "usage": usage,
        }))
    }

    // -----------------------------------------------------------------------
    // Task 5.6 — encode_stream_event
    // -----------------------------------------------------------------------

    fn encode_stream_event(
        &self,
        ev: &StreamEvent,
        st: &mut StreamEncodeState,
    ) -> Result<Vec<SseFrame>, AdapterError> {
        // Helper to produce one named SSE frame (no sequence_number — Anthropic
        // doesn't use it; keep frames lean).
        macro_rules! frame {
            ($event_name:expr, $data:expr) => {
                SseFrame {
                    event: Some($event_name.to_string()),
                    data: $data,
                }
            };
        }

        let mut out: Vec<SseFrame> = Vec::new();

        match ev {
            StreamEvent::Start => {
                if !st.started {
                    st.started = true;
                    if st.response_id.is_empty() {
                        st.response_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
                    }
                    out.push(frame!(
                        "message_start",
                        serde_json::json!({
                            "type": "message_start",
                            "message": {
                                "id": &st.response_id,
                                "type": "message",
                                "role": "assistant",
                                "content": [],
                                "model": "",
                                "stop_reason": null,
                                "usage": { "input_tokens": 0_u32, "output_tokens": 0_u32 }
                            }
                        })
                    ));
                }
            }

            StreamEvent::TextDelta { text } => {
                // Open a text content block if not already open.
                if !st.text_item_open {
                    st.text_item_open = true;
                    st.text_item_index = st.block_index;
                    out.push(frame!(
                        "content_block_start",
                        serde_json::json!({
                            "type": "content_block_start",
                            "index": st.block_index,
                            "content_block": { "type": "text", "text": "" }
                        })
                    ));
                    // Don't advance block_index yet — delta uses same index.
                }
                out.push(frame!(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": st.text_item_index,
                        "delta": { "type": "text_delta", "text": text }
                    })
                ));
            }

            StreamEvent::ReasoningDelta { text } => {
                // Open a thinking block if none is open.
                if !st.text_item_open {
                    st.text_item_open = true;
                    st.text_item_index = st.block_index;
                    out.push(frame!(
                        "content_block_start",
                        serde_json::json!({
                            "type": "content_block_start",
                            "index": st.block_index,
                            "content_block": { "type": "thinking", "thinking": "" }
                        })
                    ));
                }
                out.push(frame!(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        "index": st.text_item_index,
                        "delta": { "type": "thinking_delta", "thinking": text }
                    })
                ));
            }

            StreamEvent::ReasoningBlock { .. } => {
                // Full accumulated thinking block — no additional frame needed
                // in the stream encoding path (content_block_stop will close it).
            }

            StreamEvent::ToolCallStart { id, name } => {
                // Close any currently-open text/thinking block first.
                if st.text_item_open {
                    out.push(frame!(
                        "content_block_stop",
                        serde_json::json!({
                            "type": "content_block_stop",
                            "index": st.text_item_index
                        })
                    ));
                    st.text_item_open = false;
                    st.block_index += 1;
                }
                let provider_id = tool_id::to_provider(id);
                let tool_block_index = st.block_index;
                // Store the block index for this tool call so delta/end can find it.
                st.output_index = tool_block_index;
                st.tool_args.entry(id.clone()).or_default();
                out.push(frame!(
                    "content_block_start",
                    serde_json::json!({
                        "type": "content_block_start",
                        "index": tool_block_index,
                        "content_block": {
                            "type": "tool_use",
                            "id": provider_id,
                            "name": name,
                            "input": {}
                        }
                    })
                ));
                // Advance block_index so the next block gets a fresh index.
                st.block_index += 1;
            }

            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
                ..
            } => {
                // Accumulate args and emit input_json_delta at the tool's block index.
                st.tool_args
                    .entry(id.clone())
                    .or_default()
                    .push_str(arguments_delta);
                out.push(frame!(
                    "content_block_delta",
                    serde_json::json!({
                        "type": "content_block_delta",
                        // output_index was set by ToolCallStart to the tool block's index.
                        "index": st.output_index,
                        "delta": { "type": "input_json_delta", "partial_json": arguments_delta }
                    })
                ));
            }

            StreamEvent::ToolCallEnd { id } => {
                out.push(frame!(
                    "content_block_stop",
                    serde_json::json!({
                        "type": "content_block_stop",
                        "index": st.output_index
                    })
                ));
                // Remove accumulated args (not echoed in Anthropic's stream).
                st.tool_args.remove(id);
            }

            StreamEvent::Usage(u) => {
                // Stash for embedding in message_delta later.
                st.usage = Some(u.clone());
            }

            StreamEvent::Stop { reason } => {
                // Close any open text/thinking block.
                if st.text_item_open {
                    out.push(frame!(
                        "content_block_stop",
                        serde_json::json!({
                            "type": "content_block_stop",
                            "index": st.text_item_index
                        })
                    ));
                    st.text_item_open = false;
                }

                let stop_reason_str = match reason {
                    StopReason::EndTurn => "end_turn",
                    StopReason::ToolUse => "tool_use",
                    StopReason::MaxTokens => "max_tokens",
                    StopReason::StopSequence => "stop_sequence",
                    StopReason::Other(s) => s.as_str(),
                };

                let usage_val = match &st.usage {
                    Some(u) => serde_json::json!({
                        "output_tokens": u.output_tokens,
                    }),
                    None => serde_json::json!({ "output_tokens": 0_u32 }),
                };

                out.push(frame!(
                    "message_delta",
                    serde_json::json!({
                        "type": "message_delta",
                        "delta": { "stop_reason": stop_reason_str, "stop_sequence": null },
                        "usage": usage_val,
                    })
                ));
            }

            StreamEvent::Done => {
                out.push(frame!(
                    "message_stop",
                    serde_json::json!({ "type": "message_stop" })
                ));
            }

            StreamEvent::Error(msg) => {
                out.push(frame!(
                    "error",
                    serde_json::json!({
                        "type": "error",
                        "error": { "type": "api_error", "message": msg }
                    })
                ));
            }
        }

        Ok(out)
    }

    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError> {
        let content_arr = response
            .get("content")
            .and_then(Value::as_array)
            .ok_or(AdapterError::MissingField("content"))?;

        let mut blocks: Vec<crate::content::ContentBlock> = Vec::new();
        for b in content_arr {
            let block_type = b
                .get("type")
                .and_then(Value::as_str)
                .ok_or(AdapterError::MissingField("content[].type"))?;
            match block_type {
                "text" => {
                    if let Some(text) = b.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            blocks.push(crate::content::ContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                "tool_use" => {
                    let raw_id = b.get("id").and_then(Value::as_str).unwrap_or("");
                    let id = tool_id::to_normalized(raw_id);
                    let name = b
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = b
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new()));
                    blocks.push(crate::content::ContentBlock::ToolUse { id, name, input });
                }
                "thinking" => {
                    // Preserve the signature so the block can be echoed back
                    // on the next turn (Anthropic rejects 400 otherwise).
                    let text = b
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let signature = b.get("signature").and_then(Value::as_str).map(String::from);
                    blocks.push(crate::content::ContentBlock::Reasoning { text, signature });
                }
                _ => {
                    // Forward-compat: unknown block types skipped.
                }
            }
        }

        let usage = response.get("usage").and_then(|u| parse_usage(u));

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
        let Some(event_type) = event.get("type").and_then(Value::as_str) else {
            return Err(AdapterError::MissingField("type"));
        };
        let mut out = Vec::new();

        match event_type {
            "message_start" => {
                if let Some(usage) = event.pointer("/message/usage").and_then(parse_usage) {
                    out.push(StreamEvent::Usage(usage));
                }
            }
            "content_block_start" => {
                if let Some(cb) = event.get("content_block") {
                    let block_type = cb.get("type").and_then(Value::as_str).unwrap_or("");
                    let idx = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                    state.block_type.insert(idx, block_type.to_string());
                    if block_type == "tool_use" {
                        let raw_id = cb.get("id").and_then(Value::as_str).unwrap_or("");
                        let id = tool_id::to_normalized(raw_id);
                        let name = cb
                            .get("name")
                            .and_then(Value::as_str)
                            .unwrap_or("")
                            .to_string();
                        state.tool_input_buf.insert(id.clone(), String::new());
                        // Stash idx → id mapping so input_json_delta can find it.
                        state
                            .block_type
                            .insert(usize::MAX - idx, format!("tool_use::{id}"));
                        out.push(StreamEvent::ToolCallStart { id, name });
                    } else if block_type == "thinking" {
                        // Seed per-block buffers. `thinking_delta` appends to
                        // "text" and `signature_delta` appends to "signature".
                        // On `content_block_stop` we emit the complete block.
                        state
                            .tool_input_buf
                            .insert(format!("thinking::text::{idx}"), String::new());
                        state
                            .tool_input_buf
                            .insert(format!("thinking::sig::{idx}"), String::new());
                    }
                }
            }
            "content_block_delta" => {
                let delta = event.get("delta").unwrap_or(&Value::Null);
                let delta_type = delta.get("type").and_then(Value::as_str).unwrap_or("");
                match delta_type {
                    "text_delta" => {
                        if let Some(t) = delta.get("text").and_then(Value::as_str) {
                            if !t.is_empty() {
                                out.push(StreamEvent::TextDelta {
                                    text: t.to_string(),
                                });
                            }
                        }
                    }
                    "input_json_delta" => {
                        if let Some(partial) = delta.get("partial_json").and_then(Value::as_str) {
                            let idx =
                                event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            if let Some(tag) = state.block_type.get(&(usize::MAX - idx)) {
                                if let Some(id) = tag.strip_prefix("tool_use::") {
                                    let id = id.to_string();
                                    state
                                        .tool_input_buf
                                        .entry(id.clone())
                                        .or_default()
                                        .push_str(partial);
                                    out.push(StreamEvent::ToolCallDelta {
                                        id,
                                        name: None,
                                        arguments_delta: partial.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    "thinking_delta" | "thinking" => {
                        if let Some(t) = delta.get("thinking").and_then(Value::as_str) {
                            let idx =
                                event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            if let Some(buf) = state
                                .tool_input_buf
                                .get_mut(&format!("thinking::text::{idx}"))
                            {
                                buf.push_str(t);
                            }
                            out.push(StreamEvent::ReasoningDelta {
                                text: t.to_string(),
                            });
                        }
                    }
                    "signature_delta" => {
                        // Accumulated chunk of the thinking block's signature.
                        // Not emitted downstream per-chunk — buffered and
                        // attached to the final ReasoningBlock at block_stop.
                        if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                            let idx =
                                event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                            if let Some(buf) = state
                                .tool_input_buf
                                .get_mut(&format!("thinking::sig::{idx}"))
                            {
                                buf.push_str(sig);
                            }
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                let idx = event.get("index").and_then(Value::as_u64).unwrap_or(0) as usize;
                // Tool-use block ended — emit ToolCallEnd (same id).
                if let Some(tag) = state.block_type.get(&(usize::MAX - idx)).cloned() {
                    if let Some(id) = tag.strip_prefix("tool_use::") {
                        out.push(StreamEvent::ToolCallEnd { id: id.to_string() });
                    }
                }
                // Thinking block ended — emit ReasoningBlock with the full
                // accumulated text + signature. Consumers need this to echo
                // the block back on the next turn (Anthropic extended
                // thinking requires signature round-trip).
                if state.block_type.get(&idx).map(|s| s.as_str()) == Some("thinking") {
                    let text = state
                        .tool_input_buf
                        .remove(&format!("thinking::text::{idx}"))
                        .unwrap_or_default();
                    let sig = state
                        .tool_input_buf
                        .remove(&format!("thinking::sig::{idx}"))
                        .filter(|s| !s.is_empty());
                    if !text.is_empty() || sig.is_some() {
                        out.push(StreamEvent::ReasoningBlock {
                            text,
                            signature: sig,
                        });
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = event.get("usage").and_then(parse_usage) {
                    out.push(StreamEvent::Usage(usage));
                }
                // delta.stop_reason: end_turn | tool_use | max_tokens | stop_sequence
                if let Some(stop_reason_str) =
                    event.pointer("/delta/stop_reason").and_then(Value::as_str)
                {
                    let reason = match stop_reason_str {
                        "end_turn" => StopReason::EndTurn,
                        "tool_use" => StopReason::ToolUse,
                        "max_tokens" => StopReason::MaxTokens,
                        "stop_sequence" => StopReason::StopSequence,
                        other => StopReason::Other(other.to_string()),
                    };
                    out.push(StreamEvent::Stop { reason });
                }
            }
            "message_stop" => {
                out.push(StreamEvent::Done);
            }
            _ => {}
        }
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// helpers
// ---------------------------------------------------------------------------

fn parse_usage(v: &Value) -> Option<UsageMetadata> {
    let get_u32 = |k: &str| v.get(k).and_then(Value::as_u64).map(|x| x as u32);
    let input_tokens = get_u32("input_tokens").unwrap_or(0);
    let output_tokens = get_u32("output_tokens").unwrap_or(0);
    let cache_creation_input_tokens = get_u32("cache_creation_input_tokens");
    let cache_read_input_tokens = get_u32("cache_read_input_tokens");
    if input_tokens == 0
        && output_tokens == 0
        && cache_creation_input_tokens.is_none()
        && cache_read_input_tokens.is_none()
    {
        return None;
    }
    Some(UsageMetadata {
        input_tokens,
        output_tokens,
        total_tokens: input_tokens + output_tokens,
        cache_creation_input_tokens,
        cache_read_input_tokens,
    })
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::content::ContentBlock;

    #[test]
    fn encode_tools_dedupes() {
        let tools = vec![
            ToolDefinition {
                name: "x".into(),
                description: String::new(),
                parameters: serde_json::json!({"type":"object"}),
            },
            ToolDefinition {
                name: "x".into(),
                description: String::new(),
                parameters: serde_json::json!({"type":"object"}),
            }, // duplicate
            ToolDefinition {
                name: "y".into(),
                description: String::new(),
                parameters: serde_json::json!({"type":"object"}),
            },
        ];
        let encoded = AnthropicAdapter.encode_tools(&tools);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0]["name"], "x");
        assert_eq!(encoded[1]["name"], "y");
    }

    #[test]
    fn encode_messages_merges_consecutive_tool_results() {
        // Regression: multi-tool turns used to produce one user-message per
        // tool_result, which Anthropic rejects as "tool_use ids were found
        // without tool_result blocks immediately after". Encoder must merge
        // them into a single user message.
        use crate::tool_payload::ToolContent;

        let assistant = Message {
            id: "m1".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "toolu_A".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "toolu_B".into(),
                    name: "ls".into(),
                    input: serde_json::json!({}),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let tool_a = Message {
            id: "m2".into(),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: "toolu_A".into(),
                content: vec![ToolContent::Text { text: "A".into() }],
                is_error: false,
            }],
            tool_call_id: Some("toolu_A".into()),
            usage: None,
            timestamp: 0,
        };
        let tool_b = Message {
            id: "m3".into(),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: "toolu_B".into(),
                content: vec![ToolContent::Text { text: "B".into() }],
                is_error: false,
            }],
            tool_call_id: Some("toolu_B".into()),
            usage: None,
            timestamp: 0,
        };

        let encoded = AnthropicAdapter.encode_messages(&[assistant, tool_a, tool_b]);
        assert_eq!(
            encoded.messages.len(),
            2,
            "expected assistant + merged user, got {encoded:?}",
        );
        assert_eq!(encoded.messages[0]["role"], "assistant");
        assert_eq!(encoded.messages[1]["role"], "user");
        let results = encoded.messages[1]["content"].as_array().unwrap();
        assert_eq!(
            results.len(),
            2,
            "both tool_results must share one user msg"
        );
        assert_eq!(results[0]["tool_use_id"], "toolu_A");
        assert_eq!(results[1]["tool_use_id"], "toolu_B");
    }

    #[test]
    fn encode_messages_splits_system() {
        let msgs = vec![Message::system("you are alva"), Message::user("hi")];
        let out = AnthropicAdapter.encode_messages(&msgs);
        assert_eq!(out.system_flat().as_deref(), Some("you are alva"));
        assert_eq!(out.messages.len(), 1);
        assert_eq!(out.messages[0]["role"], "user");
    }

    #[test]
    fn decode_response_parses_tool_use() {
        let resp = serde_json::json!({
            "content": [
                { "type": "text", "text": "ok, doing it" },
                { "type": "tool_use", "id": "toolu_01", "name": "read", "input": {"path": "/a"} }
            ],
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        });
        let decoded = AnthropicAdapter.decode_response(&resp).unwrap();
        assert_eq!(decoded.message.content.len(), 2);
        match &decoded.message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
        let usage = decoded.usage.unwrap();
        assert_eq!(usage.input_tokens, 10);
        assert_eq!(usage.output_tokens, 5);
        assert_eq!(usage.total_tokens, 15);
    }

    #[test]
    fn decode_stream_emits_start_delta_end() {
        let mut state = StreamDecodeState::new();

        let start = serde_json::json!({
            "type": "content_block_start",
            "index": 0,
            "content_block": { "type": "tool_use", "id": "toolu_01", "name": "read" }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&start, &mut state)
            .unwrap();
        match &out[0] {
            StreamEvent::ToolCallStart { id, name } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCallStart"),
        }

        let delta = serde_json::json!({
            "type": "content_block_delta",
            "index": 0,
            "delta": { "type": "input_json_delta", "partial_json": "{\"path\":\"/a\"}" }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&delta, &mut state)
            .unwrap();
        match &out[0] {
            StreamEvent::ToolCallDelta {
                id,
                arguments_delta,
                ..
            } => {
                assert_eq!(id, "toolu_01");
                assert_eq!(arguments_delta, "{\"path\":\"/a\"}");
            }
            _ => panic!("expected ToolCallDelta"),
        }
        assert_eq!(state.tool_input_buf["toolu_01"], "{\"path\":\"/a\"}");

        let stop = serde_json::json!({
            "type": "content_block_stop",
            "index": 0,
        });
        let out = AnthropicAdapter
            .decode_stream_event(&stop, &mut state)
            .unwrap();
        match &out[0] {
            StreamEvent::ToolCallEnd { id } => assert_eq!(id, "toolu_01"),
            _ => panic!("expected ToolCallEnd"),
        }
    }

    #[test]
    fn decode_stream_emits_done() {
        let mut state = StreamDecodeState::new();
        let stop = serde_json::json!({ "type": "message_stop" });
        let out = AnthropicAdapter
            .decode_stream_event(&stop, &mut state)
            .unwrap();
        assert!(matches!(out[0], StreamEvent::Done));
    }

    #[test]
    fn decode_stream_message_delta_emits_stop_with_reason() {
        let mut state = StreamDecodeState::new();
        // Anthropic message_delta with end_turn
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "end_turn", "stop_sequence": null },
            "usage": { "output_tokens": 42 }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&ev, &mut state)
            .unwrap();
        // Usage first, then Stop
        assert!(matches!(out[0], StreamEvent::Usage(_)));
        assert!(matches!(
            &out[1],
            StreamEvent::Stop {
                reason: StopReason::EndTurn
            }
        ));
    }

    #[test]
    fn decode_stream_message_delta_tool_use_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use" }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&ev, &mut state)
            .unwrap();
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::ToolUse
            }
        ));
    }

    #[test]
    fn decode_stream_message_delta_max_tokens_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "max_tokens" }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&ev, &mut state)
            .unwrap();
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::MaxTokens
            }
        ));
    }

    #[test]
    fn decode_stream_message_delta_stop_sequence_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "stop_sequence" }
        });
        let out = AnthropicAdapter
            .decode_stream_event(&ev, &mut state)
            .unwrap();
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::StopSequence
            }
        ));
    }

    // -----------------------------------------------------------------------
    // Task 5.4 — decode_request tests
    // -----------------------------------------------------------------------

    #[test]
    fn anthropic_decode_request_system_and_tools() {
        let body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "system": "You are a helpful assistant.",
            "messages": [
                { "role": "user", "content": "Hello" }
            ],
            "tools": [
                {
                    "name": "read_file",
                    "description": "Reads a file",
                    "input_schema": { "type": "object", "properties": { "path": { "type": "string" } } }
                }
            ]
        });
        let req = AnthropicAdapter.decode_request(&body).unwrap();
        assert_eq!(req.model, "claude-3-5-sonnet-20241022");
        // System message should be prepended
        assert!(matches!(req.messages[0].role, MessageRole::System));
        assert_eq!(
            req.messages[0].text_content(),
            "You are a helpful assistant."
        );
        // User message follows
        assert!(req
            .messages
            .iter()
            .any(|m| matches!(m.role, MessageRole::User)));
        // Tools parsed correctly
        assert_eq!(req.tools.len(), 1);
        assert_eq!(req.tools[0].name, "read_file");
        assert_eq!(req.tools[0].description, "Reads a file");
    }

    #[test]
    fn anthropic_decode_request_rejects_image() {
        let body = serde_json::json!({
            "model": "claude-3-5-sonnet-20241022",
            "messages": [{
                "role": "user",
                "content": [
                    { "type": "image", "source": { "type": "base64", "media_type": "image/png", "data": "abc" } }
                ]
            }]
        });
        assert!(matches!(
            AnthropicAdapter.decode_request(&body),
            Err(AdapterError::UnexpectedFormat(_))
        ));
    }

    #[test]
    fn anthropic_decode_request_thinking_budget_maps_effort() {
        // budget_tokens=16384 → ReasoningEffort::High (suggested_token_budget() = 16384)
        let body = serde_json::json!({
            "model": "claude-opus-4-5",
            "messages": [{ "role": "user", "content": "think hard" }],
            "thinking": { "type": "enabled", "budget_tokens": 16384 }
        });
        let req = AnthropicAdapter.decode_request(&body).unwrap();
        assert_eq!(req.config.reasoning_effort, Some(ReasoningEffort::High));
    }

    // -----------------------------------------------------------------------
    // Task 5.5 — encode_response tests
    // -----------------------------------------------------------------------

    #[test]
    fn anthropic_encode_response_text_and_tool() {
        use super::super::DecodedResponse;
        use crate::message::UsageMetadata;

        let dr = DecodedResponse {
            message: Message {
                id: "msg_01".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "Let me help.".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "toolu_abc".into(),
                        name: "read_file".into(),
                        input: serde_json::json!({"path": "/etc/hosts"}),
                    },
                ],
                tool_call_id: None,
                usage: Some(UsageMetadata {
                    input_tokens: 42,
                    output_tokens: 15,
                    total_tokens: 57,
                    cache_creation_input_tokens: None,
                    cache_read_input_tokens: None,
                }),
                timestamp: 0,
            },
            usage: None,
        };

        let v = AnthropicAdapter.encode_response(&dr).unwrap();
        assert_eq!(v["type"], "message");
        assert_eq!(v["role"], "assistant");
        assert_eq!(v["stop_reason"], "tool_use");
        let content = v["content"].as_array().unwrap();
        assert!(content
            .iter()
            .any(|b| b["type"] == "tool_use" && b["name"] == "read_file"));
        assert_eq!(v["usage"]["input_tokens"], 42);
    }

    #[test]
    fn anthropic_encode_response_omits_unsigned_thinking() {
        use super::super::DecodedResponse;

        // Reasoning block WITHOUT signature — must be omitted
        let dr_no_sig = DecodedResponse {
            message: Message {
                id: "msg_02".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Reasoning {
                        text: "thoughts".into(),
                        signature: None,
                    },
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                ],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            usage: None,
        };
        let v = AnthropicAdapter.encode_response(&dr_no_sig).unwrap();
        let content = v["content"].as_array().unwrap();
        assert!(
            !content.iter().any(|b| b["type"] == "thinking"),
            "thinking block with no signature must be omitted"
        );
        // Text block still present
        assert!(content.iter().any(|b| b["type"] == "text"));

        // Reasoning block WITH signature — must be emitted
        let dr_with_sig = DecodedResponse {
            message: Message {
                id: "msg_03".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Reasoning {
                    text: "deep thoughts".into(),
                    signature: Some("sig_abc123".into()),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            usage: None,
        };
        let v2 = AnthropicAdapter.encode_response(&dr_with_sig).unwrap();
        let content2 = v2["content"].as_array().unwrap();
        assert!(
            content2
                .iter()
                .any(|b| b["type"] == "thinking" && b["signature"] == "sig_abc123"),
            "thinking block with signature must appear"
        );
    }

    // -----------------------------------------------------------------------
    // Task 5.6 — encode_stream_event tests
    // -----------------------------------------------------------------------

    #[test]
    fn anthropic_encode_stream_text_then_stop() {
        use super::super::StreamEncodeState;
        use crate::message::UsageMetadata;

        let mut st = StreamEncodeState::default();
        let mut frames: Vec<SseFrame> = Vec::new();

        for ev in [
            StreamEvent::Start,
            StreamEvent::TextDelta { text: "hi".into() },
            StreamEvent::Usage(UsageMetadata {
                input_tokens: 10,
                output_tokens: 5,
                total_tokens: 15,
                cache_creation_input_tokens: None,
                cache_read_input_tokens: None,
            }),
            StreamEvent::Stop {
                reason: StopReason::EndTurn,
            },
            StreamEvent::Done,
        ] {
            frames.extend(AnthropicAdapter.encode_stream_event(&ev, &mut st).unwrap());
        }

        let event_names: Vec<&str> = frames.iter().filter_map(|f| f.event.as_deref()).collect();

        assert!(
            event_names.contains(&"message_start"),
            "missing message_start: {event_names:?}"
        );
        assert!(
            event_names.contains(&"content_block_delta"),
            "missing content_block_delta: {event_names:?}"
        );
        assert!(
            event_names.contains(&"message_delta"),
            "missing message_delta: {event_names:?}"
        );
        assert!(
            event_names.contains(&"message_stop"),
            "missing message_stop: {event_names:?}"
        );

        // The message_delta must carry stop_reason == "end_turn"
        let msg_delta = frames
            .iter()
            .find(|f| f.event.as_deref() == Some("message_delta"))
            .unwrap();
        assert_eq!(
            msg_delta
                .data
                .pointer("/delta/stop_reason")
                .and_then(|v| v.as_str()),
            Some("end_turn"),
            "message_delta stop_reason must be end_turn"
        );
    }

    #[test]
    fn anthropic_encode_stream_tool_args() {
        use super::super::StreamEncodeState;

        let mut st = StreamEncodeState::default();
        let mut frames: Vec<SseFrame> = Vec::new();

        for ev in [
            StreamEvent::ToolCallStart {
                id: "toolu_01".into(),
                name: "read_file".into(),
            },
            StreamEvent::ToolCallDelta {
                id: "toolu_01".into(),
                name: None,
                arguments_delta: "{\"path\":".into(),
            },
            StreamEvent::ToolCallDelta {
                id: "toolu_01".into(),
                name: None,
                arguments_delta: "\"/a\"}".into(),
            },
            StreamEvent::ToolCallEnd {
                id: "toolu_01".into(),
            },
        ] {
            frames.extend(AnthropicAdapter.encode_stream_event(&ev, &mut st).unwrap());
        }

        // Verify content_block_start for tool_use
        let start_frame = frames
            .iter()
            .find(|f| f.event.as_deref() == Some("content_block_start"))
            .expect("content_block_start must be emitted");
        assert_eq!(
            start_frame
                .data
                .pointer("/content_block/type")
                .and_then(|v| v.as_str()),
            Some("tool_use")
        );

        // Verify two input_json_delta fragments
        let delta_frames: Vec<_> = frames
            .iter()
            .filter(|f| {
                f.event.as_deref() == Some("content_block_delta")
                    && f.data.pointer("/delta/type").and_then(|v| v.as_str())
                        == Some("input_json_delta")
            })
            .collect();
        assert_eq!(delta_frames.len(), 2, "expected 2 input_json_delta frames");
        assert_eq!(
            delta_frames[0]
                .data
                .pointer("/delta/partial_json")
                .and_then(|v| v.as_str()),
            Some("{\"path\":")
        );
        assert_eq!(
            delta_frames[1]
                .data
                .pointer("/delta/partial_json")
                .and_then(|v| v.as_str()),
            Some("\"/a\"}")
        );

        // Verify content_block_stop
        assert!(
            frames
                .iter()
                .any(|f| f.event.as_deref() == Some("content_block_stop")),
            "content_block_stop must be emitted"
        );
    }
}
