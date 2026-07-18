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
use alva_kernel_abi::tool::{Tool, ToolDefinition};

use crate::config::ProviderConfig;
use crate::util::truncate_for_log;

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
            &config.api_key,
            &config.custom_headers,
            crate::auth::AuthScheme::Bearer,
        );
        Self {
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            auth_headers,
            client: crate::util::http_client(),
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

    // `reasoning_effort` — only valid for gpt-5/o-series reasoning models.
    // The server rejects this field on non-reasoning models (400 with
    // "Unsupported parameter"), so we gate on a model-name sniff.
    // `minimal` is gpt-5 original only; gpt-5.1+ uses `none` instead of
    // `minimal` and adds `xhigh` (only on `gpt-5.1-codex-max`).
    if let Some(effort) = config.reasoning_effort {
        if let Some(value) = openai_effort_string(model, effort) {
            body["reasoning_effort"] = Value::String(value.to_string());
        }
    }
    super::apply_extra_body(&mut body, config.extra_body.as_ref());
    body
}

/// Translate `ReasoningEffort` into the exact string the OpenAI Chat
/// Completions API accepts for this model. Returns `None` when:
/// - Model isn't a reasoning model (field will be rejected → skip)
/// - Caller asked for a value that's not supported on this model
///   (e.g. `XHigh` on non-codex-max); we clamp to the closest supported
///   level instead of erroring.
pub(crate) fn openai_effort_string(
    model: &str,
    effort: alva_kernel_abi::ReasoningEffort,
) -> Option<&'static str> {
    use alva_kernel_abi::ReasoningEffort as RE;

    // Sniff model family. Anything else → no reasoning_effort.
    let is_reasoning = model.starts_with("gpt-5")
        || model.starts_with("o1-")
        || model == "o1"
        || model.starts_with("o3")
        || model.starts_with("o4");
    if !is_reasoning {
        return None;
    }
    let is_gpt5_original =
        model == "gpt-5" || model.starts_with("gpt-5-") && !model.starts_with("gpt-5.");
    let is_gpt51_plus = model.starts_with("gpt-5.") || model == "gpt-5.1" || model == "gpt-5.2";
    let is_codex_max = model.contains("codex-max");
    // `o1-mini` has no reasoning_effort at all per docs.
    if model == "o1-mini" {
        return None;
    }

    Some(match effort {
        // gpt-5.1+ introduced explicit "none". On gpt-5 original, omit
        // the field instead (return None from the outer mapping).
        RE::None => {
            if is_gpt51_plus {
                "none"
            } else {
                return None;
            }
        }
        RE::Minimal => {
            // Only gpt-5 original supports "minimal". Others fall back to "low".
            if is_gpt5_original {
                "minimal"
            } else {
                "low"
            }
        }
        RE::Low => "low",
        RE::Medium => "medium",
        RE::High => "high",
        RE::XHigh => {
            if is_codex_max {
                "xhigh"
            } else {
                "high"
            }
        }
    })
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
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
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
            body_preview = truncate_for_log(&body_str, 500),
            "LLM request body"
        );

        let req = self
            .client
            .post(&url)
            .header("Content-Type", "application/json");
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

        let raw_value: Value = serde_json::from_str(&resp_text).map_err(|e| {
            AgentError::LlmError(format!("parse response: {} — raw: {}", e, resp_text))
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
        let url = format!("{}/chat/completions", self.base_url.trim_end_matches('/'));
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let auth_headers = self.auth_headers.clone();

        let adapter = OpenAIChatAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
        let body = build_body(
            &model,
            max_tokens,
            &encoded.messages,
            &api_tools,
            config,
            true,
        );

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
            body_preview = truncate_for_log(&body_str, 500),
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
                        preview = truncate_for_log(&chunk_str, 300),
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
                                    data = truncate_for_log(data, 200),
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

    fn provider_id(&self) -> &str {
        "openai-chat"
    }
}

#[cfg(test)]
mod tests {
    //! Provider-specific regression test. Helper-level coverage lives
    //! in `crate::util::tests`; here we keep the Chinese-locale
    //! regression for the Chat Completions API (most common locale
    //! for this provider in Asian deployments).
    use crate::util::truncate_for_log;

    #[test]
    fn openai_chat_chinese_response_at_500_bytes_no_crash() {
        // Chat Completions API with Chinese user prompt → Chinese
        // assistant reply. 50× "你好，让我来帮你。" > 500 bytes;
        // byte 500 falls inside a CJK char.
        let s = "你好，让我来帮你。".repeat(50);
        assert!(s.len() > 500);
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
    }
}
