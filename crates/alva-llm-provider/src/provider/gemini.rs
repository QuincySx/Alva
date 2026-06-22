//! Google Gemini / Vertex AI provider.
//!
//! Implements `LanguageModel` by calling the Gemini API's `generateContent`
//! and `streamGenerateContent` endpoints. Works with both:
//! - **Gemini API** (`https://generativelanguage.googleapis.com`) — consumer
//!   API, API-key auth
//! - **Vertex AI** (region-scoped) — enterprise, via custom_headers (OAuth)
//!
//! Wire translation lives in
//! `alva_kernel_abi::adapter::gemini::GeminiAdapter` — this file is the HTTP
//! shell + SSE framing.

use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;
use reqwest::Client;
use serde_json::Value;

use alva_kernel_abi::adapter::gemini::GeminiAdapter;
use alva_kernel_abi::adapter::{StreamDecodeState, ToolAdapter};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel, ModelConfig};
use alva_kernel_abi::tool::{Tool, ToolDefinition};

use crate::config::ProviderConfig;
use crate::util::truncate_for_log;

/// Google Gemini / Vertex AI provider.
pub struct GeminiProvider {
    model: String,
    base_url: String,
    max_tokens: u32,
    auth_headers: std::collections::HashMap<String, String>,
    client: Client,
}

impl GeminiProvider {
    /// Create from config. The `base_url` should be either the Gemini API
    /// root (`https://generativelanguage.googleapis.com`) or a Vertex AI
    /// models root with project + region encoded.
    pub fn new(config: ProviderConfig) -> Self {
        let auth_headers = crate::auth::resolve_auth_headers(
            &config.api_key,
            &config.custom_headers,
            crate::auth::AuthScheme::GoogApiKey,
        );
        Self {
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            auth_headers,
            client: Client::new(),
        }
    }

    fn endpoint(&self, op: &str) -> String {
        format!(
            "{}/v1beta/models/{}:{}",
            self.base_url.trim_end_matches('/'),
            self.model,
            op,
        )
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
) -> Value {
    let mut generation_config = serde_json::json!({ "maxOutputTokens": max_tokens });
    if let Some(t) = config.temperature {
        generation_config["temperature"] = serde_json::json!(t);
    }
    if let Some(p) = config.top_p {
        generation_config["topP"] = serde_json::json!(p);
    }
    if !config.stop_sequences.is_empty() {
        generation_config["stopSequences"] = serde_json::json!(config.stop_sequences);
    }

    // Thinking. Gemini 2.5 series accepts `thinkingConfig.thinkingBudget`;
    // 3.x accepts `thinkingLevel` (with `thinkingBudget` kept for back-compat).
    // Docs:
    //   0     → disabled (Flash/Flash-Lite only; Pro min is 128)
    //   -1    → dynamic (model auto-adjusts, default)
    //   N > 0 → explicit budget cap
    //   range 0..=24576 for Flash/Flash-Lite
    // Non-thinking models ignore the field.
    if let Some(effort) = config.reasoning_effort {
        if let Some(thinking) = gemini_thinking_config(model, effort) {
            generation_config["thinkingConfig"] = thinking;
        }
    }

    let mut body = serde_json::json!({
        "contents": encoded.messages,
        "generationConfig": generation_config,
    });
    // Gemini's systemInstruction is a single block; auto-prefix-caches.
    if let Some(system) = encoded.system_flat() {
        body["systemInstruction"] = serde_json::json!({
            "parts": [{ "text": system }]
        });
    }
    if !tools.is_empty() {
        body["tools"] = Value::Array(tools.to_vec());
    }
    super::apply_extra_body(&mut body, config.extra_body.as_ref());
    body
}

fn gemini_thinking_config(model: &str, effort: alva_kernel_abi::ReasoningEffort) -> Option<Value> {
    use alva_kernel_abi::ReasoningEffort as RE;

    // Sniff model family. Only 2.5+ and 3.x support thinking per docs.
    let is_thinking_model = model.contains("2.5") || model.contains("-3-") || model.contains("-3.");
    if !is_thinking_model {
        return None;
    }
    let is_pro = model.contains("pro"); // 2.5-pro cannot disable thinking (min 128)
    let is_gemini_3 = model.contains("-3-") || model.contains("-3.");

    // Gemini 3.x uses thinkingLevel strings ("minimal" / "low" / "high" etc.)
    // — we map enum to the closest available.
    if is_gemini_3 {
        let level = match effort {
            RE::None => "low", // 3.x has no explicit "off"; use lowest
            RE::Minimal => "minimal",
            RE::Low => "low",
            RE::Medium | RE::High | RE::XHigh => "high",
        };
        return Some(serde_json::json!({ "thinkingLevel": level }));
    }

    // 2.5 series: use thinkingBudget token count.
    let budget: i64 = match effort {
        RE::None => {
            if is_pro {
                128 // Pro minimum — effectively the "least thinking" we can ask for
            } else {
                0 // Flash / Flash-Lite: 0 = off
            }
        }
        RE::Minimal => 1024,
        RE::Low => 2048,
        RE::Medium => 8192,
        RE::High => 16384,
        RE::XHigh => 24576,
    };
    Some(serde_json::json!({ "thinkingBudget": budget }))
}

// ---------------------------------------------------------------------------
// LanguageModel implementation
// ---------------------------------------------------------------------------

#[async_trait]
impl LanguageModel for GeminiProvider {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        let url = self.endpoint("generateContent");
        let adapter = GeminiAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let body = build_body(&self.model, max_tokens, &encoded, &api_tools, config);

        let span = tracing::info_span!("llm_request",
            provider = "gemini",
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
                "Gemini API returned {}: {}",
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
        let url = format!("{}?alt=sse", self.endpoint("streamGenerateContent"));
        let client = self.client.clone();
        let model = self.model.clone();
        let max_tokens = config.max_tokens.unwrap_or(self.max_tokens);
        let auth_headers = self.auth_headers.clone();

        let adapter = GeminiAdapter::new();
        let encoded = adapter.encode_messages(messages);
        let tool_defs: Vec<ToolDefinition> = tools.iter().map(|t| t.definition()).collect();
        let api_tools = adapter.encode_tools(&tool_defs);
        let body = build_body(&self.model, max_tokens, &encoded, &api_tools, config);

        let body_str = serde_json::to_string(&body).unwrap_or_default();
        tracing::info!(
            provider = "gemini",
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

            tracing::info!(provider = "gemini", "sending HTTP request, waiting for response...");
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
            tracing::info!(provider = "gemini", status = %resp.status(), duration_ms = req_start.elapsed().as_millis() as u64, "HTTP response received");

            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                yield StreamEvent::Error(format!("Gemini API returned {}: {}", status, body));
                return;
            }

            let adapter = GeminiAdapter::new();
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
                                    data = truncate_for_log(data, 200),
                                    "failed to parse Gemini SSE chunk, skipping"
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

            yield StreamEvent::Done;
        })
    }

    fn model_id(&self) -> &str {
        &self.model
    }

    fn provider_id(&self) -> &str {
        "gemini"
    }
}

#[cfg(test)]
mod tests {
    //! Provider-specific regression test. Helper-level coverage lives
    //! in `crate::util::tests`; here we keep the Japanese-locale
    //! regression for Gemini (common locale for this provider).
    use crate::util::truncate_for_log;

    #[test]
    fn gemini_japanese_response_at_500_bytes_no_crash() {
        // ~20× "こんにちは、今日は何をお手伝いしましょうか？" > 500 bytes;
        // each kanji/kana is 3 bytes so byte 500 falls inside a char.
        let s = "こんにちは、今日は何をお手伝いしましょうか？".repeat(20);
        assert!(s.len() > 500);
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
    }
}
