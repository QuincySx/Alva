// INPUT:  super::{ToolAdapter, EncodedMessages, DecodedResponse, StreamDecodeState, AdapterError}
// OUTPUT: GeminiAdapter (Google Gemini / Vertex AI `generateContent` + `streamGenerateContent`)
// POS:    Most complex adapter — functionDeclarations wrapper, type enum mapping, examples→example,
//         function-call id preservation/fallback generation, non-streamed tool calls.

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
//! - Message roles: `user` / `model` (not `assistant` / `tool`)
//! - Tool calls: `parts: [{functionCall: {id, name, args}}]`
//! - Tool results: `parts: [{functionResponse: {name, response}}]` — matched
//!   by **name**, not id. The encoder walks back through `messages` to
//!   find the matching `ToolUse.name` for each `ToolResult.id`.
//! - Tool-use ids: current Gemini responses preserve ids; older responses
//!   without one get a generated normalized `toolu_*` id.
//! - Streaming: Gemini does NOT stream tool-call arguments incrementally.
//!   The entire `functionCall` arrives in one event. The adapter surfaces
//!   it as a single `ToolCallDelta` with the complete JSON in
//!   `arguments_delta`.

use serde_json::{Map, Value};

use super::{
    common::tool_id, AdapterError, DecodedResponse, EncodedMessages, ProtocolAdapter,
    StreamDecodeState,
};
use crate::content::ContentBlock;
use crate::message::{Message, MessageRole, UsageMetadata};
use crate::stream::{StopReason, StreamEvent};
use crate::tool_def::ToolDefinition;

#[derive(Debug, Default, Clone, Copy)]
pub struct GeminiAdapter;

impl GeminiAdapter {
    pub const fn new() -> Self {
        Self
    }
}

impl ProtocolAdapter for GeminiAdapter {
    fn provider(&self) -> &'static str {
        "gemini"
    }

    fn encode_tools(&self, tools: &[ToolDefinition]) -> Vec<Value> {
        if tools.is_empty() {
            return vec![];
        }
        let declarations: Vec<Value> = tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": &t.name,
                    "description": &t.description,
                    "parameters": convert_schema(&t.parameters),
                })
            })
            .collect();
        vec![serde_json::json!({ "functionDeclarations": declarations })]
    }

    fn encode_messages(&self, messages: &[Message]) -> EncodedMessages {
        // Gemini's systemInstruction is a single block — collect
        // segments and the provider's build_body joins them with
        // "\n\n". Gemini has its own automatic prefix-cache so we don't
        // need explicit cache markers here.
        let mut system_segments: Vec<String> = Vec::new();
        let mut contents: Vec<Value> = Vec::new();
        let mut pending_tool_result_parts: Vec<Value> = Vec::new();
        let mut pending_tool_image_parts: Vec<Value> = Vec::new();

        for m in messages {
            if matches!(m.role, MessageRole::Tool) {
                for b in &m.content {
                    if let ContentBlock::ToolResult { id, content, .. } = b {
                        let name =
                            find_name_for_id(messages, id).unwrap_or_else(|| "unknown".to_string());
                        let mut text_parts = Vec::new();
                        for part in content {
                            match part {
                                crate::tool_payload::ToolContent::Text { text } => {
                                    text_parts.push(text.clone());
                                }
                                crate::tool_payload::ToolContent::Image { data, media_type } => {
                                    pending_tool_image_parts
                                        .push(gemini_image_part(data, media_type));
                                }
                            }
                        }
                        pending_tool_result_parts.push(serde_json::json!({
                            "functionResponse": {
                                "id": tool_id::to_provider(id),
                                "name": name,
                                "response": { "content": text_parts.join("\n") },
                            }
                        }));
                    }
                }
                continue;
            }

            flush_function_responses(&mut contents, &mut pending_tool_result_parts);
            flush_gemini_tool_images(&mut contents, &mut pending_tool_image_parts);
            match m.role {
                MessageRole::System => {
                    let text = m.text_content();
                    if !text.is_empty() {
                        system_segments.push(text);
                    }
                }
                MessageRole::User => {
                    let parts = m
                        .content
                        .iter()
                        .filter_map(|block| match block {
                            ContentBlock::Text { text } if !text.is_empty() => {
                                Some(serde_json::json!({"text": text}))
                            }
                            ContentBlock::Image { data, media_type } => {
                                Some(gemini_image_part(data, media_type))
                            }
                            _ => None,
                        })
                        .collect::<Vec<_>>();
                    contents.push(serde_json::json!({
                        "role": "user",
                        "parts": parts,
                    }));
                }
                MessageRole::Assistant => {
                    let mut parts: Vec<Value> = Vec::new();
                    let thought_signatures = m
                        .content
                        .iter()
                        .filter_map(|block| {
                            let ContentBlock::ProviderState { provider, data } = block else {
                                return None;
                            };
                            if provider != "gemini" {
                                return None;
                            }
                            Some((
                                data.get("tool_call_id")?.as_str()?.to_string(),
                                data.get("thought_signature")?.as_str()?.to_string(),
                            ))
                        })
                        .collect::<std::collections::HashMap<_, _>>();
                    for b in &m.content {
                        match b {
                            ContentBlock::Text { text } if !text.is_empty() => {
                                parts.push(serde_json::json!({"text": text}));
                            }
                            ContentBlock::Image { data, media_type } => {
                                parts.push(gemini_image_part(data, media_type));
                            }
                            ContentBlock::ToolUse { id, name, input } => {
                                let mut part = serde_json::json!({
                                    "functionCall": {
                                        "id": tool_id::to_provider(id),
                                        "name": name,
                                        "args": input
                                    }
                                });
                                if let Some(signature) = thought_signatures.get(id) {
                                    part["thoughtSignature"] = Value::String(signature.clone());
                                }
                                parts.push(part);
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
                MessageRole::Tool => unreachable!("handled before match"),
            }
        }
        flush_function_responses(&mut contents, &mut pending_tool_result_parts);
        flush_gemini_tool_images(&mut contents, &mut pending_tool_image_parts);

        EncodedMessages {
            system_segments: if system_segments.is_empty() {
                None
            } else {
                Some(system_segments)
            },
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
                    blocks.push(ContentBlock::Text {
                        text: text.to_string(),
                    });
                }
            } else if let Some(fc) = part.get("functionCall") {
                let name = fc
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                let input = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
                let raw_id = fc.get("id").and_then(Value::as_str).unwrap_or("");
                let id = if raw_id.is_empty() {
                    tool_id::generate()
                } else {
                    tool_id::to_normalized(raw_id)
                };
                if let Some(signature) = part.get("thoughtSignature").and_then(Value::as_str) {
                    blocks.push(ContentBlock::ProviderState {
                        provider: "gemini".to_string(),
                        data: serde_json::json!({
                            "tool_call_id": &id,
                            "thought_signature": signature,
                        }),
                    });
                }
                blocks.push(ContentBlock::ToolUse { id, name, input });
            }
        }

        let usage = response.get("usageMetadata").map(|u| {
            let (cache_creation_input_tokens, cache_read_input_tokens) =
                super::common::cache_usage::extract_gemini(u);
            UsageMetadata {
                input_tokens: u
                    .get("promptTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                output_tokens: u
                    .get("candidatesTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
                total_tokens: u
                    .get("totalTokenCount")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as u32,
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
        // Gemini's streamGenerateContent returns the same shape as
        // generateContent, one message at a time. Tool calls arrive
        // fully-formed (no incremental JSON). We surface each functionCall
        // as a ToolCallDelta with the complete JSON in arguments_delta.
        let mut out = Vec::new();
        let candidates = event.get("candidates").and_then(Value::as_array);
        if let Some(candidates) = candidates {
            for candidate in candidates {
                if let Some(parts) = candidate
                    .pointer("/content/parts")
                    .and_then(Value::as_array)
                {
                    for part in parts {
                        if let Some(text) = part.get("text").and_then(Value::as_str) {
                            if !text.is_empty() {
                                out.push(StreamEvent::TextDelta {
                                    text: text.to_string(),
                                });
                            }
                        } else if let Some(fc) = part.get("functionCall") {
                            let name = fc
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("")
                                .to_string();
                            let args = fc.get("args").cloned().unwrap_or(Value::Object(Map::new()));
                            let args_str = args.to_string();
                            let raw_id = fc.get("id").and_then(Value::as_str).unwrap_or("");
                            let id = if raw_id.is_empty() {
                                tool_id::generate()
                            } else {
                                tool_id::to_normalized(raw_id)
                            };
                            if let Some(signature) =
                                part.get("thoughtSignature").and_then(Value::as_str)
                            {
                                out.push(StreamEvent::ProviderState {
                                    provider: "gemini".to_string(),
                                    data: serde_json::json!({
                                        "tool_call_id": &id,
                                        "thought_signature": signature,
                                    }),
                                });
                            }
                            state.tool_input_buf.insert(id.clone(), args_str.clone());
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
            let (cache_creation_input_tokens, cache_read_input_tokens) =
                super::common::cache_usage::extract_gemini(usage);
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
                cache_creation_input_tokens,
                cache_read_input_tokens,
            }));
        }
        // Map finishReason from any candidate that carries it.
        // Gemini values: STOP, MAX_TOKENS, SAFETY, RECITATION, LANGUAGE, OTHER, BLOCKLIST,
        // PROHIBITED_CONTENT, SPII, MALFORMED_FUNCTION_CALL, IMAGE_SAFETY, UNEXPECTED_TOOL_CALL.
        // We check every candidate and emit Stop on the first finishReason we encounter,
        // since streaming chunks deliver one candidate at a time and the last chunk carries it.
        if let Some(candidates) = event.get("candidates").and_then(Value::as_array) {
            for candidate in candidates {
                if let Some(finish_reason) = candidate.get("finishReason").and_then(Value::as_str) {
                    if !finish_reason.is_empty() {
                        let reason = match finish_reason {
                            "STOP" => StopReason::EndTurn,
                            "MAX_TOKENS" => StopReason::MaxTokens,
                            other => StopReason::Other(other.to_lowercase()),
                        };
                        out.push(StreamEvent::Stop { reason });
                        break;
                    }
                }
            }
        }
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

fn flush_function_responses(contents: &mut Vec<Value>, parts: &mut Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    contents.push(serde_json::json!({
        "role": "user",
        "parts": std::mem::take(parts),
    }));
}

fn flush_gemini_tool_images(contents: &mut Vec<Value>, parts: &mut Vec<Value>) {
    if parts.is_empty() {
        return;
    }
    contents.push(serde_json::json!({
        "role": "user",
        "parts": std::mem::take(parts),
    }));
}

fn gemini_image_part(data: &str, media_type: &str) -> Value {
    serde_json::json!({
        "inlineData": {
            "mimeType": media_type,
            "data": data,
        }
    })
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

    #[test]
    fn encode_tools_wraps_function_declarations_and_maps_type_enum() {
        let tools = vec![ToolDefinition {
            name: "read".into(),
            description: String::new(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "path": { "type": "string", "examples": ["/a", "/b"] }
                },
                "required": ["path"]
            }),
        }];
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
        assert_eq!(out.system_flat().as_deref(), Some("you are alva"));
        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[0]["role"], "user");
        assert_eq!(out.messages[1]["role"], "model");
        let fc = &out.messages[1]["parts"][0]["functionCall"];
        assert_eq!(fc["id"], "1");
        assert_eq!(fc["name"], "read");
        assert_eq!(fc["args"]["path"], "/a");
    }

    #[test]
    fn encode_tool_result_resolves_name_from_preceding_tool_use() {
        use crate::tool_payload::ToolContent;
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
        assert_eq!(
            out.messages[1]["role"], "user",
            "function responses belong to a user turn"
        );
        let fr = &out.messages[1]["parts"][0]["functionResponse"];
        assert_eq!(fr["name"], "read");
        assert_eq!(fr["response"]["content"], "file body");
    }

    #[test]
    fn encode_messages_groups_parallel_function_responses_in_one_user_turn() {
        use crate::tool_payload::ToolContent;
        let assistant = Message {
            id: "a".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::ToolUse {
                    id: "toolu_1".into(),
                    name: "read".into(),
                    input: serde_json::json!({}),
                },
                ContentBlock::ToolUse {
                    id: "toolu_2".into(),
                    name: "write".into(),
                    input: serde_json::json!({}),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let tool_message = |id: &str, text: &str| Message {
            id: format!("result-{id}"),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: id.into(),
                content: vec![ToolContent::text(text)],
                is_error: false,
            }],
            tool_call_id: Some(id.into()),
            usage: None,
            timestamp: 0,
        };
        let out = GeminiAdapter.encode_messages(&[
            assistant,
            tool_message("toolu_1", "one"),
            tool_message("toolu_2", "two"),
        ]);

        assert_eq!(out.messages.len(), 2);
        assert_eq!(out.messages[1]["role"], "user");
        assert_eq!(out.messages[1]["parts"].as_array().unwrap().len(), 2);
    }

    #[test]
    fn encode_messages_preserves_user_and_tool_images() {
        use crate::tool_payload::ToolContent;

        let user = Message {
            id: "u".into(),
            role: MessageRole::User,
            content: vec![
                ContentBlock::Text {
                    text: "describe".into(),
                },
                ContentBlock::Image {
                    data: "base64-user".into(),
                    media_type: "image/png".into(),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let assistant = Message {
            id: "a".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "toolu_call_image".into(),
                name: "screenshot".into(),
                input: serde_json::json!({}),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let tool = Message {
            id: "t".into(),
            role: MessageRole::Tool,
            content: vec![ContentBlock::ToolResult {
                id: "toolu_call_image".into(),
                content: vec![
                    ToolContent::text("captured"),
                    ToolContent::image("base64-tool", "image/jpeg"),
                ],
                is_error: false,
            }],
            tool_call_id: Some("toolu_call_image".into()),
            usage: None,
            timestamp: 0,
        };
        let out = GeminiAdapter.encode_messages(&[user, assistant, tool]);

        assert_eq!(
            out.messages[0]["parts"][1]["inlineData"]["data"],
            "base64-user"
        );
        assert_eq!(
            out.messages[2]["parts"][0]["functionResponse"]["name"],
            "screenshot"
        );
        assert_eq!(
            out.messages[3]["parts"][0]["inlineData"]["data"],
            "base64-tool"
        );
    }

    #[test]
    fn decode_response_preserves_functioncall_id() {
        let resp = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [
                        { "text": "ok" },
                        { "functionCall": {
                            "id": "call_read_1",
                            "name": "read",
                            "args": { "path": "/a" }
                        } }
                    ]
                }
            }],
            "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 3, "totalTokenCount": 8 }
        });
        let decoded = GeminiAdapter.decode_response(&resp).unwrap();
        assert_eq!(decoded.message.content.len(), 2);
        match &decoded.message.content[1] {
            ContentBlock::ToolUse { id, name, input } => {
                assert_eq!(id, "toolu_call_read_1");
                assert_eq!(name, "read");
                assert_eq!(input["path"], "/a");
            }
            _ => panic!("expected ToolUse"),
        }
        assert_eq!(decoded.usage.unwrap().total_tokens, 8);
    }

    #[test]
    fn thought_signature_survives_decode_and_reencode() {
        let resp = serde_json::json!({
            "candidates": [{
                "content": {
                    "role": "model",
                    "parts": [{
                        "thoughtSignature": "sig-abc",
                        "functionCall": {
                            "id": "call_read_1",
                            "name": "read",
                            "args": {"path": "/a"}
                        }
                    }]
                }
            }]
        });
        let decoded = GeminiAdapter.decode_response(&resp).unwrap();
        assert!(matches!(
            &decoded.message.content[0],
            ContentBlock::ProviderState { provider, data }
                if provider == "gemini"
                    && data["tool_call_id"] == "toolu_call_read_1"
                    && data["thought_signature"] == "sig-abc"
        ));

        let reencoded = GeminiAdapter.encode_messages(&[decoded.message]);
        assert_eq!(
            reencoded.messages[0]["parts"][0]["thoughtSignature"],
            "sig-abc"
        );
        assert_eq!(
            reencoded.messages[0]["parts"][0]["functionCall"]["id"],
            "call_read_1"
        );
    }

    #[test]
    fn stream_emits_thought_signature_state_before_tool_call() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "candidates": [{
                "content": {
                    "parts": [{
                        "thoughtSignature": "sig-stream",
                        "functionCall": {
                            "id": "call_stream",
                            "name": "read",
                            "args": {"path": "/a"}
                        }
                    }]
                }
            }]
        });
        let out = GeminiAdapter.decode_stream_event(&ev, &mut state).unwrap();

        assert!(matches!(
            &out[0],
            StreamEvent::ProviderState { provider, data }
                if provider == "gemini"
                    && data["tool_call_id"] == "toolu_call_stream"
                    && data["thought_signature"] == "sig-stream"
        ));
        assert!(matches!(
            &out[1],
            StreamEvent::ToolCallStart { id, .. } if id == "toolu_call_stream"
        ));
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
        // ToolCallStart + ToolCallDelta + ToolCallEnd — no finishReason in this event
        assert_eq!(out.len(), 3);
        match &out[0] {
            StreamEvent::ToolCallStart { id, name } => {
                assert!(id.starts_with("toolu_"));
                assert_eq!(name, "read");
            }
            _ => panic!("expected ToolCallStart"),
        }
        match &out[1] {
            StreamEvent::ToolCallDelta {
                arguments_delta, ..
            } => {
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

    #[test]
    fn decode_stream_finish_reason_stop_emits_end_turn() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "candidates": [{
                "content": { "parts": [{ "text": "done" }] },
                "finishReason": "STOP"
            }],
            "usageMetadata": { "promptTokenCount": 5, "candidatesTokenCount": 3, "totalTokenCount": 8 }
        });
        let out = GeminiAdapter.decode_stream_event(&ev, &mut state).unwrap();
        // TextDelta, Stop, Usage
        let stop = out.iter().find(|e| matches!(e, StreamEvent::Stop { .. }));
        assert!(stop.is_some(), "expected a Stop event");
        assert!(matches!(
            stop.unwrap(),
            StreamEvent::Stop {
                reason: StopReason::EndTurn
            }
        ));
    }

    #[test]
    fn decode_stream_finish_reason_max_tokens_emits_max_tokens() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "candidates": [{
                "content": { "parts": [] },
                "finishReason": "MAX_TOKENS"
            }]
        });
        let out = GeminiAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 1);
        assert!(matches!(
            &out[0],
            StreamEvent::Stop {
                reason: StopReason::MaxTokens
            }
        ));
    }

    #[test]
    fn decode_stream_finish_reason_safety_emits_other() {
        let mut state = StreamDecodeState::new();
        let ev = serde_json::json!({
            "candidates": [{
                "content": { "parts": [] },
                "finishReason": "SAFETY"
            }]
        });
        let out = GeminiAdapter.decode_stream_event(&ev, &mut state).unwrap();
        assert_eq!(out.len(), 1);
        match &out[0] {
            StreamEvent::Stop {
                reason: StopReason::Other(s),
            } => assert_eq!(s, "safety"),
            _ => panic!("expected Stop{{Other(\"safety\")}}"),
        }
    }
}
