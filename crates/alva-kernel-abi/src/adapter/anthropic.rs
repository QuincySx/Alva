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
    common::tool_id, AdapterError, DecodedResponse, EncodedMessages, ProtocolAdapter,
    StreamDecodeState,
};
use crate::base::message::{Message, MessageRole, UsageMetadata};
use crate::base::stream::{StopReason, StreamEvent};
use crate::tool::ToolDefinition;

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
                            crate::base::content::ContentBlock::Text { text } => {
                                blocks.push(serde_json::json!({"type": "text", "text": text}));
                            }
                            crate::base::content::ContentBlock::Reasoning {
                                text,
                                signature,
                            } => {
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
                            crate::base::content::ContentBlock::ToolUse { id, name, input } => {
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
                        if let crate::base::content::ContentBlock::ToolResult {
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

    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError> {
        let content_arr = response
            .get("content")
            .and_then(Value::as_array)
            .ok_or(AdapterError::MissingField("content"))?;

        let mut blocks: Vec<crate::base::content::ContentBlock> = Vec::new();
        for b in content_arr {
            let block_type = b
                .get("type")
                .and_then(Value::as_str)
                .ok_or(AdapterError::MissingField("content[].type"))?;
            match block_type {
                "text" => {
                    if let Some(text) = b.get("text").and_then(Value::as_str) {
                        if !text.is_empty() {
                            blocks.push(crate::base::content::ContentBlock::Text {
                                text: text.to_string(),
                            });
                        }
                    }
                }
                "tool_use" => {
                    let raw_id = b.get("id").and_then(Value::as_str).unwrap_or("");
                    let id = tool_id::to_normalized(raw_id);
                    let name = b.get("name").and_then(Value::as_str).unwrap_or("").to_string();
                    let input = b
                        .get("input")
                        .cloned()
                        .unwrap_or_else(|| Value::Object(Map::new()));
                    blocks.push(crate::base::content::ContentBlock::ToolUse { id, name, input });
                }
                "thinking" => {
                    // Preserve the signature so the block can be echoed back
                    // on the next turn (Anthropic rejects 400 otherwise).
                    let text = b
                        .get("thinking")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let signature = b
                        .get("signature")
                        .and_then(Value::as_str)
                        .map(String::from);
                    blocks.push(crate::base::content::ContentBlock::Reasoning {
                        text,
                        signature,
                    });
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
                if let Some(usage) = event
                    .pointer("/message/usage")
                    .and_then(parse_usage)
                {
                    out.push(StreamEvent::Usage(usage));
                }
            }
            "content_block_start" => {
                if let Some(cb) = event.get("content_block") {
                    let block_type =
                        cb.get("type").and_then(Value::as_str).unwrap_or("");
                    let idx = event
                        .get("index")
                        .and_then(Value::as_u64)
                        .unwrap_or(0) as usize;
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
                        out.push(StreamEvent::ToolCallStart {
                            id,
                            name,
                        });
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
                                out.push(StreamEvent::TextDelta { text: t.to_string() });
                            }
                        }
                    }
                    "input_json_delta" => {
                        if let Some(partial) =
                            delta.get("partial_json").and_then(Value::as_str)
                        {
                            let idx = event
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
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
                            let idx = event
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
                            if let Some(buf) = state
                                .tool_input_buf
                                .get_mut(&format!("thinking::text::{idx}"))
                            {
                                buf.push_str(t);
                            }
                            out.push(StreamEvent::ReasoningDelta { text: t.to_string() });
                        }
                    }
                    "signature_delta" => {
                        // Accumulated chunk of the thinking block's signature.
                        // Not emitted downstream per-chunk — buffered and
                        // attached to the final ReasoningBlock at block_stop.
                        if let Some(sig) = delta.get("signature").and_then(Value::as_str) {
                            let idx = event
                                .get("index")
                                .and_then(Value::as_u64)
                                .unwrap_or(0) as usize;
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
                let idx = event
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
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
                if let Some(stop_reason_str) = event
                    .pointer("/delta/stop_reason")
                    .and_then(Value::as_str)
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
    use crate::base::content::ContentBlock;

    #[test]
    fn encode_tools_dedupes() {
        let tools = vec![
            ToolDefinition { name: "x".into(), description: String::new(), parameters: serde_json::json!({"type":"object"}) },
            ToolDefinition { name: "x".into(), description: String::new(), parameters: serde_json::json!({"type":"object"}) }, // duplicate
            ToolDefinition { name: "y".into(), description: String::new(), parameters: serde_json::json!({"type":"object"}) },
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
        use crate::tool::execution::ToolContent;

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
        assert_eq!(results.len(), 2, "both tool_results must share one user msg");
        assert_eq!(results[0]["tool_use_id"], "toolu_A");
        assert_eq!(results[1]["tool_use_id"], "toolu_B");
    }

    #[test]
    fn encode_messages_splits_system() {
        let msgs = vec![
            Message::system("you are alva"),
            Message::user("hi"),
        ];
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
        let out = AnthropicAdapter.decode_stream_event(&start, &mut state).unwrap();
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
        let out = AnthropicAdapter.decode_stream_event(&delta, &mut state).unwrap();
        match &out[0] {
            StreamEvent::ToolCallDelta { id, arguments_delta, .. } => {
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
        let out = AnthropicAdapter.decode_stream_event(&stop, &mut state).unwrap();
        match &out[0] {
            StreamEvent::ToolCallEnd { id } => assert_eq!(id, "toolu_01"),
            _ => panic!("expected ToolCallEnd"),
        }
    }

    #[test]
    fn decode_stream_emits_done() {
        let mut state = StreamDecodeState::new();
        let stop = serde_json::json!({ "type": "message_stop" });
        let out = AnthropicAdapter.decode_stream_event(&stop, &mut state).unwrap();
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
        let out = AnthropicAdapter.decode_stream_event(&ev, &mut state).unwrap();
        // Usage first, then Stop
        assert!(matches!(out[0], StreamEvent::Usage(_)));
        assert!(matches!(&out[1], StreamEvent::Stop { reason: StopReason::EndTurn }));
    }

    #[test]
    fn decode_stream_message_delta_tool_use_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "tool_use" }
        });
        let out = AnthropicAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert!(matches!(&out[0], StreamEvent::Stop { reason: StopReason::ToolUse }));
    }

    #[test]
    fn decode_stream_message_delta_max_tokens_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "max_tokens" }
        });
        let out = AnthropicAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert!(matches!(&out[0], StreamEvent::Stop { reason: StopReason::MaxTokens }));
    }

    #[test]
    fn decode_stream_message_delta_stop_sequence_stop_reason() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "type": "message_delta",
            "delta": { "stop_reason": "stop_sequence" }
        });
        let out = AnthropicAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert!(matches!(&out[0], StreamEvent::Stop { reason: StopReason::StopSequence }));
    }
}
