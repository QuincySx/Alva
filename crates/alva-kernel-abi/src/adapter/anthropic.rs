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
    common::tool_id, AdapterError, DecodedResponse, EncodedMessages, StreamDecodeState,
    ToolAdapter,
};
use crate::base::message::{Message, MessageRole, UsageMetadata};
use crate::base::stream::StreamEvent;
use crate::tool::Tool;

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

impl ToolAdapter for AnthropicAdapter {
    fn provider(&self) -> &'static str {
        "anthropic"
    }

    fn encode_tools(&self, tools: &[&dyn Tool]) -> Vec<Value> {
        // AMP `Bx`: dedupe by name, schema passthrough.
        let mut seen = std::collections::HashSet::new();
        tools
            .iter()
            .filter(|t| seen.insert(t.name().to_string()))
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "input_schema": t.parameters_schema(),
                })
            })
            .collect()
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        let mut system: Option<String> = None;
        let mut api_messages: Vec<Value> = Vec::new();

        for m in messages {
            match m.role {
                MessageRole::System => {
                    let text = m.text_content();
                    if !text.is_empty() {
                        system = Some(match system {
                            Some(existing) => format!("{existing}\n\n{text}"),
                            None => text,
                        });
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
                    api_messages.push(serde_json::json!({
                        "role": "user",
                        "content": blocks,
                    }));
                }
            }
        }

        EncodedMessages {
            system,
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
                    if let Some(t) = b.get("thinking").and_then(Value::as_str) {
                        if !t.is_empty() {
                            blocks.push(crate::base::content::ContentBlock::Text {
                                text: format!("<thinking>\n{t}\n</thinking>"),
                            });
                        }
                    }
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
                            out.push(StreamEvent::ReasoningDelta { text: t.to_string() });
                        }
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                // Emit ToolCallEnd if this block was a tool_use.
                let idx = event
                    .get("index")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                if let Some(tag) = state.block_type.get(&(usize::MAX - idx)).cloned() {
                    if let Some(id) = tag.strip_prefix("tool_use::") {
                        out.push(StreamEvent::ToolCallEnd { id: id.to_string() });
                    }
                }
            }
            "message_delta" => {
                if let Some(usage) = event.get("usage").and_then(parse_usage) {
                    out.push(StreamEvent::Usage(usage));
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
        struct MockTool(&'static str);
        #[async_trait::async_trait]
        impl Tool for MockTool {
            fn name(&self) -> &str { self.0 }
            fn description(&self) -> &str { "" }
            fn parameters_schema(&self) -> Value { serde_json::json!({"type":"object"}) }
            async fn execute(&self, _i: Value, _c: &dyn crate::tool::execution::ToolExecutionContext)
                -> Result<crate::tool::execution::ToolOutput, crate::base::error::AgentError>
            {
                unreachable!()
            }
        }
        let a = MockTool("x");
        let b = MockTool("x"); // duplicate
        let c = MockTool("y");
        let tools: Vec<&dyn Tool> = vec![&a, &b, &c];
        let encoded = AnthropicAdapter.encode_tools(&tools);
        assert_eq!(encoded.len(), 2);
        assert_eq!(encoded[0]["name"], "x");
        assert_eq!(encoded[1]["name"], "y");
    }

    #[test]
    fn encode_messages_splits_system() {
        let msgs = vec![
            Message::system("you are alva"),
            Message::user("hi"),
        ];
        let out = AnthropicAdapter.encode_messages(&msgs);
        assert_eq!(out.system.as_deref(), Some("you are alva"));
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
}
