// INPUT:  bytes::Bytes, futures::Stream, reqwest, serde_json::Value
// OUTPUT: SseEvent, parse_raw_sse, post_json_with_retry
// POS:    Shared HTTP utilities for LLM provider adapters — raw SSE parsing and retryable POST.

use bytes::Bytes;
use futures::{Stream, StreamExt, stream};
use std::pin::Pin;

use crate::ports::provider::errors::ProviderError;

/// Raw SSE event — before provider-specific JSON parsing.
/// Both OpenAI and Anthropic use SSE but with different event structures.
#[derive(Debug, Clone)]
pub struct SseEvent {
    /// Anthropic uses named events (e.g. `message_start`, `content_block_delta`).
    /// OpenAI omits the event field entirely.
    pub event: Option<String>,
    /// The `data:` payload, which is provider-specific JSON.
    pub data: String,
}

/// Internal state carried through the `unfold` stream.
struct SseParserState<S> {
    byte_stream: S,
    buffer: String,
    done: bool,
    /// Pending events parsed from the buffer but not yet yielded.
    pending: Vec<SseEvent>,
}

/// Parse a single SSE event block (text between `\n\n` separators) into an `SseEvent`.
/// Returns `None` for comment-only blocks or empty blocks.
/// Returns `Some(Err(...))` if `data: [DONE]` is encountered (signals termination).
fn parse_event_block(block: &str) -> Option<Result<SseEvent, ()>> {
    let mut event_type: Option<String> = None;
    let mut data_lines: Vec<&str> = Vec::new();

    for line in block.lines() {
        let line = line.trim_end_matches('\r');

        if line.is_empty() {
            continue;
        }

        // Skip SSE comment lines (lines starting with ':')
        if line.starts_with(':') {
            continue;
        }

        if let Some(value) = line.strip_prefix("event:") {
            event_type = Some(value.trim().to_string());
        } else if let Some(value) = line.strip_prefix("data:") {
            let value = value.trim_start_matches(' ');
            data_lines.push(value);
        }
        // Ignore other field types (id:, retry:, etc.)
    }

    if data_lines.is_empty() {
        // Block had no data lines — might be event-only or comment-only
        return None;
    }

    let data = data_lines.join("\n");

    // Check for [DONE] termination signal (OpenAI style)
    if data == "[DONE]" {
        return Some(Err(()));
    }

    Some(Ok(SseEvent {
        event: event_type,
        data,
    }))
}

/// Parse a raw SSE byte stream into a stream of `SseEvent`s.
///
/// Handles:
/// - OpenAI style: `data:` lines only, terminated by `data: [DONE]`
/// - Anthropic style: `event:` + `data:` lines
/// - Chunk splitting: events may be split across byte chunks
/// - Comment lines: lines starting with `:` are skipped
pub fn parse_raw_sse(
    byte_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + Unpin + 'static,
) -> Pin<Box<dyn Stream<Item = Result<SseEvent, ProviderError>> + Send>> {
    let initial_state = SseParserState {
        byte_stream,
        buffer: String::new(),
        done: false,
        pending: Vec::new(),
    };

    let s = stream::unfold(initial_state, |mut state| async move {
        loop {
            // First, drain any pending events from previous buffer parse
            if let Some(event) = state.pending.pop() {
                return Some((Ok(event), state));
            }

            if state.done {
                return None;
            }

            // Read next chunk from the byte stream
            match state.byte_stream.next().await {
                Some(Ok(bytes)) => {
                    let chunk = match std::str::from_utf8(&bytes) {
                        Ok(s) => s.to_string(),
                        Err(e) => {
                            return Some((
                                Err(ProviderError::Network(format!(
                                    "Invalid UTF-8 in SSE stream: {}",
                                    e
                                ))),
                                state,
                            ));
                        }
                    };
                    state.buffer.push_str(&chunk);

                    // Try to extract complete event blocks (separated by \n\n)
                    let mut events = Vec::new();
                    while let Some(pos) = state.buffer.find("\n\n") {
                        let block = state.buffer[..pos].to_string();
                        state.buffer = state.buffer[pos + 2..].to_string();

                        match parse_event_block(&block) {
                            Some(Ok(event)) => events.push(event),
                            Some(Err(())) => {
                                // [DONE] signal
                                state.done = true;
                                break;
                            }
                            None => {
                                // Comment-only or empty block, skip
                            }
                        }
                    }

                    if !events.is_empty() {
                        // Reverse so we can pop from the end efficiently
                        events.reverse();
                        let first = events.pop().unwrap();
                        state.pending = events;
                        return Some((Ok(first), state));
                    }

                    // No complete events yet — continue reading
                }
                Some(Err(e)) => {
                    state.done = true;
                    return Some((
                        Err(ProviderError::Network(format!("SSE stream error: {}", e))),
                        state,
                    ));
                }
                None => {
                    // Stream ended — process any remaining data in the buffer
                    state.done = true;

                    let remaining = std::mem::take(&mut state.buffer);
                    let trimmed = remaining.trim();
                    if !trimmed.is_empty() {
                        match parse_event_block(trimmed) {
                            Some(Ok(event)) => {
                                return Some((Ok(event), state));
                            }
                            Some(Err(())) => {
                                // [DONE] at end
                                return None;
                            }
                            None => {
                                return None;
                            }
                        }
                    }

                    return None;
                }
            }
        }
    });

    Box::pin(s)
}

/// POST JSON to a URL with retry and exponential backoff.
///
/// Retries on:
/// - 429 (rate limited): extracts `retry-after` header
/// - 5xx (server error)
/// - Network errors
///
/// Does NOT retry on:
/// - 401/403 (authentication errors) — returns immediately
/// - Other 4xx (client errors) — returns immediately
pub async fn post_json_with_retry(
    client: &reqwest::Client,
    url: &str,
    headers: &[(String, String)],
    body: &serde_json::Value,
    max_retries: u32,
) -> Result<reqwest::Response, ProviderError> {
    let mut last_error: Option<ProviderError> = None;

    for attempt in 0..=max_retries {
        if attempt > 0 {
            let backoff_ms = 1000u64 * (1u64 << (attempt - 1).min(4));
            tokio::time::sleep(std::time::Duration::from_millis(backoff_ms)).await;
        }

        let mut request_builder = client.post(url);
        for (key, value) in headers {
            request_builder = request_builder.header(key.as_str(), value.as_str());
        }

        let result = request_builder.json(body).send().await;

        match result {
            Ok(response) => {
                let status = response.status();

                if status.is_success() {
                    return Ok(response);
                }

                let status_code = status.as_u16();

                // 401/403: authentication error — do not retry
                if status_code == 401 || status_code == 403 {
                    let response_body = response.text().await.unwrap_or_default();
                    return Err(ProviderError::ApiCall {
                        message: format!(
                            "Authentication error (HTTP {}): {}",
                            status_code, response_body
                        ),
                        url: url.to_string(),
                        status_code: Some(status_code),
                        response_body: Some(response_body),
                        is_retryable: false,
                    });
                }

                // 429: rate limited — retry with backoff
                if status_code == 429 {
                    let retry_after_ms = response
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u64>().ok())
                        .map(|secs| secs * 1000);

                    last_error = Some(ProviderError::RateLimited { retry_after_ms });
                    continue;
                }

                // 5xx: server error — retry
                if status_code >= 500 {
                    let response_body = response.text().await.unwrap_or_default();
                    last_error = Some(ProviderError::ApiCall {
                        message: format!("Server error (HTTP {})", status_code),
                        url: url.to_string(),
                        status_code: Some(status_code),
                        response_body: Some(response_body),
                        is_retryable: true,
                    });
                    continue;
                }

                // Other 4xx: client error — do not retry
                let response_body = response.text().await.unwrap_or_default();
                return Err(ProviderError::ApiCall {
                    message: format!("Client error (HTTP {}): {}", status_code, response_body),
                    url: url.to_string(),
                    status_code: Some(status_code),
                    response_body: Some(response_body),
                    is_retryable: false,
                });
            }
            Err(e) => {
                // Network error — retry
                last_error = Some(ProviderError::Network(format!(
                    "Request to {} failed: {}",
                    url, e
                )));
                continue;
            }
        }
    }

    // All retries exhausted
    Err(last_error.unwrap_or_else(|| {
        ProviderError::Network(format!("Request to {} failed after {} retries", url, max_retries))
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use futures::stream;

    fn bytes_stream(
        chunks: Vec<&str>,
    ) -> impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin {
        stream::iter(
            chunks
                .into_iter()
                .map(|s| Ok(Bytes::from(s.to_string())))
                .collect::<Vec<_>>(),
        )
    }

    async fn collect_events(
        s: Pin<Box<dyn Stream<Item = Result<SseEvent, ProviderError>> + Send>>,
    ) -> Vec<SseEvent> {
        s.filter_map(|r| async { r.ok() })
            .collect::<Vec<_>>()
            .await
    }

    /// Test 1: OpenAI style — data-only lines, no event field
    #[tokio::test]
    async fn test_openai_style_data_only() {
        let input = bytes_stream(vec![
            "data: {\"choices\":[{\"delta\":{\"content\":\"Hello\"}}]}\n\n",
            "data: {\"choices\":[{\"delta\":{\"content\":\" world\"}}]}\n\n",
            "data: [DONE]\n\n",
        ]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 2);
        assert!(events[0].event.is_none());
        assert!(events[0].data.contains("Hello"));
        assert!(events[1].event.is_none());
        assert!(events[1].data.contains("world"));
    }

    /// Test 2: Anthropic style — event: + data: lines
    #[tokio::test]
    async fn test_anthropic_style_named_events() {
        let input = bytes_stream(vec![
            "event: message_start\ndata: {\"type\":\"message_start\"}\n\n",
            "event: content_block_delta\ndata: {\"type\":\"content_block_delta\",\"delta\":{\"text\":\"Hi\"}}\n\n",
            "event: message_stop\ndata: {\"type\":\"message_stop\"}\n\n",
        ]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 3);
        assert_eq!(events[0].event.as_deref(), Some("message_start"));
        assert_eq!(events[1].event.as_deref(), Some("content_block_delta"));
        assert!(events[1].data.contains("Hi"));
        assert_eq!(events[2].event.as_deref(), Some("message_stop"));
    }

    /// Test 3: Events split across byte chunks
    #[tokio::test]
    async fn test_split_across_chunks() {
        let input = bytes_stream(vec![
            "data: {\"part\"",
            ":1}\n\ndata: {\"part\":2}\n\n",
        ]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].data, "{\"part\":1}");
        assert_eq!(events[1].data, "{\"part\":2}");
    }

    /// Test 4: Skip SSE comment lines (lines starting with ':')
    #[tokio::test]
    async fn test_skip_comments() {
        let input = bytes_stream(vec![
            ": this is a comment\n\n",
            "data: {\"actual\":\"data\"}\n\n",
        ]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 1);
        assert!(events[0].data.contains("actual"));
    }

    /// Test 5: [DONE] terminates the stream
    #[tokio::test]
    async fn test_done_terminates() {
        let input = bytes_stream(vec![
            "data: {\"msg\":\"first\"}\n\n",
            "data: [DONE]\n\n",
            "data: {\"msg\":\"should_not_appear\"}\n\n",
        ]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 1);
        assert!(events[0].data.contains("first"));
    }

    /// Test 6: Empty stream produces no events
    #[tokio::test]
    async fn test_empty_stream() {
        let input = bytes_stream(vec![]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 0);
    }

    /// Test 7: Remaining buffer data is flushed when stream ends
    #[tokio::test]
    async fn test_remaining_buffer_flushed() {
        // Stream ends without trailing \n\n
        let input = bytes_stream(vec!["data: {\"final\":true}"]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 1);
        assert!(events[0].data.contains("final"));
    }

    /// Test 8: Multiple data lines in one event block are joined
    #[tokio::test]
    async fn test_multi_data_lines() {
        let input = bytes_stream(vec!["data: line1\ndata: line2\n\n"]);

        let events = collect_events(parse_raw_sse(input)).await;

        assert_eq!(events.len(), 1);
        assert_eq!(events[0].data, "line1\nline2");
    }
}
