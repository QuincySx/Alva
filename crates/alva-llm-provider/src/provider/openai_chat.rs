//! OpenAI-compatible Chat Completions provider.
//!
//! Implements `LanguageModel` by calling POST /chat/completions with
//! tool definitions. Works with OpenAI, DeepSeek, Ollama, vLLM, etc.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use alva_types::base::error::AgentError;
use alva_types::base::message::{Message, MessageRole, UsageMetadata};
use alva_types::model::{LanguageModel, ModelConfig};
use alva_types::base::stream::StreamEvent;
use alva_types::tool::Tool;
use alva_types::ContentBlock;

use crate::config::ProviderConfig;

/// OpenAI-compatible LLM provider.
pub struct OpenAIChatProvider {
    model: String,
    base_url: String,
    max_tokens: u32,
    /// Pre-resolved auth headers (from api_key or custom_headers at construction time).
    auth_headers: std::collections::HashMap<String, String>,
    client: Client,
}

impl OpenAIChatProvider {
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
impl LanguageModel for OpenAIChatProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let oai_messages = to_oai_messages(messages);
        let oai_tools = to_oai_tools(tools);

        let max_tokens = config
            .max_tokens
            .unwrap_or(self.max_tokens);

        let mut body = serde_json::json!({
            "model": self.model,
            "messages": oai_messages,
            "max_tokens": max_tokens,
        });

        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !config.stop_sequences.is_empty() {
            body["stop"] = serde_json::json!(config.stop_sequences);
        }
        if !oai_tools.is_empty() {
            body["tools"] = serde_json::json!(oai_tools);
        }

        let span = tracing::info_span!("llm_request",
            provider = "openai_chat",
            model = %self.model,
            url = %url,
            messages = oai_messages.len(),
            tools = oai_tools.len(),
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

        let oai_resp: OaiResponse = serde_json::from_str(&resp_text)
            .map_err(|e| AgentError::LlmError(format!("parse response: {} — raw: {}", e, resp_text)))?;

        from_oai_response(oai_resp)
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let auth_headers = self.auth_headers.clone();

        let oai_messages = to_oai_messages(messages);
        let oai_tools = to_oai_tools(tools);

        let mut body = serde_json::json!({
            "model": model,
            "messages": oai_messages,
            "max_tokens": max_tokens,
            "stream": true,
            "stream_options": { "include_usage": true },
        });

        if let Some(t) = config.temperature {
            body["temperature"] = serde_json::json!(t);
        }
        if let Some(p) = config.top_p {
            body["top_p"] = serde_json::json!(p);
        }
        if !config.stop_sequences.is_empty() {
            body["stop"] = serde_json::json!(config.stop_sequences);
        }
        if !oai_tools.is_empty() {
            body["tools"] = serde_json::json!(oai_tools);
        }

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "openai_chat",
            model = %model,
            url = %url,
            messages = oai_messages.len(),
            tools = oai_tools.len(),
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

            tracing::info!(provider = "openai_chat", "sending HTTP request, waiting for response...");
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
            tracing::info!(
                provider = "openai_chat",
                status = %resp.status(),
                duration_ms = req_start.elapsed().as_millis() as u64,
                "HTTP response received, reading stream"
            );

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                yield StreamEvent::Error(format!("API returned {}: {}", status, body));
                return;
            }

            // Read SSE lines from the byte stream
            let mut byte_stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut raw_bytes_total: usize = 0;
            let mut first_chunk_logged = false;
            // Diagnostics: track what we received for end-of-stream summary
            let mut sse_data_count: u32 = 0;
            let mut sse_parsed_count: u32 = 0;
            let mut sse_empty_choices: u32 = 0;
            let mut sse_content_deltas: u32 = 0;
            let mut sse_tool_deltas: u32 = 0;
            let mut first_raw_data: Option<String> = None; // capture first SSE data line for diagnostics

            while let Some(chunk) = futures::StreamExt::next(&mut byte_stream).await {
                let chunk = match chunk {
                    Ok(c) => c,
                    Err(e) => {
                        yield StreamEvent::Error(format!("stream read error: {}", e));
                        return;
                    }
                };

                let chunk_str = String::from_utf8_lossy(&chunk);
                raw_bytes_total += chunk.len();
                if !first_chunk_logged {
                    first_chunk_logged = true;
                    tracing::debug!(
                        bytes = chunk.len(),
                        preview = &chunk_str[..chunk_str.len().min(300)],
                        "first SSE chunk received"
                    );
                }
                buffer.push_str(&chunk_str);

                // Process complete lines
                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            // End-of-stream diagnostics
                            if sse_content_deltas == 0 && sse_tool_deltas == 0 {
                                tracing::warn!(
                                    sse_data_lines = sse_data_count,
                                    sse_parsed = sse_parsed_count,
                                    sse_empty_choices = sse_empty_choices,
                                    first_data = first_raw_data.as_deref().unwrap_or("(none)"),
                                    "SSE stream produced no content — 0 text deltas, 0 tool deltas"
                                );
                            }
                            yield StreamEvent::Done;
                            return;
                        }

                        sse_data_count += 1;
                        if first_raw_data.is_none() {
                            first_raw_data = Some(data[..data.len().min(300)].to_string());
                        }

                        match serde_json::from_str::<OaiStreamChunk>(data) {
                            Ok(chunk) => {
                                sse_parsed_count += 1;
                                if chunk.choices.is_empty() {
                                    sse_empty_choices += 1;
                                }
                                for choice in &chunk.choices {
                                    if let Some(ref content) = choice.delta.content {
                                        if !content.is_empty() {
                                            sse_content_deltas += 1;
                                            yield StreamEvent::TextDelta { text: content.clone() };
                                        }
                                    }
                                    if let Some(ref tool_calls) = choice.delta.tool_calls {
                                        for tc in tool_calls {
                                            sse_tool_deltas += 1;
                                            yield StreamEvent::ToolCallDelta {
                                                id: tc.id.clone().unwrap_or_default(),
                                                name: tc.function.as_ref().and_then(|f| f.name.clone()),
                                                arguments_delta: tc.function.as_ref()
                                                    .map(|f| f.arguments.clone().unwrap_or_default())
                                                    .unwrap_or_default(),
                                            };
                                        }
                                    }
                                }
                                if let Some(ref usage) = chunk.usage {
                                    yield StreamEvent::Usage(UsageMetadata {
                                        input_tokens: usage.prompt_tokens,
                                        output_tokens: usage.completion_tokens,
                                        total_tokens: usage.total_tokens,
                                    });
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    data = &data[..data.len().min(200)],
                                    "failed to parse SSE chunk, skipping"
                                );
                            }
                        }
                    }
                }
            }

            // Stream ended without [DONE] — diagnostics only (fallback is in agent-core)
            if raw_bytes_total == 0 {
                tracing::warn!("SSE stream closed with empty body — proxy returned HTTP 200 but no data");
            } else if sse_data_count == 0 {
                tracing::warn!(
                    raw_bytes = raw_bytes_total,
                    remaining_buffer = &buffer[..buffer.len().min(500)],
                    "received {} bytes but found no 'data:' lines", raw_bytes_total,
                );
            } else if sse_content_deltas == 0 && sse_tool_deltas == 0 {
                tracing::warn!(
                    sse_data_lines = sse_data_count,
                    sse_parsed = sse_parsed_count,
                    sse_empty_choices = sse_empty_choices,
                    first_data = first_raw_data.as_deref().unwrap_or("(none)"),
                    "SSE parsed OK but produced no text or tool content"
                );
            }
            yield StreamEvent::Done;
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}

// ---------------------------------------------------------------------------
// OpenAI API types (request)
// ---------------------------------------------------------------------------

#[derive(Serialize)]
struct OaiMessage {
    role: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    content: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_calls: Option<Vec<OaiToolCall>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_call_id: Option<String>,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiToolCall {
    id: String,
    #[serde(rename = "type")]
    call_type: String,
    function: OaiFunction,
}

#[derive(Serialize, Deserialize, Clone)]
struct OaiFunction {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
struct OaiToolDef {
    #[serde(rename = "type")]
    tool_type: String,
    function: OaiFunctionDef,
}

#[derive(Serialize)]
struct OaiFunctionDef {
    name: String,
    description: String,
    parameters: Value,
}

// ---------------------------------------------------------------------------
// OpenAI API types (response)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OaiResponse {
    choices: Vec<OaiChoice>,
    #[serde(default)]
    usage: Option<OaiUsage>,
}

#[derive(Deserialize)]
struct OaiChoice {
    message: OaiResponseMessage,
}

#[derive(Deserialize)]
struct OaiResponseMessage {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OaiToolCall>>,
}

#[derive(Deserialize)]
struct OaiUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    total_tokens: u32,
}

// ---------------------------------------------------------------------------
// OpenAI API types (streaming response)
// ---------------------------------------------------------------------------

#[derive(Deserialize)]
struct OaiStreamChunk {
    #[serde(default)]
    choices: Vec<OaiStreamChoice>,
    #[serde(default)]
    usage: Option<OaiUsage>,
}

#[derive(Deserialize)]
struct OaiStreamChoice {
    delta: OaiStreamDelta,
}

#[derive(Deserialize)]
struct OaiStreamDelta {
    #[serde(default)]
    content: Option<String>,
    #[serde(default)]
    tool_calls: Option<Vec<OaiStreamToolCall>>,
}

#[derive(Deserialize)]
struct OaiStreamToolCall {
    #[serde(default)]
    #[allow(dead_code)]
    index: usize,
    #[serde(default)]
    id: Option<String>,
    #[serde(default)]
    function: Option<OaiStreamFunction>,
}

#[derive(Deserialize)]
struct OaiStreamFunction {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    arguments: Option<String>,
}

// ---------------------------------------------------------------------------
// Conversion helpers
// ---------------------------------------------------------------------------

fn to_oai_messages(messages: &[Message]) -> Vec<OaiMessage> {
    messages.iter().map(|m| {
        match m.role {
            MessageRole::Tool => {
                // Tool result → send as role=tool with tool_call_id
                // Extract text from ToolResult content blocks (Vec<ToolContent>),
                // falling back to text_content() for any plain Text blocks.
                let content = {
                    let mut parts: Vec<String> = Vec::new();
                    for block in &m.content {
                        if let ContentBlock::ToolResult { content, .. } = block {
                            for tc in content {
                                parts.push(tc.to_model_string());
                            }
                        } else if let Some(text) = block.as_text() {
                            parts.push(text.to_string());
                        }
                    }
                    parts.join("\n")
                };
                OaiMessage {
                    role: "tool".to_string(),
                    content: Some(Value::String(content)),
                    tool_calls: None,
                    tool_call_id: m.tool_call_id.clone(),
                }
            }
            MessageRole::Assistant if m.has_tool_calls() => {
                // Assistant with tool calls
                let text = m.text_content();
                let tool_calls: Vec<OaiToolCall> = m.content.iter().filter_map(|b| {
                    if let ContentBlock::ToolUse { id, name, input } = b {
                        Some(OaiToolCall {
                            id: id.clone(),
                            call_type: "function".to_string(),
                            function: OaiFunction {
                                name: name.clone(),
                                arguments: input.to_string(),
                            },
                        })
                    } else {
                        None
                    }
                }).collect();

                OaiMessage {
                    role: "assistant".to_string(),
                    content: if text.is_empty() { None } else { Some(Value::String(text)) },
                    tool_calls: Some(tool_calls),
                    tool_call_id: None,
                }
            }
            _ => {
                let role = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::System => "system",
                    MessageRole::Tool => "tool",
                };
                OaiMessage {
                    role: role.to_string(),
                    content: Some(Value::String(m.text_content())),
                    tool_calls: None,
                    tool_call_id: None,
                }
            }
        }
    }).collect()
}

fn to_oai_tools(tools: &[&dyn Tool]) -> Vec<OaiToolDef> {
    tools.iter().map(|t| OaiToolDef {
        tool_type: "function".to_string(),
        function: OaiFunctionDef {
            name: t.name().to_string(),
            description: t.description().to_string(),
            parameters: t.parameters_schema(),
        },
    }).collect()
}

fn from_oai_response(resp: OaiResponse) -> Result<Message, AgentError> {
    let choice = resp.choices.into_iter().next()
        .ok_or_else(|| AgentError::LlmError("no choices in response".to_string()))?;

    let mut content_blocks = Vec::new();

    // Text content
    if let Some(text) = choice.message.content {
        if !text.is_empty() {
            content_blocks.push(ContentBlock::Text { text });
        }
    }

    // Tool calls
    if let Some(tool_calls) = choice.message.tool_calls {
        for tc in tool_calls {
            let input: Value = serde_json::from_str(&tc.function.arguments)
                .unwrap_or(Value::Object(serde_json::Map::new()));
            content_blocks.push(ContentBlock::ToolUse {
                id: tc.id,
                name: tc.function.name,
                input,
            });
        }
    }

    let usage = resp.usage.map(|u| UsageMetadata {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
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
