// INPUT:  std::{str,time::Duration}, reqwest
// OUTPUT: truncate_for_log, http_client, SseLineBuffer
// POS:    Small shared helpers every provider uses: a UTF-8-safe slice for
//         tracing previews, bounded incremental SSE framing, and the timeout-bearing HTTP client builder.

use std::str;
use std::time::Duration;

const MAX_SSE_LINE_BYTES: usize = 1024 * 1024;

/// Incremental SSE line framer that preserves UTF-8 across arbitrary network
/// chunk boundaries and rejects an unterminated/oversized provider frame.
pub(crate) struct SseLineBuffer {
    pending: Vec<u8>,
}

impl SseLineBuffer {
    pub(crate) fn new() -> Self {
        Self {
            pending: Vec::new(),
        }
    }

    pub(crate) fn push(&mut self, chunk: &[u8]) -> Result<Vec<String>, String> {
        self.pending.extend_from_slice(chunk);
        let mut lines = Vec::new();
        let mut start = 0;
        for (index, byte) in self.pending.iter().enumerate() {
            if *byte != b'\n' {
                continue;
            }
            let mut line = &self.pending[start..index];
            if line.last() == Some(&b'\r') {
                line = &line[..line.len() - 1];
            }
            if line.len() > MAX_SSE_LINE_BYTES {
                return Err(format!(
                    "SSE line is {} bytes; limit is {MAX_SSE_LINE_BYTES} bytes",
                    line.len()
                ));
            }
            let line = str::from_utf8(line)
                .map_err(|error| format!("SSE line is not valid UTF-8: {error}"))?;
            lines.push(line.to_string());
            start = index + 1;
        }
        if start > 0 {
            self.pending.drain(..start);
        }
        if self.pending.len() > MAX_SSE_LINE_BYTES {
            return Err(format!(
                "unterminated SSE line exceeds the {MAX_SSE_LINE_BYTES}-byte limit"
            ));
        }
        Ok(lines)
    }
}

/// Time allowed to establish the TCP + TLS connection to a provider. A
/// host we cannot reach at all fails here instead of hanging the turn.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);

/// Idle timeout *between* reads. reqwest resets this after every successful
/// read, so it bounds silence on the socket, not total duration — a long,
/// healthy streaming completion that keeps emitting tokens (or the
/// provider's periodic SSE keep-alives) is never cut off, but a socket that
/// goes quiet after the headers, or stalls mid-stream, is torn down instead
/// of blocking the agent loop forever. Deliberately NOT a total request
/// timeout, which would kill a legitimate slow-but-progressing stream.
const READ_TIMEOUT: Duration = Duration::from_secs(300);

/// Shared HTTP client for every provider. `Client::new()` installs NO
/// timeouts of any kind, so a stalled connection or a hung SSE stream
/// blocks the agent turn with no way out — the DoS/hang this guards
/// against. Falls back to a bare client only if the TLS backend fails to
/// initialize (not expected in practice).
pub(crate) fn http_client() -> reqwest::Client {
    client_with_timeouts(CONNECT_TIMEOUT, READ_TIMEOUT)
}

/// Browser-like User-Agent for LLM provider requests. Some API endpoints sit
/// behind Cloudflare, which blocks a default `reqwest/x.y` User-Agent as a bot
/// (HTTP 403 "error code: 1010"); a browser UA passes. Overridable via the
/// `ALVA_HTTP_USER_AGENT` env var (same var alva-bot's probes read); the
/// default below is used when it is unset or empty. Applies to every model call.
const DEFAULT_LLM_HTTP_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/124.0.0.0 Safari/537.36";

fn llm_http_user_agent() -> String {
    std::env::var("ALVA_HTTP_USER_AGENT")
        .ok()
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| DEFAULT_LLM_HTTP_USER_AGENT.to_string())
}

fn client_with_timeouts(connect: Duration, read: Duration) -> reqwest::Client {
    reqwest::Client::builder()
        .user_agent(llm_http_user_agent())
        .connect_timeout(connect)
        .read_timeout(read)
        // Provider requests carry API keys or arbitrary custom auth headers.
        // Never replay those credentials to a redirect target; callers must
        // configure the final provider endpoint explicitly.
        .redirect(reqwest::redirect::Policy::none())
        .build()
        .unwrap_or_else(|_| {
            reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .build()
                .expect("reqwest client without redirects must initialize")
        })
}

/// Byte budget for the request/response `body_preview` fields in provider
/// debug logs. Defaults to 500 so console logs stay readable, but is
/// overridable via `ALVA_LOG_BODY_PREVIEW_CHARS` — raise it (e.g. to 20000)
/// when capturing full multi-turn requests to a debug log file to diagnose a
/// provider-side 400 whose offending field sits past the first 500 bytes.
/// Only affects the body previews; the small SSE-chunk previews keep their own
/// fixed limits.
fn body_preview_len() -> usize {
    std::env::var("ALVA_LOG_BODY_PREVIEW_CHARS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|&n| n > 0)
        .unwrap_or(500)
}

/// Truncate an HTTP body for a `body_preview` log field, honoring
/// `ALVA_LOG_BODY_PREVIEW_CHARS` (see [`body_preview_len`]).
pub(crate) fn preview_body(s: &str) -> &str {
    truncate_for_log(s, body_preview_len())
}

/// UTF-8 safe slice for use in `tracing` macros' body/data/chunk
/// preview fields. Naive `&s[..s.len().min(N)]` panics if byte N
/// lands inside a multi-byte char — realistic for HTTP response
/// bodies and SSE chunks that include emoji or CJK. A panic in a
/// tracing call brings down the request handler.
///
/// Returns `&str` (not `String`) so it can be passed directly to
/// `tracing::debug!` / `tracing::warn!` without an extra allocation.
///
/// History: each provider (anthropic, openai_responses, openai_chat,
/// gemini) carried its own file-private copy of this helper across
/// L68-L71 before being consolidated here in L72.
pub(crate) fn truncate_for_log(s: &str, max_bytes: usize) -> &str {
    let mut end = s.len().min(max_bytes);
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

#[cfg(test)]
mod tests {
    //! Consolidated tests for `truncate_for_log`. These cover the
    //! pure helper itself; each provider file retains its
    //! integration-style regression test that exercises the helper
    //! via that provider's specific tracing call paths and locale
    //! scenario (Anthropic-Chinese / OpenAI-Responses-reasoning /
    //! OpenAI-Chat-Chinese / Gemini-Japanese).
    use super::*;

    #[test]
    fn short_string_returns_whole_string() {
        assert_eq!(truncate_for_log("hello", 500), "hello");
    }

    #[test]
    fn exact_byte_length_returns_whole_string() {
        let s = "a".repeat(500);
        assert_eq!(truncate_for_log(&s, 500), s.as_str());
    }

    #[test]
    fn long_ascii_truncates_at_max_bytes() {
        let s = "a".repeat(800);
        assert_eq!(truncate_for_log(&s, 500).len(), 500);
    }

    #[test]
    fn backs_off_from_mid_emoji_at_boundary() {
        // 498 ASCII + 🦀 (4 bytes) + tail = 602 bytes; byte 500 inside
        // emoji → back off to byte 498.
        let s = format!("{}{}{}", "a".repeat(498), "🦀", "b".repeat(100));
        assert_eq!(s.len(), 602);
        assert!(!s.is_char_boundary(500));
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
        assert!(!out.contains("🦀"));
    }

    #[test]
    fn backs_off_from_mid_cjk_at_boundary() {
        // CJK char "中" = 3 bytes. 200 of them = 600 bytes; byte 500
        // is inside the 167th char (bytes 498..501).
        let s = "中".repeat(200);
        assert_eq!(s.len(), 600);
        assert!(!s.is_char_boundary(500));
        let out = truncate_for_log(&s, 500);
        assert!(out.len() <= 500);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn three_hundred_byte_first_chunk_limit_pattern() {
        // openai_chat's first-chunk preview uses 300 bytes — verify
        // the helper handles this less-common limit correctly.
        let s = format!("{}{}{}", "x".repeat(298), "🦀", "y".repeat(100));
        assert_eq!(s.len(), 402);
        assert!(!s.is_char_boundary(300));
        let out = truncate_for_log(&s, 300);
        assert!(out.len() <= 300);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn sse_chunk_200_byte_limit_pattern() {
        let s = format!("{}{}{}", "x".repeat(198), "🦀", "y".repeat(50));
        assert_eq!(s.len(), 252);
        assert!(!s.is_char_boundary(200));
        let out = truncate_for_log(&s, 200);
        assert!(out.len() <= 200);
        assert!(out.is_char_boundary(out.len()));
    }

    #[test]
    fn zero_max_bytes_returns_empty() {
        assert_eq!(truncate_for_log("anything", 0), "");
    }

    #[test]
    fn sse_line_buffer_preserves_utf8_split_across_chunks() {
        let payload = "data: {\"text\":\"中文\"}\n\n".as_bytes();
        let split = payload
            .windows(3)
            .position(|window| window == "中".as_bytes())
            .unwrap()
            + 1;
        let mut buffer = SseLineBuffer::new();
        assert!(buffer.push(&payload[..split]).unwrap().is_empty());
        assert_eq!(
            buffer.push(&payload[split..]).unwrap(),
            vec!["data: {\"text\":\"中文\"}", ""]
        );
    }

    #[test]
    fn sse_line_buffer_rejects_unbounded_unterminated_frame() {
        let mut buffer = SseLineBuffer::new();
        let error = buffer
            .push(&vec![b'x'; MAX_SSE_LINE_BYTES + 1])
            .unwrap_err();
        assert!(error.contains("unterminated SSE line"), "{error}");
    }
}

#[cfg(test)]
mod http_client_tests {
    use super::client_with_timeouts;
    use std::time::Duration;
    use tokio::io::AsyncReadExt;
    use tokio::net::TcpListener;

    /// Pins the reason `http_client()` sets a read timeout: a provider that
    /// completes the TCP handshake, swallows the request, and then goes
    /// silent forever must NOT hang the agent turn. A client built with
    /// `Client::new()` (no timeouts) waits on such a socket indefinitely;
    /// our client tears it down on the read timeout.
    #[tokio::test]
    async fn read_timeout_fires_on_a_server_that_accepts_then_stalls() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        // Accept the connection, read the request, then stall — holding the
        // socket open (no close, no bytes) well past the client's timeout so
        // the client cannot observe an EOF either.
        tokio::spawn(async move {
            let (mut sock, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 1024];
            let _ = sock.read(&mut buf).await;
            tokio::time::sleep(Duration::from_secs(30)).await;
            drop(sock);
        });

        let client = client_with_timeouts(Duration::from_secs(5), Duration::from_millis(200));

        // Outer guard: with the read timeout the client aborts itself in
        // ~200ms. A client with no read timeout hangs here until this 5s
        // guard trips — exactly the regression this test locks down, so the
        // `.expect` below is the mutation signal.
        let result = tokio::time::timeout(
            Duration::from_secs(5),
            client.get(format!("http://{addr}/")).send(),
        )
        .await
        .expect("client must abort on its own read timeout, not hang past the 5s guard");

        let err = result.expect_err("a stalled server must not yield a successful response");
        assert!(err.is_timeout(), "expected a timeout error, got: {err:?}");
    }
}
