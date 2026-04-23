// INPUT:  super::{ToolAdapter, EncodedMessages, DecodedResponse, StreamDecodeState, AdapterError}
// OUTPUT: GeminiAdapter (Google Gemini / Vertex AI `generateContent` + `streamGenerateContent`)
// POS:    Most complex adapter — functionDeclarations wrapper, type enum mapping, examples→example,
//         id auto-generation (Gemini doesn't emit tool-use ids), non-streamed tool calls.

//! Google Gemini / Vertex AI adapter (`generateContent` + `streamGenerateContent`).
//!
//! Gemini diverges from OpenAI-style APIs in several ways; the adapter
//! hides them so callers can treat it like any other `ToolAdapter`.
//!
//! Key translations:
//! - All tools wrapped in a single outer `{functionDeclarations: [...]}` tool
//! - Schema `type` maps from lowercase string to uppercase enum
//!   (`"string"` → `"STRING"`, etc.)
//! - JSON Schema `examples: [x, y]` → Gemini `example: x` (single value)
//! - Message roles: `user` / `model` / `function` (not `assistant` / `tool`)
//! - Tool calls: `parts: [{functionCall: {name, args}}]`
//! - Tool results: `parts: [{functionResponse: {name, response}}]` — matched
//!   by **name**, not id. The encoder walks back through `messages` to
//!   find the matching `ToolUse.name` for each `ToolResult.id`.
//! - Tool-use ids: Gemini doesn't emit any — the adapter generates a
//!   normalized `toolu_*` id on decode using `common::tool_id::generate()`.
//! - Streaming: Gemini does NOT stream tool-call arguments incrementally.
//!   The entire `functionCall` arrives in one event. The adapter surfaces
//!   it as a single `ToolCallDelta` with the complete JSON in
//!   `arguments_delta`.

use serde_json::{Map, Value};

use super::{
    common::tool_id, AdapterError, DecodedResponse, EncodedMessages, StreamDecodeState,
    ToolAdapter,
};
use crate::base::content::ContentBlock;
use crate::base::message::{Message, MessageRole, UsageMetadata};
use crate::base::stream::StreamEvent;
use crate::tool::Tool;

#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ToolAdapter for GeminiAdapter {
    fn provider(&self) -> &'static str {
        "gemini"
    }

    fn encode_tools(&self, tools: &[&dyn Tool]) -> Vec<Value> {
        if tools.is_empty() {
            return vec![];
        }
        let declarations: Vec<Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name(),
                    "description": t.description(),
                    "parameters": convert_schema(&t.parameters_schema()),
                })
            })
            .collect();
        vec![serde_json::json!({ "functionDeclarations": declarations })]
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        let mut system: Option<String> = None;
        let mut contents: Vec<Value> = Vec::new();

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
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": [{"text": m.text_content()}],
                    }));
                }
                MessageRole::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    for b in &m.content {
                        match b {
                            ContentBlock::Text { text } if !text.is_empty() => {
                                parts.push(serde_json::json!({"text": text}));
                            }
                            ContentBlock::ToolUse { name, input, .. } => {
                                parts.push(serde_json::json!({
                                    "functionCall": { "name": name, "args": input }
                                }));
                            }
                            _ => {}
                        }
                    }
                    if parts.is_empty() {
                        let text = m.text_content();
                        if !text.is_empty() {
                            parts.push(serde_json::json!({"text": text}));
                        }
                    }
                    if !parts.is_empty() {
                        contents.push(serde_json::json!({
                            "role": "model",
                            "parts": parts,
                        }));
                    }
                }
                MessageRole::Tool => {
                    // Gemini matches tool results by NAME — find the name by
                    // walking backward through the already-processed messages
                    // for a matching tool_use id.
                    for b in &m.content {
                        if let ContentBlock::ToolResult { id, content, .. } = b {
                            let name = find_name_for_id(messages, id)
                                .unwrap_or_else(|| "unknown".to_string());
                            let text: String = content
                                .iter()
                                .map(|tc| tc.to_model_string())
                                .collect::<Vec<_>>()
                                .join("\n");
                            contents.push(serde_json::json!({
                                "role": "function",
                                "parts": [{
                                    "functionResponse": {
                                        "name": name,
                                        "response": { "content": text },
                                    }
                                }],
                            }));
                        }
                    }
                }
            }
        }

        EncodedMessages {
            system,
            messages: contents,
        }
    }

    fn decode_response(&self, response: &Value) -> Result<DecodedResponse, AdapterError> {
        let candidates = response
            .get("candidates")
            .and_then(Value::as_array)
            .ok_or(AdapterError::MissingField("candidates"))?;
        let candidate = candidates
            .first()
            .ok_or(AdapterError::MissingField("candidates[0]"))?;
        let parts_arr = candidate
            .pointer("/content/parts")
            .and_then(Value::as_array)
            .ok_or(AdapterError::MissingField("candidates[0].content.parts"))?;

        let mut blocks: Vec<ContentBlock> = Vec::new();
        for part in parts_arr {
            if let Some(text) = part.get("text").and_then(Value::as_str) {
                if !text.is_empty() {
                    blocks.push(ContentBlock::Text { text: text.to_string() });
                }
            } else if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
                // Gemini doesn't emit ids — generate a normalized one.
                blocks.push(ContentBlock::ToolUse {
                    id: tool_id::generate(),
                    name,
                    input,
                });
            }
        }

        let usage = response.get("usageMetadata").map(|u| UsageMetadata {
            input_tokens: u.get("promptTokenCount").and_then(Value::as_u64).unwrap_or(0) as u32,
            output_tokens: u
                .get("candidatesTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            total_tokens: u
                .get("totalTokenCount")
                .and_then(Value::as_u64)
                .unwrap_or(0) as u32,
            ..Default::default()
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
        // Gemini's streamGenerateContent returns the same shape as
        // generateContent, one message at a time. Tool calls arrive
        // fully-formed (no incremental JSON). We surface each functionCall
        // as a ToolCallDelta with the complete JSON in arguments_delta.
        let mut out = Vec::new();
        let candidates = event.get("candidates").and_then(Value::as_array);
        if let Some(candidates) = candidates {
            for candidate in candidates {
                if let Some(parts) = candidate.pointer("/content/parts").and_then(Value::as_array)
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(StreamEvent::TextDelta { text: text.to_string() });
                            }
                        } else if let Some(fc) = part.get("functionCall") {
                            let name = fc
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
                            let args_str = args.to_string();
                            let id = tool_id::generate();
                            state
                                .tool_input_buf
                                .insert(id.clone(), args_str.clone());
                            // Gemini gives us the complete tool call in one event — emit the
                            // full start/delta/end triple so consumers match other providers.
                            out.push(StreamEvent::ToolCallStart {
                                id: id.clone(),
                                name: name.clone(),
                            });
                            out.push(StreamEvent::ToolCallDelta {
                                id: id.clone(),
                                name: Some(name),
                                arguments_delta: args_str,
                            });
                            out.push(StreamEvent::ToolCallEnd { id });
                        }
                    }
                }
            }
        }
        if let Some(usage) = event.get("usageMetadata") {
            out.push(StreamEvent::Usage(UsageMetadata {
                input_tokens: usage
                    .get("promptTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                output_tokens: usage
                    .get("candidatesTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                total_tokens: usage
                    .get("totalTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                ..Default::default()
            }));
        }
        // finishReason == "STOP" is the closest analog to "this was the last
        // event" but Gemini's stream just closes — the provider will emit
        // StreamEvent::Done itself when the byte stream ends. We don't emit
        // Done here to avoid double-emit.
        Ok(out)
    }
}

// ---------------------------------------------------------------------------
// Schema conversion (JSON Schema → Gemini OpenAPI subset)
// ---------------------------------------------------------------------------

fn map_type(t: &str) -> Option<&'static str> {
    match t {
        "string" => Some("STRING"),
        "number" => Some("NUMBER"),
        "integer" => Some("INTEGER"),
        "boolean" => Some("BOOLEAN"),
        "object" => Some("OBJECT"),
        "array" => Some("ARRAY"),
        _ => None,
    }
}

/// Recursive schema conversion. Gemini uses an OpenAPI 3.0 subset:
/// - `type` is an UPPERCASE enum (STRING/NUMBER/...)
/// - `examples: [...]` is not supported, but `example: <single>` is
/// - `oneOf/anyOf/allOf` and most custom keywords are rejected — we strip
/// - `additionalProperties` is not supported — we strip
fn convert_schema(s: &Value) -> Value {
    let Value::Object(obj) = s else {
        return s.clone();
    };
    let mut out = Map::new();

    if let Some(t) = obj.get("type").and_then(Value::as_str) {
        if let Some(g) = map_type(t) {
            out.insert("type".to_string(), Value::String(g.to_string()));
        }
    }
    if let Some(d) = obj.get("description").cloned() {
        out.insert("description".to_string(), d);
    }
    if let Some(r) = obj.get("required").cloned() {
        out.insert("required".to_string(), r);
    }
    if let Some(e) = obj.get("enum").cloned() {
        out.insert("enum".to_string(), e);
    }
    if let Some(examples) = obj.get("examples").and_then(Value::as_array) {
        if let Some(first) = examples.first() {
            out.insert("example".to_string(), first.clone());
        }
    }
    if let Some(example) = obj.get("example").cloned() {
        out.insert("example".to_string(), example);
    }
    if let Some(nullable) = obj.get("nullable").cloned() {
        out.insert("nullable".to_string(), nullable);
    }
    if let Some(format) = obj.get("format").cloned() {
        out.insert("format".to_string(), format);
    }
    if let Some(properties) = obj.get("properties").and_then(Value::as_object) {
        let mut p_out = Map::new();
        for (k, v) in properties {
            p_out.insert(k.clone(), convert_schema(v));
        }
        out.insert("properties".to_string(), Value::Object(p_out));
        // If `properties` was present but `type` missing, Gemini still expects OBJECT
        if !out.contains_key("type") {
            out.insert("type".to_string(), Value::String("OBJECT".to_string()));
        }
    }
    if let Some(items) = obj.get("items") {
        out.insert("items".to_string(), convert_schema(items));
        if !out.contains_key("type") {
            out.insert("type".to_string(), Value::String("ARRAY".to_string()));
        }
    }
    Value::Object(out)
}

/// Walk `messages` looking for a ToolUse whose id matches `target_id`, and
/// return its `name`. Used by the Tool-role encoder to supply Gemini's
/// required `functionResponse.name`.
fn find_name_for_id(messages: &[Message], target_id: &str) -> Option<String> {
    for m in messages.iter().rev() {
        for b in &m.content {
            if let ContentBlock::ToolUse { id, name, .. } = b {
                if id == target_id {
                    return Some(name.clone());
                }
            }
        }
    }
    None
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
    fn encode_tools_wraps_function_declarations_and_maps_type_enum() {
        let t = MockTool {
            n: "read",
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "examples": ["/a", "/b"] }
                },
                "required": ["path"]
            }),
        };
        let tools: Vec<&dyn Tool> = vec![&t];
        let encoded = GeminiAdapter.encode_tools(&tools);
        assert_eq!(encoded.len(), 1);
        let decls = encoded[0]["functionDeclarations"].as_array().unwrap();
        assert_eq!(decls[0]["name"], "read");
        let params = &decls[0]["parameters"];
        assert_eq!(params["type"], "OBJECT");
        assert_eq!(params["properties"]["path"]["type"], "STRING");
        // examples[0] → example (single value)
        assert_eq!(params["properties"]["path"]["example"], "/a");
        assert!(params["properties"]["path"].get("examples").is_none());
    }

    #[test]
    fn encode_messages_uses_model_role_and_functioncall_parts() {
        let msgs = vec![
            Message::system("you are alva"),
            Message::user("hi"),
            Message {
                id: "m".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/a"}),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
        ];
        let out = GeminiAdapter.encode_messages(&msgs);
        assert_eq!(out.system.as_deref(), Some("you are alva"));
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0]["role"], "user");
        assert_eq!(out.messages[1]["role"], "model");
        let fc = &out.messages[1]["parts"][0]["functionCall"];
        assert_eq!(fc["name"], "read");
        assert_eq!(fc["args"]["path"], "/a");
    }

    #[test]
    fn encode_tool_result_resolves_name_from_preceding_tool_use() {
        use crate::tool::execution::ToolContent;
        let msgs = vec![
            Message {
                id: "a".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({"path": "/a"}),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            Message {
                id: "b".into(),
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    id: "toolu_1".into(),
                    content: vec![ToolContent::text("file body")],
                    is_error: false,
                }],
                tool_call_id: Some("toolu_1".into()),
                usage: None,
                timestamp: 0,
            },
        ];
        let out = GeminiAdapter.encode_messages(&msgs);
        let fr = &out.messages[1]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "read");
        assert_eq!(fr["response"]["content"], "file body");
    }

    #[test]
    fn decode_response_parses_functioncall_and_generates_id() {
        let resp = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        { "text": "ok" },
                        { "functionCall": { "name": "read", "args": { "path": "/a" } } }
                    ]
                }
            }],
            "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 3, "totalTokenCount": 8 }
        });
        let decoded = GeminiAdapter.decode_response(&resp).unwrap();
        assert_eq!(decoded.message.content.len(), 2);
        match &decoded.message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert!(id.starts_with("toolu_"));
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
        assert_eq!(decoded.usage.unwrap().total_tokens, 8);
    }

    #[test]
    fn decode_stream_emits_tool_call_triple_from_one_event() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{ "functionCall": { "name": "read", "args": {"path": "/a"} } }]
                }
            }]
        });
        let out = GeminiAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 3);
        match &out[0] {
            StreamEvent::ToolCallStart { id, name } => {
                assert!(id.starts_with("toolu_"));
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCallStart"),
        }
        match &out[1] {
            StreamEvent::ToolCallDelta { arguments_delta, .. } => {
                let parsed: Value = serde_json::from_str(arguments_delta).unwrap();
                assert_eq!(parsed["path"], "/a");
            }
            _ => panic!("expected ToolCallDelta"),
        }
        match &out[2] {
            StreamEvent::ToolCallEnd { id } => {
                assert!(id.starts_with("toolu_"));
            }
            _ => panic!("expected ToolCallEnd"),
        }
    }
}
