//! Direct Anthropic Messages API provider.
//!
//! Implements `LanguageModel` by calling Anthropic's `/v1/messages` endpoint
//! directly (not via OpenAI-compatible proxy). Supports:
//! - Native Anthropic message format
//! - System prompt as separate parameter
//! - Tool use blocks (Claude format)
//! - Thinking blocks
//! - Streaming via SSE
//! - Token counting from response usage
//! - Rate limit header tracking

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::base::message::{Message, MessageRole, UsageMetadata};
use alva_types::base::stream::StreamEvent;
use alva_types::model::{LanguageModel, ModelConfig};
use alva_types::tool::Tool;
use alva_types::ContentBlock;

use crate::config::ProviderConfig;
use crate::rate_limit::RateLimitState;

/// Default Anthropic API base URL.
const DEFAULT_ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
/// Current Anthropic API version header value.
const ANTHROPIC_API_VERSION: &str = "2023-06-01";

/// Anthropic Messages API provider.
pub struct AnthropicProvider {
    model: String,
    base_url: String,
    max_tokens: u32,
    /// Pre-resolved auth headers (from api_key or custom_headers at construction time).
    auth_headers: std::collections::HashMap<String, String>,
    client: Client,
    /// Shared rate limit state for tracking API usage.
    pub rate_limit: Arc<RateLimitState>,
}

impl AnthropicProvider {
    /// Create from config. Auth is resolved once here — api_key or custom_headers
    /// are converted to unified headers via `XApiKey` scheme.
    pub fn new(config: ProviderConfig) -> Self {
        let auth_headers = crate::auth::resolve_auth_headers(
            &config.api_key, &config.custom_headers, crate::auth::AuthScheme::XApiKey,
        );
        Self {
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            auth_headers,
            client: Client::new(),
            rate_limit: Arc::new(RateLimitState::new()),
        }
    }

    /// Create with an existing rate limit state (for sharing across providers).
    pub fn with_rate_limit(mut self, rate_limit: Arc<RateLimitState>) -> Self {
        self.rate_limit = rate_limit;
        self
    }
}

// ---------------------------------------------------------------------------
// LanguageModel implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for AnthropicProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        let url = format!(
            "{}/v1/messages",
            self.base_url.trim_end_matches('/')
        );

        // Record request for rate limiting
        let _rate_check = self.rate_limit.record_request();

        let (system_prompt, api_messages) = to_anthropic_messages(messages);
        let api_tools = to_anthropic_tools(tools);

        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": api_messages,
            "max_tokens": max_tokens,
        });

        if let Some(system) = system_prompt {
            body["system"] = serde_json::json!(system);
        }
        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !config.stop_sequences.is_empty() {
            body["stop_sequences"] = serde_json::json!(config.stop_sequences);
        }
        if !api_tools.is_empty() {
            body["tools"] = serde_json::json!(api_tools);
        }

        let span = tracing::info_span!("llm_request",
            provider = "anthropic",
            model = %self.model,
            url = %url,
            messages = api_messages.len(),
            tools = api_tools.len(),
            stream = false,
        );
        let _guard = span.enter();

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::debug!(
            body_len = body_str.len(),
            body_preview = &body_str[..body_str.len().min(500)],
            "LLM request body"
        );

        let req = self.client.post(&url)
            .header("anthropic-version", ANTHROPIC_API_VERSION)
            .header("Content-Type", "application/json");
        let req = crate::auth::apply_headers(req, &self.auth_headers);
        let resp = req
            .json(&body)
            .send()
            .await
            .map_err(|e| AgentError::LlmError(format!("HTTP request failed: {}", e)))?;

        let status = resp.status();

        // Extract rate limit headers before consuming body
        let rate_headers: Vec<(String, String)> = resp
            .headers()
            .iter()
            .filter(|(k, _)| {
                let name = k.as_str().to_lowercase();
                name.starts_with("x-ratelimit") || name == "retry-after"
            })
            .filter_map(|(k, v)| {
                v.to_str()
                    .ok()
                    .map(|val| (k.as_str().to_string(), val.to_string()))
            })
            .collect();

        self.rate_limit.update_from_headers(&rate_headers);

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
                "Anthropic API returned {}: {}",
                status, resp_text
            )));
        }

        let api_resp: AnthropicResponse = serde_json::from_str(&resp_text).map_err(|e| {
            AgentError::LlmError(format!("parse response: {} -- raw: {}", e, resp_text))
        })?;

        from_anthropic_response(api_resp)
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let url = format!(
            "{}/v1/messages",
            self.base_url.trim_end_matches('/')
        );
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let rate_limit = self.rate_limit.clone();
        let auth_headers = self.auth_headers.clone();

        let (system_prompt, api_messages) = to_anthropic_messages(messages);
        let api_tools = to_anthropic_tools(tools);

        let mut body = serde_json::json!({
            "model": model,
            "messages": api_messages,
            "max_tokens": max_tokens,
            "stream": true,
        });

        if let Some(system) = system_prompt {
            body["system"] = serde_json::json!(system);
        }
        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !config.stop_sequences.is_empty() {
            body["stop_sequences"] = serde_json::json!(config.stop_sequences);
        }
        if !api_tools.is_empty() {
            body["tools"] = serde_json::json!(api_tools);
        }

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "anthropic",
            model = %model,
            url = %url,
            messages = api_messages.len(),
            tools = api_tools.len(),
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

            // Record request for rate limiting
            let _rate_check = rate_limit.record_request();

            let req = client.post(&url)
                .header("anthropic-version", ANTHROPIC_API_VERSION)
                .header("Content-Type", "application/json");
            let req = crate::auth::apply_headers(req, &auth_headers);
            let resp = match req
                .json(&body)
                .send()
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    yield StreamEvent::Error(format!("HTTP request failed: {}", e));
                    return;
                }
            };

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                yield StreamEvent::Error(format!("Anthropic API returned {}: {}", status, body));
                return;
            }

            // Read SSE lines from the byte stream
            let mut byte_stream = resp.bytes_stream();
            let mut buffer = String::new();

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

                    if let Some(data) = line.strip_prefix("data: ") {
                        let parsed = serde_json::from_str::<AnthropicStreamEvent>(data);
                        if let Err(ref e) = parsed {
                            tracing::warn!(
                                error = %e,
                                data = &data[..data.len().min(200)],
                                "failed to parse Anthropic SSE chunk, skipping"
                            );
                        }
                        if let Ok(event) = parsed {
                            match event.event_type.as_str() {
                                "content_block_delta" => {
                                    if let Some(delta) = event.delta {
                                        match delta.delta_type.as_deref() {
                                            Some("text_delta") => {
                                                if let Some(text) = delta.text {
                                                    if !text.is_empty() {
                                                        yield StreamEvent::TextDelta { text };
                                                    }
                                                }
                                            }
                                            Some("input_json_delta") => {
                                                if let Some(partial) = delta.partial_json {
                                                    yield StreamEvent::ToolCallDelta {
                                                        id: String::new(),
                                                        name: None,
                                                        arguments_delta: partial,
                                                    };
                                                }
                                            }
                                            Some("thinking") => {
                                                if let Some(thinking) = delta.thinking {
                                                    yield StreamEvent::TextDelta { text: thinking };
                                                }
                                            }
                                            _ => {}
                                        }
                                    }
                                }
                                "content_block_start" => {
                                    if let Some(content_block) = event.content_block {
                                        if content_block.block_type == "tool_use" {
                                            yield StreamEvent::ToolCallDelta {
                                                id: content_block.id.unwrap_or_default(),
                                                name: content_block.name,
                                                arguments_delta: String::new(),
                                            };
                                        }
                                    }
                                }
                                "message_delta" => {
                                    if let Some(usage) = event.usage {
                                        yield StreamEvent::Usage(UsageMetadata {
                                            input_tokens: usage.input_tokens.unwrap_or(0),
                                            output_tokens: usage.output_tokens.unwrap_or(0),
                                            total_tokens: usage.input_tokens.unwrap_or(0)
                                                + usage.output_tokens.unwrap_or(0),
                                        });
                                    }
                                }
                                "message_stop" => {
                                    yield StreamEvent::Done;
                                    return;
                                }
                                "message_start" => {
                                    if let Some(message) = event.message {
                                        if let Some(usage) = message.usage {
                                            yield StreamEvent::Usage(UsageMetadata {
                                                input_tokens: usage.input_tokens.unwrap_or(0),
                                                output_tokens: usage.output_tokens.unwrap_or(0),
                                                total_tokens: usage.input_tokens.unwrap_or(0)
                                                    + usage.output_tokens.unwrap_or(0),
                                            });
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
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
// Anthropic API types (request)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct AnthropicMessage {
    role: String,
    content: Value,
}

#[derive(Serialize)]
struct AnthropicToolDef {
    name: String,
    description: String,
    input_schema: Value,
}

// ---------------------------------------------------------------------------
// Anthropic API types (response)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AnthropicResponse {
    content: Vec<AnthropicContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

#[derive(Deserialize)]
struct AnthropicContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    input: Option<Value>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicUsage {
    #[serde(default)]
    input_tokens: Option<u32>,
    #[serde(default)]
    output_tokens: Option<u32>,
}

// ---------------------------------------------------------------------------
// Anthropic API types (streaming)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct AnthropicStreamEvent {
    #[serde(rename = "type")]
    event_type: String,
    #[serde(default)]
    delta: Option<AnthropicStreamDelta>,
    #[serde(default)]
    content_block: Option<AnthropicStreamContentBlock>,
    #[serde(default)]
    usage: Option<AnthropicUsage>,
    #[serde(default)]
    message: Option<AnthropicStreamMessage>,
}

#[derive(Deserialize)]
struct AnthropicStreamDelta {
    #[serde(rename = "type")]
    delta_type: Option<String>,
    #[serde(default)]
    text: Option<String>,
    #[serde(default)]
    partial_json: Option<String>,
    #[serde(default)]
    thinking: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamContentBlock {
    #[serde(rename = "type")]
    block_type: String,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
}

#[derive(Deserialize)]
struct AnthropicStreamMessage {
    #[serde(default)]
    usage: Option<AnthropicUsage>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

/// Extract system prompt (if any) and convert messages to Anthropic format.
///
/// Anthropic API takes system prompt as a separate top-level parameter,
/// not as a message with role "system".
fn to_anthropic_messages(messages: &[Message]) -> (Option<String>, Vec<AnthropicMessage>) {
    let mut system_prompt = None;
    let mut api_messages = Vec::new();

    for m in messages {
        match m.role {
            MessageRole::System => {
                // Collect system messages into a single system prompt
                let text = m.text_content();
                if !text.is_empty() {
                    system_prompt = Some(match system_prompt {
                        Some(existing) => format!("{}\n\n{}", existing, text),
                        None => text,
                    });
                }
            }
            MessageRole::User => {
                api_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: Value::String(m.text_content()),
                });
            }
            MessageRole::Assistant => {
                let mut content_blocks = Vec::new();

                for block in &m.content {
                    match block {
                        ContentBlock::Text { text } => {
                            content_blocks.push(serde_json::json!({
                                "type": "text",
                                "text": text,
                            }));
                        }
                        ContentBlock::ToolUse { id, name, input } => {
                            content_blocks.push(serde_json::json!({
                                "type": "tool_use",
                                "id": id,
                                "name": name,
                                "input": input,
                            }));
                        }
                        _ => {
                            if let Some(text) = block.as_text() {
                                content_blocks.push(serde_json::json!({
                                    "type": "text",
                                    "text": text,
                                }));
                            }
                        }
                    }
                }

                if content_blocks.is_empty() {
                    let text = m.text_content();
                    if !text.is_empty() {
                        content_blocks.push(serde_json::json!({
                            "type": "text",
                            "text": text,
                        }));
                    }
                }

                api_messages.push(AnthropicMessage {
                    role: "assistant".to_string(),
                    content: Value::Array(content_blocks),
                });
            }
            MessageRole::Tool => {
                // Tool results in Anthropic format
                let mut tool_results = Vec::new();

                for block in &m.content {
                    if let ContentBlock::ToolResult {
                        id,
                        content,
                        is_error,
                    } = block
                    {
                        let text_content: Vec<String> =
                            content.iter().map(|tc| tc.to_model_string()).collect();

                        tool_results.push(serde_json::json!({
                            "type": "tool_result",
                            "tool_use_id": id,
                            "content": text_content.join("\n"),
                            "is_error": is_error,
                        }));
                    }
                }

                if tool_results.is_empty() {
                    // Fallback: plain text tool result
                    let text = m.text_content();
                    let tool_use_id = m.tool_call_id.as_deref().unwrap_or("unknown");
                    tool_results.push(serde_json::json!({
                        "type": "tool_result",
                        "tool_use_id": tool_use_id,
                        "content": text,
                    }));
                }

                api_messages.push(AnthropicMessage {
                    role: "user".to_string(),
                    content: Value::Array(tool_results),
                });
            }
        }
    }

    (system_prompt, api_messages)
}

fn to_anthropic_tools(tools: &[&dyn Tool]) -> Vec<AnthropicToolDef> {
    tools
        .iter()
        .map(|t| AnthropicToolDef {
            name: t.name().to_string(),
            description: t.description().to_string(),
            input_schema: t.parameters_schema(),
        })
        .collect()
}

fn from_anthropic_response(resp: AnthropicResponse) -> Result<Message, AgentError> {
    let mut content_blocks = Vec::new();

    for block in resp.content {
        match block.block_type.as_str() {
            "text" => {
                if let Some(text) = block.text {
                    if !text.is_empty() {
                        content_blocks.push(ContentBlock::Text { text });
                    }
                }
            }
            "tool_use" => {
                let id = block.id.unwrap_or_default();
                let name = block.name.unwrap_or_default();
                let input = block.input.unwrap_or(Value::Object(serde_json::Map::new()));
                content_blocks.push(ContentBlock::ToolUse { id, name, input });
            }
            "thinking" => {
                // Include thinking blocks as text for now
                if let Some(thinking) = block.thinking {
                    if !thinking.is_empty() {
                        content_blocks.push(ContentBlock::Text {
                            text: format!("<thinking>\n{}\n</thinking>", thinking),
                        });
                    }
                }
            }
            _ => {
                tracing::debug!(block_type = %block.block_type, "unknown Anthropic content block type");
            }
        }
    }

    let usage = resp.usage.map(|u| UsageMetadata {
        input_tokens: u.input_tokens.unwrap_or(0),
        output_tokens: u.output_tokens.unwrap_or(0),
        total_tokens: u.input_tokens.unwrap_or(0) + u.output_tokens.unwrap_or(0),
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
