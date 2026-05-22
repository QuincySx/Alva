//! OpenAI Responses API provider.
//!
//! Implements `LanguageModel` by calling POST /v1/responses with the newer
//! Responses API format. Wire translation lives in
//! `alva_kernel_abi::adapter::openai_responses::OpenAIResponsesAdapter` —
//! this file is the HTTP shell + named-SSE framing.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde_json::Value;

use alva_kernel_abi::adapter::openai_responses::OpenAIResponsesAdapter;
use alva_kernel_abi::adapter::{StreamDecodeState, ToolAdapter};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::{Tool, ToolDefinition};

use crate::config::ProviderConfig;
use crate::util::truncate_for_log;

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
        "input": encoded.messages,
        "max_output_tokens": max_tokens,
    });
    if stream {
        body["stream"] = Value::Bool(true);
    }
    // OpenAI Responses uses a single `instructions` string. Auto-prefix
    // caching kicks in for ≥1024 token prefixes — the kernel's stable→
    // dynamic ordering already gives us a long stable prefix, so we
    // just join the segments.
    if let Some(instructions) = encoded.system_flat() {
        body["instructions"] = Value::String(instructions);
    }
    if let Some(t) = config.temperature {
        body["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = config.top_p {
        body["top_p"] = serde_json::json!(p);
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
    }

    // Responses API uses a nested `reasoning: { effort }` (vs Chat's
    // top-level `reasoning_effort`). Same model applicability + value
    // mapping rules — reuse the Chat helper.
    if let Some(effort) = config.reasoning_effort {
        if let Some(value) = super::openai_chat::openai_effort_string(model, effort) {
            body["reasoning"] = serde_json::json!({ "effort": value });
        }
    }
    super::apply_extra_body(&mut body, config.extra_body.as_ref());
    body
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
    ) -> Result<CompletionResponse, AgentError> {
        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));
        let adapter = OpenAIResponsesAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let body = build_body(&self.model, max_tokens, &encoded, &api_tools, config, false);

        let span = tracing::info_span!("llm_request",
            provider = "openai_responses",
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
            body_preview = truncate_for_log(&body_str, 500),
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
            body_preview = truncate_for_log(&resp_text, 500),
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
        let url = format!("{}/v1/responses", self.base_url.trim_end_matches('/'));
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let auth_headers = self.auth_headers.clone();

        let adapter = OpenAIResponsesAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
        let body = build_body(&model, max_tokens, &encoded, &api_tools, config, true);

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "openai_responses",
            model = %model,
            url = %url,
            messages = encoded.messages.len(),
            tools = api_tools.len(),
            stream = true,
            body_len = body_str.len(),
            "LLM stream request"
        );
        tracing::debug!(
            body_preview = truncate_for_log(&body_str, 500),
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

            let adapter = OpenAIResponsesAdapter::new();
            let mut state = StreamDecodeState::new();
            let mut byte_stream = resp.bytes_stream();
            let mut buffer = String::new();

            // Responses API SSE: `event: <name>` line precedes `data: <json>` line.
            // We track the current event name in state.event_type and let the
            // adapter dispatch on it.
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

                    if let Some(event_name) = line.strip_prefix("event: ") {
                        state.event_type = Some(event_name.trim().to_string());
                        continue;
                    }

                    if let Some(data) = line.strip_prefix("data: ") {
                        let event: Value = match serde_json::from_str(data) {
                            Ok(v) => v,
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    event_type = ?state.event_type,
                                    data = truncate_for_log(data, 200),
                                    "failed to parse SSE chunk"
                                );
                                state.event_type = None;
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
                        state.event_type = None;
                    }
                }
            }

            yield StreamEvent::Done;
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "openai-responses"
    }
}

#[cfg(test)]
mod tests {
    //! Provider-specific regression test. Helper-level coverage lives
    //! in `crate::util::tests`; here we keep the OpenAI Responses
    //! locale-realistic scenario (reasoning trace with emoji).
    use crate::util::truncate_for_log;

    #[test]
    fn openai_responses_reasoning_trace_with_emoji_at_500_bytes_no_crash() {
        // Realistic: OpenAI Responses returns reasoning content with
        // emoji at varied byte positions; tracing macro must not panic.
        let s = format!(
            "{}🤔{}",
            "Reasoning step: ".repeat(30), // ~480 ASCII bytes
            " continued".repeat(5),
        );
        // Must not panic regardless of where 🤔 lands relative to 500.
        let out = truncate_for_log(&s, 500);
        assert!(out.is_char_boundary(out.len()));
    }
}
