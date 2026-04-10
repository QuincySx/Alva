//! OpenAI Responses API provider.
//!
//! Implements `LanguageModel` by calling POST /v1/responses with the newer
//! Responses API format. Key differences from Chat Completions:
//! - System prompt goes in `instructions` field (not as a message)
//! - Input uses typed items: `message`, `function_call`, `function_call_output`
//! - Streaming uses named SSE events (`event:` + `data:` lines)
//! - Response output items have explicit types

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde::Deserialize;
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::base::message::{Message, MessageRole, UsageMetadata};
use alva_types::base::stream::StreamEvent;
use alva_types::model::{LanguageModel, ModelConfig};
use alva_types::tool::Tool;
use alva_types::ContentBlock;

use crate::config::ProviderConfig;

/// OpenAI Responses API provider.
pub struct OpenAIResponsesProvider {
    model: String,
    base_url: String,
    max_tokens: u32,
    /// Pre-resolved auth headers (from api_key or custom_headers at construction time).
    auth_headers: std::collections::HashMap<String, String>,
    client: Client,
}

impl OpenAIResponsesProvider {
    /// Create from config. Auth is resolved once here — api_key or custom_headers
    /// are converted to unified headers via `Bearer` scheme.
    pub fn new(config: ProviderConfig) -> Self {
        let auth_headers = crate::auth::resolve_auth_headers(
            &config.api_key, &config.custom_headers, crate::auth::AuthScheme::Bearer,
        );
        Self {
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            auth_headers,
            client: Client::new(),
        }
    }
}

// ---------------------------------------------------------------------------
// LanguageModel implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for OpenAIResponsesProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));

        let (instructions, input) = to_responses_input(messages);
        let resp_tools = to_responses_tools(tools);

        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);

        let mut body = serde_json::json!({
            "model": self.model,
            "input": input,
            "max_output_tokens": max_tokens,
        });

        if let Some(instructions) = instructions {
            body["instructions"] = serde_json::json!(instructions);
        }
        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !resp_tools.is_empty() {
            body["tools"] = serde_json::json!(resp_tools);
        }

        let span = tracing::info_span!("llm_request",
            provider = "openai_responses",
            model = %self.model,
            url = %url,
            messages = input.len(),
            tools = resp_tools.len(),
            stream = false,
        );
        let _guard = span.enter();

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::debug!(
            body_len = body_str.len(),
            body_preview = &body_str[..body_str.len().min(500)],
            "LLM request body"
        );

        let req = self.client.post(&url).header("Content-Type", "application/json");
        let req = crate::auth::apply_headers(req, &self.auth_headers);
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::LlmError(format!("HTTP request failed: {}", e)))?;

        let status = resp.status();
        let resp_text = resp
            .text()
            .await
            .map_err(|e| AgentError::LlmError(format!("read response body: {}", e)))?;

        tracing::debug!(
            status = %status,
            body_len = resp_text.len(),
            body_preview = &resp_text[..resp_text.len().min(500)],
            "LLM response"
        );

        if !status.is_success() {
            return Err(AgentError::LlmError(format!(
                "API returned {}: {}",
                status, resp_text
            )));
        }

        let responses_resp: ResponsesApiResponse = serde_json::from_str(&resp_text).map_err(
            |e| AgentError::LlmError(format!("parse response: {} — raw: {}", e, resp_text)),
        )?;

        from_responses_output(responses_resp)
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let auth_headers = self.auth_headers.clone();

        let (instructions, input) = to_responses_input(messages);
        let resp_tools = to_responses_tools(tools);

        let mut body = serde_json::json!({
            "model": model,
            "input": input,
            "max_output_tokens": max_tokens,
            "stream": true,
        });

        if let Some(instructions) = instructions {
            body["instructions"] = serde_json::json!(instructions);
        }
        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !resp_tools.is_empty() {
            body["tools"] = serde_json::json!(resp_tools);
        }

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "openai_responses",
            model = %model,
            url = %url,
            messages = input.len(),
            tools = resp_tools.len(),
            stream = true,
            body_len = body_str.len(),
            "LLM stream request"
        );
        tracing::debug!(
            body_preview = &body_str[..body_str.len().min(500)],
            "LLM stream request body"
        );

        Box::pin(async_stream::stream! {
            yield StreamEvent::Start;

            tracing::info!(provider = "openai_responses", "sending HTTP request, waiting for response...");
            let req_start = std::time::Instant::now();
            let req = client.post(&url).header("Content-Type", "application/json");
            let req = crate::auth::apply_headers(req, &auth_headers);
            let resp = match req
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    tracing::error!(duration_ms = req_start.elapsed().as_millis() as u64, error = %e, "HTTP request failed");
                    yield StreamEvent::Error(format!("HTTP request failed: {}", e));
                    return;
                }
            };
            tracing::info!(provider = "openai_responses", status = %resp.status(), duration_ms = req_start.elapsed().as_millis() as u64, "HTTP response received");

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                yield StreamEvent::Error(format!("API returned {}: {}", status, body));
                return;
            }

            // Read SSE lines from the byte stream.
            // The Responses API uses named events:
            //   event: response.output_text.delta
            //   data: {"delta": "Hello", ...}
            let mut byte_stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut current_event_type = String::new();

            while let Some(chunk) = futures::StreamExt::next(&mut byte_stream).await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield StreamEvent::Error(format!("stream read error: {}", e));
                        return;
                    }
                };

                buffer.push_str(&String::from_utf8_lossy(&chunk));

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    // Track the event type from "event:" lines
                    if let Some(event_name) = line.strip_prefix("event: ") {
                        current_event_type = event_name.trim().to_string();
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        let event_type = current_event_type.as_str();
                        match event_type {
                            "response.output_text.delta" => {
                                match serde_json::from_str::<ResponsesTextDelta>(data) {
                                    Ok(delta) => {
                                        if !delta.delta.is_empty() {
                                            yield StreamEvent::TextDelta { text: delta.delta };
                                        }
                                    }
                                    Err(e) => tracing::warn!(error = %e, event_type, data = &data[..data.len().min(200)], "failed to parse SSE chunk"),
                                }
                            }
                            "response.function_call_arguments.delta" => {
                                match serde_json::from_str::<ResponsesFunctionCallDelta>(data) {
                                    Ok(delta) => {
                                        yield StreamEvent::ToolCallDelta {
                                            id: delta.call_id.unwrap_or_default(),
                                            name: delta.name,
                                            arguments_delta: delta.delta,
                                        };
                                    }
                                    Err(e) => tracing::warn!(error = %e, event_type, data = &data[..data.len().min(200)], "failed to parse SSE chunk"),
                                }
                            }
                            "response.completed" => {
                                match serde_json::from_str::<ResponsesCompletedEvent>(data) {
                                    Ok(completed) => {
                                        if let Some(usage) = completed.response.usage {
                                            yield StreamEvent::Usage(UsageMetadata {
                                                input_tokens: usage.input_tokens,
                                                output_tokens: usage.output_tokens,
                                                total_tokens: usage.total_tokens,
                                            });
                                        }
                                    }
                                    Err(e) => tracing::warn!(error = %e, event_type, data = &data[..data.len().min(200)], "failed to parse SSE chunk"),
                                }
                                yield StreamEvent::Done;
                                return;
                            }
                            _ => {
                                // Other event types (response.created, response.output_item.added, etc.)
                            }
                        }

                        // Reset event type after processing data
                        current_event_type.clear();
                    }
                }
            }

            yield StreamEvent::Done;
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// Responses API types (response — non-streaming)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ResponsesApiResponse {
    #[serde(default)]
    output: Vec<ResponsesOutputItem>,
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

#[derive(Deserialize)]
struct ResponsesOutputItem {
    #[serde(rename = "type")]
    item_type: String,
    /// For "message" items: the content array
    #[serde(default)]
    content: Option<Vec<ResponsesContentPart>>,
    /// For "function_call" items: the call ID
    #[serde(default)]
    call_id: Option<String>,
    /// For "function_call" items: the function name
    #[serde(default)]
    name: Option<String>,
    /// For "function_call" items: the arguments JSON string
    #[serde(default)]
    arguments: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesContentPart {
    #[serde(rename = "type")]
    part_type: String,
    #[serde(default)]
    text: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesUsage {
    input_tokens: u32,
    output_tokens: u32,
    total_tokens: u32,
}

// ---------------------------------------------------------------------------
// Responses API types (streaming)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct ResponsesTextDelta {
    delta: String,
}

#[derive(Deserialize)]
struct ResponsesFunctionCallDelta {
    delta: String,
    #[serde(default)]
    call_id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct ResponsesCompletedEvent {
    response: ResponsesCompletedResponse,
}

#[derive(Deserialize)]
struct ResponsesCompletedResponse {
    #[serde(default)]
    usage: Option<ResponsesUsage>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Extract system prompt and convert messages to Responses API input items.
///
/// System messages are collected into an `instructions` string (returned separately).
/// Other messages are converted to typed input items.
fn to_responses_input(messages: &[Message]) -> (Option<String>, Vec<Value>) {
    let mut instructions: Option<String> = None;
    let mut input = Vec::new();

    for m in messages {
        match m.role {
            MessageRole::System => {
                let text = m.text_content();
                if !text.is_empty() {
                    instructions = Some(match instructions {
                        Some(existing) => format!("{}\n\n{}", existing, text),
                        None => text,
                    });
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
                // First emit any text as a message
                let text = m.text_content();
                if !text.is_empty() {
                    input.push(serde_json::json!({
                        "type": "message",
                        "role": "assistant",
                        "content": text,
                    }));
                }

                // Then emit each tool call as a separate function_call item
                for block in &m.content {
                    if let ContentBlock::ToolUse { id, name, input: args } = block {
                        input.push(serde_json::json!({
                            "type": "function_call",
                            "call_id": id,
                            "name": name,
                            "arguments": args.to_string(),
                        }));
                    }
                }
            }
            MessageRole::Tool => {
                // Tool result → function_call_output
                // Try to get content from ToolResult blocks first, fall back to text
                let mut parts: Vec<String> = Vec::new();
                let mut call_id = m.tool_call_id.clone();

                for block in &m.content {
                    if let ContentBlock::ToolResult {
                        id, content, ..
                    } = block
                    {
                        // Use the ToolResult's id as the call_id
                        if call_id.is_none() {
                            call_id = Some(id.clone());
                        }
                        for tc in content {
                            parts.push(tc.to_model_string());
                        }
                    } else if let Some(text) = block.as_text() {
                        parts.push(text.to_string());
                    }
                }

                let output = parts.join("\n");
                let call_id = call_id.unwrap_or_else(|| "unknown".to_string());

                input.push(serde_json::json!({
                    "type": "function_call_output",
                    "call_id": call_id,
                    "output": output,
                }));
            }
        }
    }

    (instructions, input)
}

/// Convert tools to Responses API format (same structure as Chat Completions).
fn to_responses_tools(tools: &[&dyn Tool]) -> Vec<Value> {
    tools
        .iter()
        .map(|t| {
            serde_json::json!({
                "type": "function",
                "name": t.name(),
                "description": t.description(),
                "parameters": t.parameters_schema(),
            })
        })
        .collect()
}

/// Convert a Responses API response into an internal Message.
fn from_responses_output(resp: ResponsesApiResponse) -> Result<Message, AgentError> {
    let mut content_blocks = Vec::new();

    for item in resp.output {
        match item.item_type.as_str() {
            "message" => {
                if let Some(content_parts) = item.content {
                    for part in content_parts {
                        match part.part_type.as_str() {
                            "output_text" | "text" => {
                                if let Some(text) = part.text {
                                    if !text.is_empty() {
                                        content_blocks.push(ContentBlock::Text { text });
                                    }
                                }
                            }
                            _ => {
                                tracing::debug!(
                                    part_type = %part.part_type,
                                    "unknown Responses content part type"
                                );
                            }
                        }
                    }
                }
            }
            "function_call" => {
                let id = item.call_id.unwrap_or_default();
                let name = item.name.unwrap_or_default();
                let input: Value = item
                    .arguments
                    .as_deref()
                    .and_then(|a| serde_json::from_str(a).ok())
                    .unwrap_or(Value::Object(serde_json::Map::new()));
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
            _ => {
                tracing::debug!(
                    item_type = %item.item_type,
                    "unknown Responses output item type"
                );
            }
        }
    }

    let usage = resp.usage.map(|u| UsageMetadata {
        input_tokens: u.input_tokens,
        output_tokens: u.output_tokens,
        total_tokens: u.total_tokens,
    });

    Ok(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::Assistant,
        content: content_blocks,
        tool_call_id: None,
        usage,
        timestamp: chrono::Utc::now().timestamp_millis(),
    })
}
