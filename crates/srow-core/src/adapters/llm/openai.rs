// INPUT:  crate::ports::provider::*, crate::adapters::llm::http::*, async_trait, futures, reqwest, serde_json, base64
// OUTPUT: OpenAILanguageModel
// POS:    OpenAI Chat Completions API provider — direct HTTP, no rig-core. Implements LanguageModel trait.

use async_trait::async_trait;
use futures::{Stream, StreamExt, stream};
use std::collections::HashMap;
use std::pin::Pin;

use crate::adapters::llm::http::{SseEvent, parse_raw_sse, post_json_with_retry};
use crate::ports::provider::content::LanguageModelContent;
use crate::ports::provider::errors::ProviderError;
use crate::ports::provider::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelUsage, ResponseFormat,
    ResponseMetadata, UnifiedFinishReason, UsageInputTokens, UsageOutputTokens,
};
use crate::ports::provider::prompt::{
    AssistantContentPart, DataContent, LanguageModelMessage, ToolContentPart, UserContentPart,
};
use crate::ports::provider::tool_types::{LanguageModelTool, ToolChoice, ToolResultOutput};

/// OpenAI Chat Completions API language model provider.
///
/// Uses direct HTTP requests against the OpenAI Chat Completions API (`/chat/completions`).
/// Supports both streaming and non-streaming generation, tool use, and structured output.
pub struct OpenAILanguageModel {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model_id: String,
}

impl OpenAILanguageModel {
    /// Create a new OpenAI provider with the default base URL.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.openai.com/v1".to_string(),
            model_id: model.into(),
        }
    }

    /// Create a new OpenAI provider with a custom base URL.
    pub fn with_base_url(
        api_key: impl Into<String>,
        base_url: impl Into<String>,
        model: impl Into<String>,
    ) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: base_url.into(),
            model_id: model.into(),
        }
    }

    /// Build HTTP headers for the OpenAI API request.
    fn build_headers(&self, options: &LanguageModelCallOptions) -> Vec<(String, String)> {
        let mut headers = vec![
            (
                "authorization".to_string(),
                format!("Bearer {}", self.api_key),
            ),
            ("content-type".to_string(), "application/json".to_string()),
        ];

        // Merge any extra headers from options
        if let Some(ref extra) = options.headers {
            for (k, v) in extra {
                headers.push((k.clone(), v.clone()));
            }
        }

        headers
    }

    /// Build the request body JSON for the OpenAI Chat Completions API.
    pub fn build_request_body(
        &self,
        options: &LanguageModelCallOptions,
        stream: bool,
    ) -> Result<serde_json::Value, ProviderError> {
        let mut body = serde_json::Map::new();

        body.insert("model".to_string(), serde_json::json!(self.model_id));

        // Messages
        let messages = self.convert_messages(&options.prompt)?;
        body.insert("messages".to_string(), serde_json::Value::Array(messages));

        // max_tokens
        let max_output = options.max_output_tokens.unwrap_or(8192);
        body.insert("max_tokens".to_string(), serde_json::json!(max_output));

        // Tools
        if let Some(ref tool_choice) = options.tool_choice {
            if *tool_choice != ToolChoice::None {
                if let Some(ref tools) = options.tools {
                    let tools_json = self.convert_tools(tools);
                    if !tools_json.is_empty() {
                        body.insert(
                            "tools".to_string(),
                            serde_json::Value::Array(tools_json),
                        );
                        body.insert(
                            "tool_choice".to_string(),
                            self.convert_tool_choice(tool_choice),
                        );
                    }
                }
            } else {
                // ToolChoice::None → send "none"
                body.insert(
                    "tool_choice".to_string(),
                    serde_json::json!("none"),
                );
            }
        } else if let Some(ref tools) = options.tools {
            // No explicit tool_choice but tools provided — default to auto
            let tools_json = self.convert_tools(tools);
            if !tools_json.is_empty() {
                body.insert(
                    "tools".to_string(),
                    serde_json::Value::Array(tools_json),
                );
                body.insert("tool_choice".to_string(), serde_json::json!("auto"));
            }
        }

        // Optional parameters
        if let Some(temp) = options.temperature {
            body.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(top_p) = options.top_p {
            body.insert("top_p".to_string(), serde_json::json!(top_p));
        }
        if let Some(freq_penalty) = options.frequency_penalty {
            body.insert(
                "frequency_penalty".to_string(),
                serde_json::json!(freq_penalty),
            );
        }
        if let Some(pres_penalty) = options.presence_penalty {
            body.insert(
                "presence_penalty".to_string(),
                serde_json::json!(pres_penalty),
            );
        }
        if let Some(ref stop) = options.stop_sequences {
            if !stop.is_empty() {
                body.insert("stop".to_string(), serde_json::json!(stop));
            }
        }
        if let Some(seed) = options.seed {
            body.insert("seed".to_string(), serde_json::json!(seed));
        }

        // Response format
        if let Some(ref format) = options.response_format {
            body.insert(
                "response_format".to_string(),
                self.convert_response_format(format),
            );
        }

        // Stream flag
        if stream {
            body.insert("stream".to_string(), serde_json::Value::Bool(true));
            body.insert(
                "stream_options".to_string(),
                serde_json::json!({"include_usage": true}),
            );
        }

        Ok(serde_json::Value::Object(body))
    }

    /// Convert prompt messages into OpenAI Chat Completions format.
    fn convert_messages(
        &self,
        prompt: &[LanguageModelMessage],
    ) -> Result<Vec<serde_json::Value>, ProviderError> {
        let mut messages: Vec<serde_json::Value> = Vec::new();

        for msg in prompt {
            match msg {
                LanguageModelMessage::System { content, .. } => {
                    messages.push(serde_json::json!({
                        "role": "system",
                        "content": content,
                    }));
                }
                LanguageModelMessage::User { content, .. } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|part| self.convert_user_part(part))
                        .collect();
                    // If there is only one text part, simplify to a string
                    if parts.len() == 1 {
                        if let Some(text) = parts[0].get("text") {
                            if parts[0].get("type").and_then(|v| v.as_str()) == Some("text") {
                                messages.push(serde_json::json!({
                                    "role": "user",
                                    "content": text,
                                }));
                                continue;
                            }
                        }
                    }
                    messages.push(serde_json::json!({
                        "role": "user",
                        "content": parts,
                    }));
                }
                LanguageModelMessage::Assistant { content, .. } => {
                    // Separate text content from tool calls
                    let mut text_parts: Vec<String> = Vec::new();
                    let mut tool_calls: Vec<serde_json::Value> = Vec::new();

                    for part in content {
                        match part {
                            AssistantContentPart::Text { text, .. } => {
                                text_parts.push(text.clone());
                            }
                            AssistantContentPart::ToolCall {
                                tool_call_id,
                                tool_name,
                                input,
                                ..
                            } => {
                                let arguments = serde_json::to_string(input)
                                    .unwrap_or_else(|_| "{}".to_string());
                                tool_calls.push(serde_json::json!({
                                    "id": tool_call_id,
                                    "type": "function",
                                    "function": {
                                        "name": tool_name,
                                        "arguments": arguments,
                                    }
                                }));
                            }
                            _ => {}
                        }
                    }

                    let mut msg = serde_json::Map::new();
                    msg.insert("role".to_string(), serde_json::json!("assistant"));
                    if !text_parts.is_empty() {
                        msg.insert(
                            "content".to_string(),
                            serde_json::json!(text_parts.join("")),
                        );
                    }
                    if !tool_calls.is_empty() {
                        msg.insert(
                            "tool_calls".to_string(),
                            serde_json::Value::Array(tool_calls),
                        );
                    }
                    if msg.contains_key("content") || msg.contains_key("tool_calls") {
                        messages.push(serde_json::Value::Object(msg));
                    }
                }
                LanguageModelMessage::Tool { content, .. } => {
                    for part in content {
                        match part {
                            ToolContentPart::ToolResult {
                                tool_call_id,
                                output,
                                ..
                            } => {
                                let result_text = self.convert_tool_result_output(output);
                                messages.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": tool_call_id,
                                    "content": result_text,
                                }));
                            }
                            ToolContentPart::ToolApprovalResponse {
                                approval_id,
                                approved,
                                reason,
                                ..
                            } => {
                                let text = if *approved {
                                    format!("Approved: {}", approval_id)
                                } else {
                                    format!(
                                        "Denied: {}{}",
                                        approval_id,
                                        reason
                                            .as_ref()
                                            .map(|r| format!(" — {}", r))
                                            .unwrap_or_default()
                                    )
                                };
                                messages.push(serde_json::json!({
                                    "role": "tool",
                                    "tool_call_id": approval_id,
                                    "content": text,
                                }));
                            }
                        }
                    }
                }
            }
        }

        Ok(messages)
    }

    /// Convert a user content part to OpenAI JSON format.
    fn convert_user_part(&self, part: &UserContentPart) -> serde_json::Value {
        match part {
            UserContentPart::Text { text, .. } => {
                serde_json::json!({"type": "text", "text": text})
            }
            UserContentPart::File {
                data, media_type, ..
            } => {
                if media_type.starts_with("image/") {
                    match data {
                        DataContent::Base64 { data } => {
                            serde_json::json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", media_type, data),
                                }
                            })
                        }
                        DataContent::Url { url } => {
                            serde_json::json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": url,
                                }
                            })
                        }
                        DataContent::Bytes { data } => {
                            use base64::Engine;
                            let encoded =
                                base64::engine::general_purpose::STANDARD.encode(data);
                            serde_json::json!({
                                "type": "image_url",
                                "image_url": {
                                    "url": format!("data:{};base64,{}", media_type, encoded),
                                }
                            })
                        }
                    }
                } else {
                    // Non-image file — send as text description
                    serde_json::json!({"type": "text", "text": format!("[File: {}]", media_type)})
                }
            }
        }
    }

    /// Convert a tool result output to a string for the OpenAI API.
    fn convert_tool_result_output(&self, output: &ToolResultOutput) -> String {
        match output {
            ToolResultOutput::Text { value } => value.clone(),
            ToolResultOutput::Json { value } => serde_json::to_string(value).unwrap_or_default(),
            ToolResultOutput::ExecutionDenied { reason } => {
                format!(
                    "[Execution denied]{}",
                    reason
                        .as_ref()
                        .map(|r| format!(": {}", r))
                        .unwrap_or_default()
                )
            }
            ToolResultOutput::ErrorText { value } => format!("[Error] {}", value),
            ToolResultOutput::ErrorJson { value } => {
                format!("[Error] {}", serde_json::to_string(value).unwrap_or_default())
            }
            ToolResultOutput::Content { value } => {
                value
                    .iter()
                    .map(|item| match item {
                        crate::ports::provider::tool_types::ToolResultContentItem::Text {
                            text,
                        } => text.clone(),
                        _ => format!("{:?}", item),
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            }
        }
    }

    /// Convert tool definitions to OpenAI JSON format.
    ///
    /// OpenAI uses `parameters` (not `input_schema`).
    fn convert_tools(&self, tools: &[LanguageModelTool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .filter_map(|tool| match tool {
                LanguageModelTool::Function(ft) => {
                    let mut func = serde_json::Map::new();
                    func.insert("name".to_string(), serde_json::json!(ft.name));
                    if let Some(ref desc) = ft.description {
                        func.insert("description".to_string(), serde_json::json!(desc));
                    }
                    func.insert("parameters".to_string(), ft.input_schema.clone());
                    Some(serde_json::json!({
                        "type": "function",
                        "function": serde_json::Value::Object(func),
                    }))
                }
                LanguageModelTool::Provider(_) => {
                    // Provider tools are not directly mappable to OpenAI function tools
                    None
                }
            })
            .collect()
    }

    /// Convert tool choice to OpenAI JSON format.
    pub fn convert_tool_choice(&self, choice: &ToolChoice) -> serde_json::Value {
        match choice {
            ToolChoice::Auto => serde_json::json!("auto"),
            ToolChoice::None => serde_json::json!("none"),
            ToolChoice::Required => serde_json::json!("required"),
            ToolChoice::Tool { tool_name } => {
                serde_json::json!({"type": "function", "function": {"name": tool_name}})
            }
        }
    }

    /// Convert response format to OpenAI JSON format.
    fn convert_response_format(&self, format: &ResponseFormat) -> serde_json::Value {
        match format {
            ResponseFormat::Text => serde_json::json!({"type": "text"}),
            ResponseFormat::Json {
                schema,
                name,
                description: _,
            } => {
                if let Some(ref schema_value) = schema {
                    let schema_name = name.as_deref().unwrap_or("response");
                    serde_json::json!({
                        "type": "json_schema",
                        "json_schema": {
                            "name": schema_name,
                            "schema": schema_value,
                            "strict": true,
                        }
                    })
                } else {
                    serde_json::json!({"type": "json_object"})
                }
            }
        }
    }

    /// Parse a non-streaming OpenAI response into LanguageModelGenerateResult.
    pub fn parse_response(
        &self,
        json: &serde_json::Value,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
        // Check for API error response
        if let Some(error) = json.get("error") {
            let message = error
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("Unknown API error");
            return Err(ProviderError::ApiCall {
                message: message.to_string(),
                url: format!("{}/chat/completions", self.base_url),
                status_code: None,
                response_body: Some(json.to_string()),
                is_retryable: false,
            });
        }

        let choices = json
            .get("choices")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProviderError::InvalidResponseData {
                message: "Missing 'choices' array in response".to_string(),
            })?;

        let mut content: Vec<LanguageModelContent> = Vec::new();
        let mut finish_reason_raw = "stop".to_string();

        if let Some(choice) = choices.first() {
            // Extract finish_reason
            if let Some(fr) = choice.get("finish_reason").and_then(|v| v.as_str()) {
                finish_reason_raw = fr.to_string();
            }

            let message = choice.get("message");

            // Extract text content
            if let Some(text) = message
                .and_then(|m| m.get("content"))
                .and_then(|v| v.as_str())
            {
                if !text.is_empty() {
                    content.push(LanguageModelContent::Text {
                        text: text.to_string(),
                        provider_metadata: None,
                    });
                }
            }

            // Extract tool calls
            if let Some(tool_calls) = message
                .and_then(|m| m.get("tool_calls"))
                .and_then(|v| v.as_array())
            {
                for tc in tool_calls {
                    let id = tc
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = tc
                        .get("function")
                        .and_then(|f| f.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let arguments = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("{}")
                        .to_string();
                    content.push(LanguageModelContent::ToolCall {
                        tool_call_id: id,
                        tool_name: name,
                        input: arguments,
                        provider_metadata: None,
                    });
                }
            }
        }

        // Map finish_reason
        let unified = match finish_reason_raw.as_str() {
            "stop" => UnifiedFinishReason::Stop,
            "tool_calls" => UnifiedFinishReason::ToolCalls,
            "length" => UnifiedFinishReason::Length,
            "content_filter" => UnifiedFinishReason::ContentFilter,
            _ => UnifiedFinishReason::Other,
        };

        // Parse usage
        let usage_json = json.get("usage");
        let prompt_tokens = usage_json
            .and_then(|u| u.get("prompt_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let completion_tokens = usage_json
            .and_then(|u| u.get("completion_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let usage = LanguageModelUsage {
            input_tokens: UsageInputTokens {
                total: prompt_tokens,
                no_cache: None,
                cache_read: None,
                cache_write: None,
            },
            output_tokens: UsageOutputTokens {
                total: completion_tokens,
                text: None,
                reasoning: None,
            },
            raw: usage_json.cloned(),
        };

        // Response metadata
        let response_id = json.get("id").and_then(|v| v.as_str()).map(|s| s.to_string());
        let response_model = json
            .get("model")
            .and_then(|v| v.as_str())
            .map(|s| s.to_string());

        Ok(LanguageModelGenerateResult {
            content,
            finish_reason: FinishReason {
                unified,
                raw: Some(finish_reason_raw),
            },
            usage,
            provider_metadata: None,
            warnings: Vec::new(),
            response: Some(ResponseMetadata {
                id: response_id,
                timestamp: None,
                model_id: response_model,
                headers: None,
            }),
        })
    }

    /// Convert an SSE event stream into a stream of LanguageModelStreamParts.
    ///
    /// OpenAI SSE format uses data-only lines (no named events).
    /// Tool calls are streamed with `delta.tool_calls[i]` where each tool call
    /// has an `index` field and deltas are accumulated across multiple events.
    pub fn sse_to_stream_parts(
        &self,
        sse_stream: Pin<Box<dyn Stream<Item = Result<SseEvent, ProviderError>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = LanguageModelStreamPart> + Send>> {
        let base_url = self.base_url.clone();
        let state = OpenAIStreamState::new();

        let part_stream = stream::unfold(
            (sse_stream, state, base_url),
            |(mut sse_stream, mut state, base_url)| async move {
                loop {
                    match sse_stream.next().await {
                        Some(Ok(event)) => {
                            let parts =
                                process_openai_sse_event(&event, &mut state, &base_url);
                            if !parts.is_empty() {
                                return Some((parts, (sse_stream, state, base_url)));
                            }
                            // No parts produced — continue reading
                        }
                        Some(Err(e)) => {
                            let parts = vec![LanguageModelStreamPart::Error {
                                error: e.to_string(),
                            }];
                            return Some((parts, (sse_stream, state, base_url)));
                        }
                        None => {
                            // Stream ended — emit end events for any active parts
                            let parts = finalize_stream(&mut state);
                            if !parts.is_empty() {
                                return Some((parts, (sse_stream, state, base_url)));
                            }
                            return None;
                        }
                    }
                }
            },
        )
        .flat_map(|parts| stream::iter(parts));

        Box::pin(part_stream)
    }
}

// ---------------------------------------------------------------------------
// Streaming state
// ---------------------------------------------------------------------------

/// Tracks the state of an active tool call being streamed.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ActiveToolCall {
    /// The tool call ID from the API (e.g. "call_xxx").
    id: String,
    /// The tool name.
    name: String,
    /// Whether ToolInputStart has been emitted.
    started: bool,
}

/// Internal state tracked during OpenAI SSE stream processing.
struct OpenAIStreamState {
    /// Whether we are currently streaming text content.
    text_active: bool,
    /// Whether TextStart has been emitted.
    text_started: bool,
    /// A generated ID for the text block.
    text_id: String,
    /// Active tool calls keyed by index.
    active_tools: HashMap<u32, ActiveToolCall>,
    /// Accumulated input tokens.
    input_tokens: Option<u32>,
    /// Accumulated output tokens.
    output_tokens: Option<u32>,
    /// Whether we have already emitted a Finish event.
    finished: bool,
}

impl OpenAIStreamState {
    fn new() -> Self {
        Self {
            text_active: false,
            text_started: false,
            text_id: "text-0".to_string(),
            active_tools: HashMap::new(),
            input_tokens: None,
            output_tokens: None,
            finished: false,
        }
    }
}

/// Process a single OpenAI SSE event and return zero or more stream parts.
fn process_openai_sse_event(
    event: &SseEvent,
    state: &mut OpenAIStreamState,
    _base_url: &str,
) -> Vec<LanguageModelStreamPart> {
    // OpenAI SSE has no named events — parse the data JSON directly
    let data: serde_json::Value = match serde_json::from_str(&event.data) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    let mut parts: Vec<LanguageModelStreamPart> = Vec::new();

    let choices = data.get("choices").and_then(|v| v.as_array());

    if let Some(choices) = choices {
        for choice in choices {
            let delta = choice.get("delta");
            let finish_reason = choice.get("finish_reason").and_then(|v| v.as_str());

            // Process text content delta
            if let Some(content) = delta.and_then(|d| d.get("content")).and_then(|v| v.as_str()) {
                if !content.is_empty() {
                    if !state.text_started {
                        state.text_started = true;
                        state.text_active = true;
                        parts.push(LanguageModelStreamPart::TextStart {
                            id: state.text_id.clone(),
                        });
                    }
                    parts.push(LanguageModelStreamPart::TextDelta {
                        id: state.text_id.clone(),
                        delta: content.to_string(),
                    });
                }
            }

            // Process tool call deltas
            if let Some(tool_calls) = delta
                .and_then(|d| d.get("tool_calls"))
                .and_then(|v| v.as_array())
            {
                // If text was active, end it before starting tool calls
                if state.text_active {
                    state.text_active = false;
                    parts.push(LanguageModelStreamPart::TextEnd {
                        id: state.text_id.clone(),
                    });
                }

                for tc in tool_calls {
                    let index = tc
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0) as u32;

                    // Check if this is a new tool call (has `id` field)
                    if let Some(id) = tc.get("id").and_then(|v| v.as_str()) {
                        let name = tc
                            .get("function")
                            .and_then(|f| f.get("name"))
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();

                        state.active_tools.insert(
                            index,
                            ActiveToolCall {
                                id: id.to_string(),
                                name: name.clone(),
                                started: true,
                            },
                        );

                        parts.push(LanguageModelStreamPart::ToolInputStart {
                            id: id.to_string(),
                            tool_name: name,
                            title: None,
                        });
                    }

                    // Process arguments delta
                    if let Some(args) = tc
                        .get("function")
                        .and_then(|f| f.get("arguments"))
                        .and_then(|v| v.as_str())
                    {
                        if !args.is_empty() {
                            let tool_id = state
                                .active_tools
                                .get(&index)
                                .map(|t| t.id.clone())
                                .unwrap_or_else(|| format!("tool-{}", index));

                            parts.push(LanguageModelStreamPart::ToolInputDelta {
                                id: tool_id,
                                delta: args.to_string(),
                            });
                        }
                    }
                }
            }

            // Process finish_reason
            if let Some(fr) = finish_reason {
                if !fr.is_empty() && fr != "null" {
                    // End any active text
                    if state.text_active {
                        state.text_active = false;
                        parts.push(LanguageModelStreamPart::TextEnd {
                            id: state.text_id.clone(),
                        });
                    }

                    // End any active tool calls
                    let tool_indices: Vec<u32> = state.active_tools.keys().copied().collect();
                    for idx in tool_indices {
                        if let Some(tool) = state.active_tools.remove(&idx) {
                            parts.push(LanguageModelStreamPart::ToolInputEnd {
                                id: tool.id,
                            });
                        }
                    }

                    let unified = match fr {
                        "stop" => UnifiedFinishReason::Stop,
                        "tool_calls" => UnifiedFinishReason::ToolCalls,
                        "length" => UnifiedFinishReason::Length,
                        "content_filter" => UnifiedFinishReason::ContentFilter,
                        _ => UnifiedFinishReason::Other,
                    };

                    // Extract usage from this chunk if available
                    if let Some(usage) = data.get("usage") {
                        state.input_tokens = usage
                            .get("prompt_tokens")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32);
                        state.output_tokens = usage
                            .get("completion_tokens")
                            .and_then(|v| v.as_u64())
                            .map(|v| v as u32);
                    }

                    state.finished = true;
                    parts.push(LanguageModelStreamPart::Finish {
                        usage: LanguageModelUsage {
                            input_tokens: UsageInputTokens {
                                total: state.input_tokens,
                                no_cache: None,
                                cache_read: None,
                                cache_write: None,
                            },
                            output_tokens: UsageOutputTokens {
                                total: state.output_tokens,
                                text: None,
                                reasoning: None,
                            },
                            raw: None,
                        },
                        finish_reason: FinishReason {
                            unified,
                            raw: Some(fr.to_string()),
                        },
                        provider_metadata: None,
                    });
                }
            }
        }
    }

    // Handle usage in a chunk without choices (OpenAI sends usage in the last chunk
    // when stream_options.include_usage is true)
    if choices.is_none() || choices.map(|c| c.is_empty()).unwrap_or(false) {
        if let Some(usage) = data.get("usage") {
            state.input_tokens = usage
                .get("prompt_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .or(state.input_tokens);
            state.output_tokens = usage
                .get("completion_tokens")
                .and_then(|v| v.as_u64())
                .map(|v| v as u32)
                .or(state.output_tokens);
        }
    }

    parts
}

/// Finalize the stream — emit end events for any remaining active parts.
fn finalize_stream(state: &mut OpenAIStreamState) -> Vec<LanguageModelStreamPart> {
    let mut parts: Vec<LanguageModelStreamPart> = Vec::new();

    if state.text_active {
        state.text_active = false;
        parts.push(LanguageModelStreamPart::TextEnd {
            id: state.text_id.clone(),
        });
    }

    let tool_indices: Vec<u32> = state.active_tools.keys().copied().collect();
    for idx in tool_indices {
        if let Some(tool) = state.active_tools.remove(&idx) {
            parts.push(LanguageModelStreamPart::ToolInputEnd { id: tool.id });
        }
    }

    parts
}

// ---------------------------------------------------------------------------
// LanguageModel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for OpenAILanguageModel {
    fn provider(&self) -> &str {
        "openai"
    }

    fn model_id(&self) -> &str {
        &self.model_id
    }

    async fn do_generate(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelGenerateResult, ProviderError> {
        let body = self.build_request_body(&options, false)?;
        let headers = self.build_headers(&options);
        let url = format!("{}/chat/completions", self.base_url);

        let resp = post_json_with_retry(&self.client, &url, &headers, &body, 2).await?;
        let json: serde_json::Value = resp
            .json()
            .await
            .map_err(|e| ProviderError::Network(e.to_string()))?;

        self.parse_response(&json)
    }

    async fn do_stream(
        &self,
        options: LanguageModelCallOptions,
    ) -> Result<LanguageModelStreamResult, ProviderError> {
        let body = self.build_request_body(&options, true)?;
        let headers = self.build_headers(&options);
        let url = format!("{}/chat/completions", self.base_url);

        let resp = post_json_with_retry(&self.client, &url, &headers, &body, 2).await?;
        let byte_stream = resp.bytes_stream();
        let sse_stream = parse_raw_sse(byte_stream);
        let part_stream = self.sse_to_stream_parts(sse_stream);

        Ok(LanguageModelStreamResult {
            stream: part_stream,
            response: None,
        })
    }
}
