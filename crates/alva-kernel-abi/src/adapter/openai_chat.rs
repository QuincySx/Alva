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
    AdapterError, DecodedResponse, EncodedMessages, StreamDecodeState, ToolAdapter,
};
use crate::base::content::ContentBlock;
use crate::base::message::{Message, MessageRole, UsageMetadata};
use crate::base::stream::StreamEvent;
use crate::tool::Tool;

#[derive(Debug, Default, Clone, Copy)]
pub struct OpenAIChatAdapter;

impl OpenAIChatAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ToolAdapter for OpenAIChatAdapter {
    fn provider(&self) -> &'static str {
        "openai-chat"
    }

    fn encode_tools(&self, tools: &[&dyn Tool]) -> Vec<Value> {
        let mut seen = std::collections::HashSet::new();
        tools
            .iter()
            .filter(|t| seen.insert(t.name().to_string()))
            .map(|t| {
                let mut params = t.parameters_schema();
                // AMP `YLR`: OpenAI-compat backends reject properties missing `type`.
                schema_fix::fill_missing_types(&mut params);
                // Ensure an explicit additionalProperties on object nodes — some
                // compatible backends default to false and reject extra fields.
                schema_fix::force_additional_properties(&mut params, true);
                serde_json::json!({
                    "type": "function",
                    "function": {
                        "name": t.name(),
                        "description": t.description(),
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
                blocks.push(ContentBlock::Text { text: text.to_string() });
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
                let args_str =
                    function.get("arguments").and_then(Value::as_str).unwrap_or("{}");
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
                        out.push(StreamEvent::TextDelta { text: text.to_string() });
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
                // ToolCallEnd for any tool calls still open in this stream.
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
}

// ---------------------------------------------------------------------------
// tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    struct MockTool {
        n: &'static str,
        schema: Value,
    }
    #[async_trait::async_trait]
    impl Tool for MockTool {
        fn name(&self) -> &str { self.n }
        fn description(&self) -> &str { "" }
        fn parameters_schema(&self) -> Value { self.schema.clone() }
        async fn execute(&self, _i: Value, _c: &dyn crate::tool::execution::ToolExecutionContext)
            -> Result<crate::tool::execution::ToolOutput, crate::base::error::AgentError>
        {
            unreachable!()
        }
    }

    #[test]
    fn encode_tools_wraps_function_and_fixes_schema() {
        // Schema missing `type` on properties — YLR should patch it.
        let t = MockTool {
            n: "read",
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "tags": { "items": { "type": "string" } }
                }
            }),
        };
        let tools: Vec<&dyn Tool> = vec![&t];
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
                ContentBlock::Text { text: "doing".into() },
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
        let out1 = OpenAIChatAdapter.decode_stream_event(&c1, &mut state).unwrap();
        match &out1[0] {
            StreamEvent::ToolCallStart { id, name } => {
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCallStart"),
        }
        match &out1[1] {
            StreamEvent::ToolCallDelta { id, arguments_delta, .. } => {
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
        let out2 = OpenAIChatAdapter.decode_stream_event(&c2, &mut state).unwrap();
        match &out2[0] {
            StreamEvent::ToolCallDelta { id, arguments_delta, .. } => {
                assert_eq!(id, "toolu_call_abc");
                assert_eq!(arguments_delta, "th\":\"/a\"}");
            }
            _ => panic!("expected ToolCallDelta"),
        }
        assert_eq!(state.tool_input_buf["toolu_call_abc"], "{\"path\":\"/a\"}");

        // Third chunk: finish_reason present → emits ToolCallEnd
        let c3 = serde_json::json!({
            "choices": [{ "delta": {}, "finish_reason": "tool_calls" }]
        });
        let out3 = OpenAIChatAdapter.decode_stream_event(&c3, &mut state).unwrap();
        assert!(matches!(&out3[0], StreamEvent::ToolCallEnd { .. }));
    }
}
