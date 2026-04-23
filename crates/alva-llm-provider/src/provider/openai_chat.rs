//! OpenAI-compatible Chat Completions provider.
//!
//! Implements `LanguageModel` by calling POST /chat/completions with
//! tool definitions. Works with OpenAI, DeepSeek, Ollama, vLLM, Groq,
//! Fireworks, OpenRouter, xAI, Moonshot, etc.
//!
//! Wire translation lives in
//! `alva_kernel_abi::adapter::openai_chat::OpenAIChatAdapter` — this file
//! is just the HTTP shell + SSE framing.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde_json::Value;

use alva_kernel_abi::adapter::openai_chat::OpenAIChatAdapter;
use alva_kernel_abi::adapter::{StreamDecodeState, ToolAdapter};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::Tool;

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
// Body assembly
// ---------------------------------------------------------------------------

fn build_body(
    model: &str,
    max_tokens: u32,
    messages: &[Value],
    tools: &[Value],
    config: &ModelConfig,
    stream: bool,
) -> Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": messages,
        "max_tokens": max_tokens,
    });
    if stream {
        body["stream"] = Value::Bool(true);
        body["stream_options"] = serde_json::json!({ "include_usage": true });
    }
    if let Some(t) = config.temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = config.top_p {
        body["top_p"] = serde_json::json!(p);
    }
    if !config.stop_sequences.is_empty() {
        body["stop"] = serde_json::json!(config.stop_sequences);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
    }
    body
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
    ) -> Result<CompletionResponse, AgentError> {
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let adapter = OpenAIChatAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let api_tools = adapter.encode_tools(tools);
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let body = build_body(
            &self.model,
            max_tokens,
            &encoded.messages,
            &api_tools,
            config,
            false,
        );

        let span = tracing::info_span!("llm_request",
            provider = "openai_chat",
            model = %self.model,
            url = %url,
            messages = encoded.messages.len(),
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

        let raw_value: Value = serde_json::from_str(&resp_text)
            .map_err(|e| AgentError::LlmError(format!("parse response: {} — raw: {}", e, resp_text)))?;
        let decoded = adapter
            .decode_response(&raw_value)
            .map_err(|e| AgentError::LlmError(format!("decode: {e}")))?;
        Ok(CompletionResponse {
            message: decoded.message,
            raw: Some(raw_value),
        })
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

        let adapter = OpenAIChatAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let api_tools = adapter.encode_tools(tools);
        let body = build_body(&model, max_tokens, &encoded.messages, &api_tools, config, true);

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "openai_chat",
            model = %model,
            url = %url,
            messages = encoded.messages.len(),
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

            let adapter = OpenAIChatAdapter::new();
            let mut state = StreamDecodeState::new();
            let mut byte_stream = resp.bytes_stream();
            let mut buffer = String::new();
            let mut raw_bytes_total: usize = 0;
            let mut first_chunk_logged = false;

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

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        if data == "[DONE]" {
                            yield StreamEvent::Done;
                            return;
                        }

                        let event: Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    data = &data[..data.len().min(200)],
                                    "failed to parse SSE chunk, skipping"
                                );
                                continue;
                            }
                        };
                        match adapter.decode_stream_event(&event, &mut state) {
                            Ok(events) => {
                                for ev in events {
                                    yield ev;
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "decode_stream_event failed");
                            }
                        }
                    }
                }
            }

            if raw_bytes_total == 0 {
                tracing::warn!("SSE stream closed with empty body");
            }
            yield StreamEvent::Done;
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }
}
