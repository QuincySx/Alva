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

use crate::util::truncate_for_log;
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
    // System prompt: emit as a TextBlock array so we can place
    // `cache_control: ephemeral` markers on every segment except the
    // last (the convention is "all-but-last is stable / cacheable").
    // Single-segment systems still emit an array-of-1 with no cache
    // marker — Anthropic accepts both shapes (string OR block array).
    // 4-breakpoint marker placement (mirrors pi-mono / Claude Code):
    //   * tools[N-1]      → last tool gets cache_control
    //   * system[K-1]     → last system block stays uncached;
    //                       earlier system blocks each get marker
    //   * messages[user]  → last user message's last text block
    //                       gets cache_control (added below)
    if let Some(segments) = &encoded.system_segments {
        if !segments.is_empty() {
            let last_idx = segments.len() - 1;
            let blocks: Vec<Value> = segments
                .iter()
                .enumerate()
                .map(|(i, text)| {
                    if i < last_idx {
                        serde_json::json!({
                            "type": "text",
                            "text": text,
                            "cache_control": { "type": "ephemeral" },
                        })
                    } else {
                        serde_json::json!({
                            "type": "text",
                            "text": text,
                        })
                    }
                })
                .collect();
            body["system"] = Value::Array(blocks);
        }
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
        // Mark the LAST tool with `cache_control: ephemeral` so the
        // tool schema layer caches independently from the system
        // prompt and message history. Tools usually change less than
        // the system prompt, so this is the cheapest cache hit.
        let mut tools_with_cache = tools.to_vec();
        if let Some(last) = tools_with_cache.last_mut() {
            if let Some(obj) = last.as_object_mut() {
                obj.insert(
                    "cache_control".to_string(),
                    serde_json::json!({ "type": "ephemeral" }),
                );
            }
        }
        body["tools"] = Value::Array(tools_with_cache);
    }

    // Mark the last user message's last text block with
    // `cache_control: ephemeral` so the entire conversation history up
    // to (and including) that message becomes cacheable. The current
    // turn's appended user content sits *after* this marker and is
    // the natural delta. Mirrors pi-mono's `addCacheControlToLastConversationMessage`.
    apply_cache_marker_to_last_user(&mut body);

    // Extended thinking. Per Anthropic docs:
    //   - Opus 4.7+ must use {type:"adaptive"} — manual enable returns 400
    //   - 4.6 / Sonnet 4.6 manual mode deprecated but still works
    //   - budget_tokens must be < max_tokens
    //   - Mid-turn toggling breaks prompt caching + may strip thinking
    //     blocks; that's a call-site concern, not ours here.
    if let Some(effort) = config.reasoning_effort {
        apply_anthropic_thinking(&mut body, model, max_tokens, effort);
    }
    super::apply_extra_body(&mut body, config.extra_body.as_ref());
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
            body_preview = truncate_for_log(&body_str, 500),
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
            body_preview = truncate_for_log(&resp_text, 500),
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
                                    data = truncate_for_log(data, 200),
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

    fn provider_id(&self) -> &str {
        "anthropic"
    }
}

/// Apply extended-thinking config to the request body based on model + effort.
///
/// - `ReasoningEffort::None` → `thinking: {type:"disabled"}` (except Opus 4.7+
///   which rejects disabled; for those we just omit the field, relying on the
///   model's default adaptive behavior)
/// - Opus 4.7+ with any non-`None` effort → `{type:"adaptive"}`. 4.7 doesn't
///   honor explicit budgets.
/// - Other extended-thinking models → `{type:"enabled", budget_tokens: N}`
///   where N is clamped to `max_tokens - 1` to satisfy the budget_tokens <
///   max_tokens rule.
/// Place `cache_control: ephemeral` on the **last user message's last
/// text block** in `body["messages"]`. This is one of Anthropic's 4
/// cache breakpoints — caching the entire conversation prefix up to
/// that user turn so subsequent turns can reuse it.
///
/// Behavior:
///   - If `messages` isn't an array, no-op.
///   - If no `role: "user"` message exists, no-op.
///   - If the user message's `content` is a string, upgrade to a
///     `[{type:"text", text:..., cache_control:...}]` block array.
///   - If `content` is already a block array, mark the last text block.
fn apply_cache_marker_to_last_user(body: &mut Value) {
    let Some(messages) = body.get_mut("messages").and_then(|v| v.as_array_mut()) else {
        return;
    };
    // Find the last user message by index.
    let last_user_idx = messages
        .iter()
        .rposition(|m| m.get("role").and_then(|r| r.as_str()) == Some("user"));
    let Some(idx) = last_user_idx else { return };
    let Some(msg) = messages.get_mut(idx) else { return };
    let Some(obj) = msg.as_object_mut() else { return };
    let Some(content) = obj.get_mut("content") else { return };

    match content {
        Value::String(text) => {
            let upgraded = serde_json::json!([{
                "type": "text",
                "text": text,
                "cache_control": { "type": "ephemeral" },
            }]);
            *content = upgraded;
        }
        Value::Array(blocks) => {
            // Walk from the end to find the last text block (skip
            // tool_use / image / etc. — those don't accept cache_control
            // in the same way).
            for block in blocks.iter_mut().rev() {
                let Some(b_obj) = block.as_object_mut() else { continue };
                if b_obj.get("type").and_then(|t| t.as_str()) == Some("text") {
                    b_obj.insert(
                        "cache_control".to_string(),
                        serde_json::json!({ "type": "ephemeral" }),
                    );
                    break;
                }
            }
        }
        _ => {}
    }
}

fn apply_anthropic_thinking(
    body: &mut Value,
    model: &str,
    max_tokens: u32,
    effort: alva_kernel_abi::ReasoningEffort,
) {
    use alva_kernel_abi::ReasoningEffort as RE;

    let is_opus_47_plus = model.contains("opus-4-7") || model.contains("opus-4.7");

    match effort {
        RE::None => {
            if !is_opus_47_plus {
                body["thinking"] = serde_json::json!({"type": "disabled"});
            }
            // Opus 4.7+ doesn't support disabled — leave field out, model
            // uses adaptive-by-default.
        }
        _ => {
            if is_opus_47_plus {
                body["thinking"] = serde_json::json!({"type": "adaptive"});
            } else {
                let budget = effort
                    .suggested_token_budget()
                    .unwrap_or(8192)
                    .min(max_tokens.saturating_sub(1).max(1024));
                body["thinking"] = serde_json::json!({
                    "type": "enabled",
                    "budget_tokens": budget,
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    //! Provider-specific regression tests. The helper-level coverage
    //! lives in `crate::util::tests` after L72's DRY consolidation;
    //! here we keep the Anthropic-locale realistic regression
    //! (Chinese error response).
    use crate::util::truncate_for_log;

    #[test]
    fn anthropic_chinese_error_response_at_500_bytes_no_crash() {
        // Realistic: Anthropic returns a localized error message in
        // Chinese. 200 CJK chars × 3 bytes = 600 bytes; byte 500 falls
        // inside the 167th char.
        let s = "中".repeat(200);
        assert_eq!(s.len(), 600);
        assert!(!s.is_char_boundary(500));
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
    }
}
