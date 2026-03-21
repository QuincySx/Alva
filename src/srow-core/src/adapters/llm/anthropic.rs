// INPUT:  crate::ports::provider::*, crate::adapters::llm::http::*, async_trait, futures, reqwest, serde_json, base64
// OUTPUT: AnthropicLanguageModel
// POS:    Anthropic Messages API provider — direct HTTP, no rig-core. Implements LanguageModel trait.

use async_trait::async_trait;
use futures::{Stream, StreamExt, stream};
use std::pin::Pin;

use crate::adapters::llm::http::{SseEvent, parse_raw_sse, post_json_with_retry};
use crate::ports::provider::content::LanguageModelContent;
use crate::ports::provider::errors::ProviderError;
use crate::ports::provider::language_model::{
    FinishReason, LanguageModel, LanguageModelCallOptions, LanguageModelGenerateResult,
    LanguageModelStreamPart, LanguageModelStreamResult, LanguageModelUsage, ResponseMetadata,
    UnifiedFinishReason, UsageInputTokens, UsageOutputTokens,
};
use crate::ports::provider::prompt::{
    AssistantContentPart, DataContent, LanguageModelMessage, ToolContentPart, UserContentPart,
};
use crate::ports::provider::tool_types::{LanguageModelTool, ToolChoice, ToolResultOutput};

/// Anthropic Messages API language model provider.
///
/// Uses direct HTTP requests against the Anthropic Messages API (`/v1/messages`).
/// Supports both streaming and non-streaming generation, tool use, and extended thinking.
pub struct AnthropicLanguageModel {
    client: reqwest::Client,
    api_key: String,
    base_url: String,
    model_id: String,
}

impl AnthropicLanguageModel {
    /// Create a new Anthropic provider with the default base URL.
    pub fn new(api_key: impl Into<String>, model: impl Into<String>) -> Self {
        Self {
            client: reqwest::Client::new(),
            api_key: api_key.into(),
            base_url: "https://api.anthropic.com/v1".to_string(),
            model_id: model.into(),
        }
    }

    /// Create a new Anthropic provider with a custom base URL.
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

    /// Build HTTP headers for the Anthropic API request.
    fn build_headers(&self, options: &LanguageModelCallOptions) -> Vec<(String, String)> {
        let mut headers = vec![
            ("x-api-key".to_string(), self.api_key.clone()),
            ("anthropic-version".to_string(), "2023-06-01".to_string()),
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

    /// Build the request body JSON for the Anthropic Messages API.
    pub fn build_request_body(
        &self,
        options: &LanguageModelCallOptions,
        stream: bool,
    ) -> Result<serde_json::Value, ProviderError> {
        let mut body = serde_json::Map::new();

        body.insert("model".to_string(), serde_json::json!(self.model_id));

        // Extract thinking config from provider_options
        let thinking_config = self.extract_thinking_config(options);

        // max_tokens: if thinking is enabled, use budget_tokens + max_output_tokens
        let max_output = options.max_output_tokens.unwrap_or(8192);
        if let Some(ref thinking) = thinking_config {
            let budget = thinking
                .get("budget_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(10000) as u32;
            body.insert(
                "max_tokens".to_string(),
                serde_json::json!(budget + max_output),
            );
        } else {
            body.insert("max_tokens".to_string(), serde_json::json!(max_output));
        }

        // Extract system messages and build conversation messages
        let (system_blocks, messages) = self.convert_messages(&options.prompt)?;

        if !system_blocks.is_empty() {
            body.insert("system".to_string(), serde_json::Value::Array(system_blocks));
        }

        body.insert("messages".to_string(), serde_json::Value::Array(messages));

        // Tools
        if let Some(ref tool_choice) = options.tool_choice {
            // If tool_choice is None, don't send tools at all
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
            }
        } else if let Some(ref tools) = options.tools {
            // No explicit tool_choice but tools provided — default to auto
            let tools_json = self.convert_tools(tools);
            if !tools_json.is_empty() {
                body.insert(
                    "tools".to_string(),
                    serde_json::Value::Array(tools_json),
                );
                body.insert("tool_choice".to_string(), serde_json::json!({"type": "auto"}));
            }
        }

        // Optional parameters
        if let Some(temp) = options.temperature {
            body.insert("temperature".to_string(), serde_json::json!(temp));
        }
        if let Some(top_p) = options.top_p {
            body.insert("top_p".to_string(), serde_json::json!(top_p));
        }
        if let Some(top_k) = options.top_k {
            body.insert("top_k".to_string(), serde_json::json!(top_k));
        }
        if let Some(ref stop) = options.stop_sequences {
            if !stop.is_empty() {
                body.insert("stop_sequences".to_string(), serde_json::json!(stop));
            }
        }

        // Thinking support
        if let Some(thinking) = thinking_config {
            body.insert("thinking".to_string(), thinking);
        }

        // Stream flag
        if stream {
            body.insert("stream".to_string(), serde_json::Value::Bool(true));
        }

        Ok(serde_json::Value::Object(body))
    }

    /// Extract thinking configuration from provider_options.
    fn extract_thinking_config(
        &self,
        options: &LanguageModelCallOptions,
    ) -> Option<serde_json::Value> {
        if let Some(ref provider_opts) = options.provider_options {
            if let Some(anthropic_opts) = provider_opts.get("anthropic") {
                if let Some(thinking) = anthropic_opts.get("thinking") {
                    return Some(thinking.clone());
                }
            }
        }
        None
    }

    /// Convert prompt messages into Anthropic format.
    ///
    /// Returns (system_blocks, messages) where system messages are extracted to
    /// top-level content blocks and conversation messages follow strict
    /// user/assistant alternation (consecutive same-role messages are merged,
    /// tool messages become tool_result blocks in user messages).
    fn convert_messages(
        &self,
        prompt: &[LanguageModelMessage],
    ) -> Result<(Vec<serde_json::Value>, Vec<serde_json::Value>), ProviderError> {
        let mut system_blocks: Vec<serde_json::Value> = Vec::new();
        let mut raw_messages: Vec<(String, Vec<serde_json::Value>)> = Vec::new();

        for msg in prompt {
            match msg {
                LanguageModelMessage::System { content, .. } => {
                    system_blocks.push(serde_json::json!({"type": "text", "text": content}));
                }
                LanguageModelMessage::User { content, .. } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|part| self.convert_user_part(part))
                        .collect();
                    raw_messages.push(("user".to_string(), parts));
                }
                LanguageModelMessage::Assistant { content, .. } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .filter_map(|part| self.convert_assistant_part(part))
                        .collect();
                    if !parts.is_empty() {
                        raw_messages.push(("assistant".to_string(), parts));
                    }
                }
                LanguageModelMessage::Tool { content, .. } => {
                    let parts: Vec<serde_json::Value> = content
                        .iter()
                        .map(|part| self.convert_tool_part(part))
                        .collect();
                    // Tool messages become user messages with tool_result blocks
                    raw_messages.push(("user".to_string(), parts));
                }
            }
        }

        // Merge consecutive same-role messages
        let merged = self.merge_consecutive_messages(raw_messages);

        Ok((system_blocks, merged))
    }

    /// Merge consecutive messages with the same role into a single message.
    /// Anthropic requires strict user/assistant alternation.
    fn merge_consecutive_messages(
        &self,
        raw: Vec<(String, Vec<serde_json::Value>)>,
    ) -> Vec<serde_json::Value> {
        let mut result: Vec<serde_json::Value> = Vec::new();
        let mut current_role: Option<String> = None;
        let mut current_content: Vec<serde_json::Value> = Vec::new();

        for (role, parts) in raw {
            if current_role.as_ref() == Some(&role) {
                // Same role — merge content
                current_content.extend(parts);
            } else {
                // Different role — flush previous
                if let Some(ref prev_role) = current_role {
                    result.push(serde_json::json!({
                        "role": prev_role,
                        "content": current_content,
                    }));
                }
                current_role = Some(role);
                current_content = parts;
            }
        }

        // Flush last
        if let Some(ref role) = current_role {
            result.push(serde_json::json!({
                "role": role,
                "content": current_content,
            }));
        }

        result
    }

    /// Convert a user content part to Anthropic JSON format.
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
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": data,
                                }
                            })
                        }
                        DataContent::Url { url } => {
                            serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "url",
                                    "url": url,
                                }
                            })
                        }
                        DataContent::Bytes { data } => {
                            use base64::Engine;
                            let encoded =
                                base64::engine::general_purpose::STANDARD.encode(data);
                            serde_json::json!({
                                "type": "image",
                                "source": {
                                    "type": "base64",
                                    "media_type": media_type,
                                    "data": encoded,
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

    /// Convert an assistant content part to Anthropic JSON format.
    fn convert_assistant_part(&self, part: &AssistantContentPart) -> Option<serde_json::Value> {
        match part {
            AssistantContentPart::Text { text, .. } => {
                Some(serde_json::json!({"type": "text", "text": text}))
            }
            AssistantContentPart::ToolCall {
                tool_call_id,
                tool_name,
                input,
                ..
            } => Some(serde_json::json!({
                "type": "tool_use",
                "id": tool_call_id,
                "name": tool_name,
                "input": input,
            })),
            AssistantContentPart::Reasoning { text, .. } => {
                Some(serde_json::json!({
                    "type": "thinking",
                    "thinking": text,
                    "signature": "",
                }))
            }
            _ => None,
        }
    }

    /// Convert a tool content part to Anthropic JSON format.
    fn convert_tool_part(&self, part: &ToolContentPart) -> serde_json::Value {
        match part {
            ToolContentPart::ToolResult {
                tool_call_id,
                output,
                ..
            } => {
                let content = self.convert_tool_result_output(output);
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": tool_call_id,
                    "content": content,
                })
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
                serde_json::json!({
                    "type": "tool_result",
                    "tool_use_id": approval_id,
                    "content": text,
                })
            }
        }
    }

    /// Convert a tool result output to a string for the Anthropic API.
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
                // Flatten content items to text
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

    /// Convert tool definitions to Anthropic JSON format.
    fn convert_tools(&self, tools: &[LanguageModelTool]) -> Vec<serde_json::Value> {
        tools
            .iter()
            .filter_map(|tool| match tool {
                LanguageModelTool::Function(ft) => {
                    let mut obj = serde_json::Map::new();
                    obj.insert("name".to_string(), serde_json::json!(ft.name));
                    if let Some(ref desc) = ft.description {
                        obj.insert("description".to_string(), serde_json::json!(desc));
                    }
                    obj.insert("input_schema".to_string(), ft.input_schema.clone());
                    Some(serde_json::Value::Object(obj))
                }
                LanguageModelTool::Provider(_) => {
                    // Provider tools are not directly mappable to Anthropic function tools
                    None
                }
            })
            .collect()
    }

    /// Convert tool choice to Anthropic JSON format.
    fn convert_tool_choice(&self, choice: &ToolChoice) -> serde_json::Value {
        match choice {
            ToolChoice::Auto => serde_json::json!({"type": "auto"}),
            ToolChoice::Required => serde_json::json!({"type": "any"}),
            ToolChoice::Tool { tool_name } => {
                serde_json::json!({"type": "tool", "name": tool_name})
            }
            ToolChoice::None => {
                // Should not reach here since we filter None before calling this
                serde_json::json!({"type": "auto"})
            }
        }
    }

    /// Parse a non-streaming Anthropic response into LanguageModelGenerateResult.
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
                url: format!("{}/messages", self.base_url),
                status_code: None,
                response_body: Some(json.to_string()),
                is_retryable: false,
            });
        }

        let content_blocks = json
            .get("content")
            .and_then(|v| v.as_array())
            .ok_or_else(|| ProviderError::InvalidResponseData {
                message: "Missing 'content' array in response".to_string(),
            })?;

        let mut content: Vec<LanguageModelContent> = Vec::new();

        for block in content_blocks {
            let block_type = block.get("type").and_then(|v| v.as_str()).unwrap_or("");
            match block_type {
                "text" => {
                    let text = block
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    content.push(LanguageModelContent::Text {
                        text,
                        provider_metadata: None,
                    });
                }
                "thinking" => {
                    let text = block
                        .get("thinking")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    content.push(LanguageModelContent::Reasoning {
                        text,
                        provider_metadata: None,
                    });
                }
                "tool_use" => {
                    let id = block
                        .get("id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name = block
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let input = block
                        .get("input")
                        .map(|v| serde_json::to_string(v).unwrap_or_default())
                        .unwrap_or_else(|| "{}".to_string());
                    content.push(LanguageModelContent::ToolCall {
                        tool_call_id: id,
                        tool_name: name,
                        input,
                        provider_metadata: None,
                    });
                }
                _ => {
                    // Unknown block type — skip
                }
            }
        }

        // Parse stop_reason
        let stop_reason_raw = json
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");

        let unified = match stop_reason_raw {
            "end_turn" => UnifiedFinishReason::Stop,
            "tool_use" => UnifiedFinishReason::ToolCalls,
            "max_tokens" => UnifiedFinishReason::Length,
            "stop_sequence" => UnifiedFinishReason::Stop,
            _ => UnifiedFinishReason::Other,
        };

        // Parse usage
        let usage_json = json.get("usage");
        let input_tokens = usage_json
            .and_then(|u| u.get("input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let output_tokens = usage_json
            .and_then(|u| u.get("output_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let cache_creation = usage_json
            .and_then(|u| u.get("cache_creation_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);
        let cache_read = usage_json
            .and_then(|u| u.get("cache_read_input_tokens"))
            .and_then(|v| v.as_u64())
            .map(|v| v as u32);

        let usage = LanguageModelUsage {
            input_tokens: UsageInputTokens {
                total: input_tokens,
                no_cache: None,
                cache_read,
                cache_write: cache_creation,
            },
            output_tokens: UsageOutputTokens {
                total: output_tokens,
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
                raw: Some(stop_reason_raw.to_string()),
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
    pub fn sse_to_stream_parts(
        &self,
        sse_stream: Pin<Box<dyn Stream<Item = Result<SseEvent, ProviderError>> + Send>>,
    ) -> Pin<Box<dyn Stream<Item = LanguageModelStreamPart> + Send>> {
        let base_url = self.base_url.clone();
        let state = StreamState::new();

        let part_stream = stream::unfold(
            (sse_stream, state, base_url),
            |(mut sse_stream, mut state, base_url)| async move {
                loop {
                    match sse_stream.next().await {
                        Some(Ok(event)) => {
                            let parts =
                                process_sse_event(&event, &mut state, &base_url);
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
                            // Stream ended
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

/// The type of a content block being streamed.
#[derive(Debug, Clone, PartialEq)]
enum BlockType {
    Text,
    Thinking,
    ToolUse,
}

/// Tracks the state of a streaming content block.
#[derive(Debug, Clone)]
#[allow(dead_code)]
struct ActiveBlock {
    index: usize,
    block_type: BlockType,
    /// For tool_use blocks, the tool's ID from the API.
    tool_id: Option<String>,
    /// For tool_use blocks, the tool name.
    tool_name: Option<String>,
}

/// Internal state tracked during SSE stream processing.
struct StreamState {
    input_tokens: Option<u32>,
    output_tokens: Option<u32>,
    active_blocks: Vec<ActiveBlock>,
}

impl StreamState {
    fn new() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            active_blocks: Vec::new(),
        }
    }

    fn find_block(&self, index: usize) -> Option<&ActiveBlock> {
        self.active_blocks.iter().find(|b| b.index == index)
    }

    fn block_id(&self, index: usize) -> String {
        if let Some(block) = self.find_block(index) {
            match block.block_type {
                BlockType::ToolUse => block
                    .tool_id
                    .clone()
                    .unwrap_or_else(|| index.to_string()),
                _ => index.to_string(),
            }
        } else {
            index.to_string()
        }
    }
}

/// Process a single SSE event and return zero or more stream parts.
fn process_sse_event(
    event: &SseEvent,
    state: &mut StreamState,
    _base_url: &str,
) -> Vec<LanguageModelStreamPart> {
    let event_type = event.event.as_deref().unwrap_or("");

    // Parse the data JSON
    let data: serde_json::Value = match serde_json::from_str(&event.data) {
        Ok(v) => v,
        Err(_) => return Vec::new(),
    };

    match event_type {
        "message_start" => {
            // Extract input_tokens from message.usage
            if let Some(usage) = data
                .get("message")
                .and_then(|m| m.get("usage"))
            {
                state.input_tokens = usage
                    .get("input_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
            }
            Vec::new()
        }

        "content_block_start" => {
            let index = data
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let content_block = data.get("content_block");
            let block_type_str = content_block
                .and_then(|b| b.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match block_type_str {
                "text" => {
                    state.active_blocks.push(ActiveBlock {
                        index,
                        block_type: BlockType::Text,
                        tool_id: None,
                        tool_name: None,
                    });
                    vec![LanguageModelStreamPart::TextStart {
                        id: index.to_string(),
                    }]
                }
                "thinking" => {
                    state.active_blocks.push(ActiveBlock {
                        index,
                        block_type: BlockType::Thinking,
                        tool_id: None,
                        tool_name: None,
                    });
                    vec![LanguageModelStreamPart::ReasoningStart {
                        id: index.to_string(),
                    }]
                }
                "tool_use" => {
                    let tool_id = content_block
                        .and_then(|b| b.get("id"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let tool_name = content_block
                        .and_then(|b| b.get("name"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    state.active_blocks.push(ActiveBlock {
                        index,
                        block_type: BlockType::ToolUse,
                        tool_id: Some(tool_id.clone()),
                        tool_name: Some(tool_name.clone()),
                    });

                    vec![LanguageModelStreamPart::ToolInputStart {
                        id: tool_id,
                        tool_name,
                        title: None,
                    }]
                }
                _ => Vec::new(),
            }
        }

        "content_block_delta" => {
            let index = data
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let delta = data.get("delta");
            let delta_type = delta
                .and_then(|d| d.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            match delta_type {
                "text_delta" => {
                    let text = delta
                        .and_then(|d| d.get("text"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = state.block_id(index);
                    vec![LanguageModelStreamPart::TextDelta { id, delta: text }]
                }
                "thinking_delta" => {
                    let thinking = delta
                        .and_then(|d| d.get("thinking"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = state.block_id(index);
                    vec![LanguageModelStreamPart::ReasoningDelta {
                        id,
                        delta: thinking,
                    }]
                }
                "input_json_delta" => {
                    let partial_json = delta
                        .and_then(|d| d.get("partial_json"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let id = state.block_id(index);
                    vec![LanguageModelStreamPart::ToolInputDelta {
                        id,
                        delta: partial_json,
                    }]
                }
                _ => Vec::new(),
            }
        }

        "content_block_stop" => {
            let index = data
                .get("index")
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as usize;

            let id = state.block_id(index);
            let block_type = state
                .find_block(index)
                .map(|b| b.block_type.clone());

            // Remove the block from active blocks
            state.active_blocks.retain(|b| b.index != index);

            match block_type {
                Some(BlockType::Text) => {
                    vec![LanguageModelStreamPart::TextEnd { id }]
                }
                Some(BlockType::Thinking) => {
                    vec![LanguageModelStreamPart::ReasoningEnd { id }]
                }
                Some(BlockType::ToolUse) => {
                    vec![LanguageModelStreamPart::ToolInputEnd { id }]
                }
                None => Vec::new(),
            }
        }

        "message_delta" => {
            let stop_reason_raw = data
                .get("delta")
                .and_then(|d| d.get("stop_reason"))
                .and_then(|v| v.as_str())
                .unwrap_or("end_turn");

            let unified = match stop_reason_raw {
                "end_turn" => UnifiedFinishReason::Stop,
                "tool_use" => UnifiedFinishReason::ToolCalls,
                "max_tokens" => UnifiedFinishReason::Length,
                "stop_sequence" => UnifiedFinishReason::Stop,
                _ => UnifiedFinishReason::Other,
            };

            // Extract output tokens from usage
            if let Some(usage) = data.get("usage") {
                state.output_tokens = usage
                    .get("output_tokens")
                    .and_then(|v| v.as_u64())
                    .map(|v| v as u32);
            }

            vec![LanguageModelStreamPart::Finish {
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
                    raw: Some(stop_reason_raw.to_string()),
                },
                provider_metadata: None,
            }]
        }

        "message_stop" => {
            // Stream ends — no part to emit
            Vec::new()
        }

        "ping" => {
            // Heartbeat — skip
            Vec::new()
        }

        "error" => {
            let message = data
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|v| v.as_str())
                .or_else(|| data.get("message").and_then(|v| v.as_str()))
                .unwrap_or("Unknown streaming error");
            vec![LanguageModelStreamPart::Error {
                error: message.to_string(),
            }]
        }

        _ => Vec::new(),
    }
}

// ---------------------------------------------------------------------------
// LanguageModel trait implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for AnthropicLanguageModel {
    fn provider(&self) -> &str {
        "anthropic"
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
        let url = format!("{}/messages", self.base_url);

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
        let url = format!("{}/messages", self.base_url);

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
