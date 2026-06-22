// HTTP integration tests for read_url (wiremock-based).
//
// These exercise the reqwest fetch path end-to-end: wiremock plays the
// remote server, read_url issues a real GET, and we assert on the
// tool's structured output (content / content_type / cached / error).
// The SSRF gate lives in SecurityMiddleware now (Loop D2), so a TestCtx
// with no bus bypasses it cleanly — read_url itself doesn't refuse
// loopback URLs.
//
// SHARED-STATE NOTE: read_url has a process-global LRU cache + per-
// domain rate limit (60s window, 10 req max). All wiremock servers
// bind to 127.0.0.1:RANDOM_PORT, and read_url's domain-extractor drops
// the port — so every test in this file shares the "127.0.0.1" rate-
// limit bucket. Keep each test to ≤1 fresh request (cache hits don't
// count) and the total fresh requests across the file ≤ ~6 to stay
// safely under the 10/min cap even under parallel test execution.

#![cfg(feature = "web")]

use std::any::Any;
use std::path::Path;

use alva_agent_extension_builtin::read_url::ReadUrlTool;
use alva_kernel_abi::{BusHandle, CancellationToken, Tool, ToolExecutionContext};
use serde_json::{json, Value};
use wiremock::matchers::{method, path as wm_path};
use wiremock::{Mock, MockServer, ResponseTemplate};

struct TestCtx {
    cancel: CancellationToken,
}

impl ToolExecutionContext for TestCtx {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
    fn session_id(&self) -> &str {
        "http-test"
    }
    fn workspace(&self) -> Option<&Path> {
        None
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn bus(&self) -> Option<&BusHandle> {
        // No bus → no SecurityGuard → SSRF gate is bypassed.
        // wiremock binds to 127.0.0.1 (loopback) which the SSRF gate
        // would otherwise block under default Some(Medium) threshold.
        None
    }
}

fn ctx() -> TestCtx {
    TestCtx {
        cancel: CancellationToken::new(),
    }
}

/// Parse read_url's success output (a JSON string) back into a Value.
/// read_url returns `ToolOutput::text(serde_json::to_string_pretty(...))`,
/// so the text payload IS pretty-printed JSON.
fn parse_success(out: &alva_kernel_abi::ToolOutput) -> Value {
    let text = out.model_text();
    serde_json::from_str(&text).expect("read_url success output must be valid JSON")
}

// ─── Test 1: text/plain body ──────────────────────────────────────────

#[tokio::test]
async fn fetch_text_body_passes_through_verbatim() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/plain"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/plain; charset=utf-8")
                .set_body_string("hello plain text"),
        )
        .mount(&server)
        .await;
    let url = format!("{}/plain", server.uri());

    let out = ReadUrlTool
        .execute(json!({ "url": url }), &ctx())
        .await
        .expect("fetch should succeed");

    assert!(!out.is_error);
    let value = parse_success(&out);
    assert_eq!(
        value.get("content").and_then(|v| v.as_str()),
        Some("hello plain text"),
        "text/plain must pass through without HTML conversion"
    );
    assert!(value
        .get("content_type")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .contains("text/plain"),);
    assert_eq!(value.get("truncated"), Some(&Value::Bool(false)));
}

// ─── Test 2: text/html → markdown ─────────────────────────────────────

#[tokio::test]
async fn fetch_html_converts_to_markdown() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/page"))
        .respond_with(
            // set_body_raw lets us control both body and content-type
            // explicitly; insert_header gets overwritten by some of
            // wiremock 0.6's set_body_* helpers, so we use the form that
            // takes both atomically.
            ResponseTemplate::new(200).set_body_raw(
                "<html><body><h1>Hello</h1>\
                 <p>Visit <a href=\"https://example.com\">link</a>.</p>\
                 <script>alert('XSS')</script></body></html>"
                    .as_bytes()
                    .to_vec(),
                "text/html; charset=utf-8",
            ),
        )
        .mount(&server)
        .await;
    let url = format!("{}/page", server.uri());

    let out = ReadUrlTool
        .execute(json!({ "url": url }), &ctx())
        .await
        .expect("fetch should succeed");

    assert!(!out.is_error);
    let value = parse_success(&out);
    let content = value
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or_default();
    // markdown heading + link rendered, script body stripped
    assert!(content.contains("# Hello"), "h1 missing in: {content}");
    assert!(
        content.contains("[link](https://example.com)"),
        "link not rendered in: {content}"
    );
    assert!(
        !content.contains("alert"),
        "script content leaked to LLM: {content}"
    );
}

// ─── Test 3: 4xx response → AgentError ────────────────────────────────

#[tokio::test]
async fn non_2xx_response_returns_tool_error() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/missing"))
        .respond_with(ResponseTemplate::new(404))
        .mount(&server)
        .await;
    let url = format!("{}/missing", server.uri());

    let err = ReadUrlTool
        .execute(json!({ "url": url }), &ctx())
        .await
        .expect_err("404 must surface as ToolError");
    let msg = format!("{err}");
    assert!(
        msg.contains("404") || msg.contains("HTTP"),
        "expected status mention: {msg}"
    );
}

// ─── Test 4: cache hit on second fetch ────────────────────────────────

#[tokio::test]
async fn second_fetch_of_same_url_hits_cache() {
    // Pin: the first request goes to wiremock; the second is served
    // from the global LRU cache without re-hitting the network. Mocked
    // with `expect(1)` so wiremock asserts on drop that exactly one
    // request arrived.
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(wm_path("/cacheme"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/plain")
                .set_body_string("cached body"),
        )
        .expect(1)
        .mount(&server)
        .await;
    let url = format!("{}/cacheme", server.uri());

    // First — populates cache
    let first = ReadUrlTool
        .execute(json!({ "url": url.clone() }), &ctx())
        .await
        .expect("first fetch ok");
    let first_value = parse_success(&first);
    assert_eq!(
        first_value.get("content").and_then(|v| v.as_str()),
        Some("cached body")
    );
    // First call: cache miss → no `cached` flag (or false).
    assert!(
        first_value
            .get("cached")
            .and_then(|v| v.as_bool())
            .unwrap_or(false)
            == false,
        "first call must not be marked cached"
    );

    // Second — should hit cache
    let second = ReadUrlTool
        .execute(json!({ "url": url }), &ctx())
        .await
        .expect("second fetch ok");
    let second_value = parse_success(&second);
    assert_eq!(
        second_value.get("content").and_then(|v| v.as_str()),
        Some("cached body"),
        "cache must return same body"
    );
    assert_eq!(
        second_value.get("cached"),
        Some(&Value::Bool(true)),
        "second call must be marked cached: {second_value:?}"
    );
    // server.drop() will verify expect(1) is satisfied (wiremock 0.6 verifies on Drop)
}
