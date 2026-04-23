//! Direct Anthropic Messages API provider.
//!
//! Implements `LanguageModel` by calling Anthropic's `/v1/messages` endpoint
//! directly (not via OpenAI-compatible proxy). Supports:
//! - Native Anthropic message format (via `AnthropicAdapter`)
//! - System prompt as separate parameter
//! - Tool use blocks (Claude format)
//! - Thinking blocks
//! - Streaming via SSE
//! - Token counting from response usage
//! - Rate limit header tracking
//!
//! Wire translation (tool/message encode + response/stream decode) lives in
//! `alva_kernel_abi::adapter::anthropic::AnthropicAdapter`. This file is the
//! HTTP shell — it handles the request body assembly, SSE framing, and rate
//! limit tracking, then delegates all JSON shaping to the adapter.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde_json::Value;

use alva_kernel_abi::adapter::anthropic::AnthropicAdapter;
use alva_kernel_abi::adapter::{StreamDecodeState, ToolAdapter};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::Tool;

use crate::config::ProviderConfig;
use crate::rate_limit::RateLimitState;

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
// Body assembly
// ---------------------------------------------------------------------------

fn build_body(
    model: &str,
    max_tokens: u32,
    encoded: &alva_kernel_abi::adapter::EncodedMessages,
    tools: &[Value],
    config: &ModelConfig,
    stream: bool,
) -> Value {
    let mut body = serde_json::json!({
        "model": model,
        "messages": encoded.messages,
        "max_tokens": max_tokens,
    });
    if stream {
        body["stream"] = Value::Bool(true);
    }
    if let Some(system) = &encoded.system {
        body["system"] = Value::String(system.clone());
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
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
    }
    body
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
    ) -> Result<CompletionResponse, AgentError> {
        let url = format!(
            "{}/v1/messages",
            self.base_url.trim_end_matches('/')
        );

        // Record request for rate limiting
        let _rate_check = self.rate_limit.record_request();

        let adapter = AnthropicAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let api_tools = adapter.encode_tools(tools);
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let body = build_body(&self.model, max_tokens, &encoded, &api_tools, config, false);

        let span = tracing::info_span!("llm_request",
            provider = "anthropic",
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

        let raw_value: Value = serde_json::from_str(&resp_text).map_err(|e| {
            AgentError::LlmError(format!("parse response: {} -- raw: {}", e, resp_text))
        })?;

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
        let url = format!(
            "{}/v1/messages",
            self.base_url.trim_end_matches('/')
        );
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let rate_limit = self.rate_limit.clone();
        let auth_headers = self.auth_headers.clone();

        let adapter = AnthropicAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let api_tools = adapter.encode_tools(tools);
        let body = build_body(&model, max_tokens, &encoded, &api_tools, config, true);

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "anthropic",
            model = %model,
            url = %url,
            messages = encoded.messages.len(),
            tools = api_tools.len(),
            stream = true,
            body_len = body_str.len(),
            "LLM stream request"
        );

        Box::pin(async_stream::stream! {
            yield StreamEvent::Start;

            let _rate_check = rate_limit.record_request();

            tracing::info!(provider = "anthropic", "sending HTTP request, waiting for response...");
            let req_start = std::time::Instant::now();
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
                    tracing::error!(duration_ms = req_start.elapsed().as_millis() as u64, error = %e, "HTTP request failed");
                    yield StreamEvent::Error(format!("HTTP request failed: {}", e));
                    return;
                }
            };
            tracing::info!(provider = "anthropic", status = %resp.status(), duration_ms = req_start.elapsed().as_millis() as u64, "HTTP response received");

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                yield StreamEvent::Error(format!("Anthropic API returned {}: {}", status, body));
                return;
            }

            let adapter = AnthropicAdapter::new();
            let mut state = StreamDecodeState::new();
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

                while let Some(newline_pos) = buffer.find('\n') {
                    let line = buffer[..newline_pos].trim().to_string();
                    buffer = buffer[newline_pos + 1..].to_string();

                    if line.is_empty() || line.starts_with(':') {
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        let event: Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    data = &data[..data.len().min(200)],
                                    "failed to parse Anthropic SSE chunk, skipping"
                                );
                                continue;
                            }
                        };
                        match adapter.decode_stream_event(&event, &mut state) {
                            Ok(events) => {
                                for ev in events {
                                    let is_done = matches!(ev, StreamEvent::Done);
                                    yield ev;
                                    if is_done {
                                        return;
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(error = %e, "decode_stream_event failed");
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
