//! End-to-end test: gateway HTTP server → mock upstream OpenAI Chat → response.
//!
//! Topology:
//!   reqwest client
//!     →  gateway (axum, ephemeral port)
//!         →  mock OpenAI server (tokio TcpListener, ephemeral port)
//!
//! Covers:
//!   - Non-streaming:  OpenAI Responses inbound → OpenAI Chat upstream → OpenAI Responses outbound.
//!   - Streaming:      OpenAI Responses inbound (stream:true) → OpenAI Chat upstream SSE →
//!                     OpenAI Responses SSE outbound with monotonically-increasing sequence_number.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use alva_app_gateway::app;
use alva_llm_provider::{AliasRouter, ProviderConfig};

// ---------------------------------------------------------------------------
// Test: image content block is rejected with 400 before upstream is contacted
// ---------------------------------------------------------------------------

/// Verify that a `/v1/messages` request containing an image content block
/// returns HTTP 400 at the decode_request step — before any upstream call is
/// made.  The "upstream" URL points at a port that is never bound, which
/// proves the rejection is purely in-process.
#[tokio::test]
async fn gateway_image_input_returns_400() {
    // Set a dummy env var so build_router succeeds (api_key_env lookup).
    std::env::set_var("DUMMY_GW_KEY_IMAGE_TEST", "dummy-key");

    // Build an AliasRouter with one Anthropic-kind route pointing at an
    // unreachable address — the request must be rejected before any upstream
    // connection attempt.
    let mut router = AliasRouter::new();
    router.insert(
        "claude-x".into(),
        ProviderConfig {
            kind: Some("anthropic".into()),
            base_url: "http://127.0.0.1:19999".into(), // nothing listening here
            api_key: "dummy-key".into(),
            model: "claude-3-5-sonnet-20241022".into(),
            max_tokens: 1024,
            custom_headers: Default::default(),
        },
    );

    // Start the gateway on an ephemeral port.
    let gw_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let gw_addr = gw_listener.local_addr().expect("gateway addr");
    let gw_url = format!("http://{}", gw_addr);

    let gw_router = app(Arc::new(router));
    let _gw_handle = tokio::spawn(async move {
        axum::serve(gw_listener, gw_router).await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // POST /v1/messages with an image content block (Anthropic wire format).
    // decode_request in AnthropicAdapter returns AdapterError::UnexpectedFormat
    // for any block with type == "image", which the gateway maps to HTTP 400.
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/v1/messages"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "claude-x",
            "messages": [{
                "role": "user",
                "content": [{
                    "type": "image",
                    "source": {
                        "type": "base64",
                        "media_type": "image/png",
                        "data": "iVBOR"
                    }
                }]
            }]
        }))
        .send()
        .await
        .expect("send image-input request to gateway");

    assert_eq!(
        resp.status().as_u16(),
        400,
        "image input must be rejected with HTTP 400 (decode_request fires before upstream)"
    );

    // Optionally verify the error body mentions the rejection cause.
    let body: serde_json::Value = resp.json().await.expect("parse error response as JSON");
    let msg = body
        .pointer("/error/message")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    assert!(
        msg.contains("image"),
        "error message should mention 'image', got: '{msg}'"
    );
}

// ---------------------------------------------------------------------------
// Mock upstream OpenAI-Chat-compatible server (non-streaming)
// ---------------------------------------------------------------------------

/// Start a minimal HTTP server that accepts one non-streaming chat.completions
/// request and replies with a canned JSON response.
///
/// Returns: `(base_url, seen_path_handle, server_join_handle)`.
/// `seen_path_handle` is populated with the request path once the request
/// arrives, so the test can assert that the gateway sent to the right route.
async fn start_mock_openai_chat_server() -> (String, Arc<Mutex<String>>, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock server");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{}", addr);

    let seen_path: Arc<Mutex<String>> = Arc::new(Mutex::new(String::new()));
    let seen_path_clone = seen_path.clone();

    let handle = tokio::spawn(async move {
        // Accept connections in a loop so the gateway can re-connect if
        // needed (reqwest may open a new connection per request).
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            let seen_path_inner = seen_path_clone.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 32768];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let request = String::from_utf8_lossy(&buf[..n]);

                // Capture the request-line path
                if let Some(line) = request.lines().next() {
                    // e.g. "POST /chat/completions HTTP/1.1"
                    let parts: Vec<&str> = line.splitn(3, ' ').collect();
                    if parts.len() >= 2 {
                        *seen_path_inner.lock().unwrap() = parts[1].to_string();
                    }
                }

                if request.contains("POST") && request.contains("/chat/completions") {
                    // Canned non-streaming chat.completion response
                    let body = r#"{
                        "id": "chatcmpl-mock001",
                        "object": "chat.completion",
                        "model": "real-model",
                        "choices": [{
                            "index": 0,
                            "message": {
                                "role": "assistant",
                                "content": "hi from upstream"
                            },
                            "finish_reason": "stop"
                        }],
                        "usage": {
                            "prompt_tokens": 5,
                            "completion_tokens": 3,
                            "total_tokens": 8
                        }
                    }"#;

                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: application/json\r\n\
                         Content-Length: {}\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                } else {
                    let response =
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            });
        }
    });

    (base_url, seen_path, handle)
}

// ---------------------------------------------------------------------------
// Mock upstream OpenAI-Chat SSE streaming server
// ---------------------------------------------------------------------------

/// Start a minimal HTTP server that accepts a streaming chat.completions
/// request and replies with an INTERLEAVED SSE stream:
///
/// role chunk → text "A" → tool_call start → tool_call args (two chunks) →
/// text "B" → finish_reason chunk → usage chunk → `data: [DONE]`
///
/// This interleaving is the pathological case that tests the gateway's
/// sequence_number monotonicity invariant across text and tool-call deltas.
///
/// Returns: `(base_url, server_join_handle)`.
async fn start_mock_openai_chat_sse_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind mock SSE server");
    let addr = listener.local_addr().expect("local addr");
    let base_url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(c) => c,
                Err(_) => break,
            };
            tokio::spawn(async move {
                let mut buf = vec![0u8; 32768];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let request = String::from_utf8_lossy(&buf[..n]);

                if !(request.contains("POST") && request.contains("/chat/completions")) {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    ).await;
                    return;
                }

                // SSE response headers — no Content-Length, chunked/streaming.
                let headers = "HTTP/1.1 200 OK\r\n\
                    Content-Type: text/event-stream\r\n\
                    Cache-Control: no-cache\r\n\
                    Connection: close\r\n\
                    \r\n";
                let _ = stream.write_all(headers.as_bytes()).await;

                // Helper: write one `data: <json>\n\n` SSE line.
                // OpenAI Chat Completions does NOT emit named `event:` lines —
                // only `data:` lines (unlike the Responses API).
                macro_rules! sse {
                    ($json:expr) => {{
                        let line = format!("data: {}\n\n", $json);
                        let _ = stream.write_all(line.as_bytes()).await;
                    }};
                }

                // 1. Role chunk (opening frame — assistant role)
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"role":"assistant","content":null},"finish_reason":null}]}"#);

                // 2. Text delta "A"
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"content":"A"},"finish_reason":null}]}"#);

                // 3. Tool call start — id + name (first appearance of index 0)
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_tool1","type":"function","function":{"name":"get_weather","arguments":""}}]},"finish_reason":null}]}"#);

                // 4. Tool call args partial "{\"loc"
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"{\"loc"}}]},"finish_reason":null}]}"#);

                // 5. Tool call args continuation "ation\":\"SF\"}"
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"ation\":\"SF\"}"}}]},"finish_reason":null}]}"#);

                // 6. Text delta "B" (interleaved AFTER tool call args, before finish)
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{"content":"B"},"finish_reason":null}]}"#);

                // 7. Finish reason chunk
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[{"index":0,"delta":{},"finish_reason":"tool_calls"}]}"#);

                // 8. Usage chunk (stream_options.include_usage format — empty choices)
                sse!(r#"{"id":"chatcmpl-interleaved","object":"chat.completion.chunk","model":"real-model","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":5,"total_tokens":15}}"#);

                // 9. [DONE] sentinel
                let _ = stream.write_all(b"data: [DONE]\n\n").await;
            });
        }
    });

    (base_url, handle)
}

// ---------------------------------------------------------------------------
// E2E test: Responses → Chat upstream → Responses response (non-streaming)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gateway_e2e_responses_to_chat_non_streaming() {
    // 1. Start mock upstream
    let (mock_base_url, seen_path, _mock_handle) = start_mock_openai_chat_server().await;

    // 2. Build AliasRouter pointing at the mock
    let mut router = AliasRouter::new();
    router.insert(
        "gpt-x".into(),
        ProviderConfig {
            kind: Some("openai-chat".into()),
            base_url: mock_base_url.clone(),
            api_key: "test-key".into(),
            model: "real-model".into(),
            max_tokens: 1024,
            custom_headers: Default::default(),
        },
    );

    // 3. Start the gateway on an ephemeral port
    let gw_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let gw_addr = gw_listener.local_addr().expect("gateway addr");
    let gw_url = format!("http://{}", gw_addr);

    let gw_router = app(Arc::new(router));
    let _gw_handle = tokio::spawn(async move {
        axum::serve(gw_listener, gw_router).await.ok();
    });

    // Give the server a moment to start accepting connections
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // 4. Send a POST /v1/responses request (OpenAI Responses API format)
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/v1/responses"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-x",
            "input": [
                {
                    "role": "user",
                    "content": [
                        { "type": "input_text", "text": "hello" }
                    ]
                }
            ]
        }))
        .send()
        .await
        .expect("send request to gateway");

    // 5. Assert HTTP 200
    assert_eq!(
        resp.status().as_u16(),
        200,
        "gateway must return 200 for non-streaming request"
    );

    // 6. Parse response body
    let body: serde_json::Value = resp.json().await.expect("parse gateway response as JSON");

    // (a) Response object type matches Responses API shape
    assert_eq!(
        body["object"].as_str(),
        Some("response"),
        "Responses API object field must be 'response', got: {body}"
    );

    // (b) Model echo: gateway injects the real alias
    assert_eq!(
        body["model"].as_str(),
        Some("gpt-x"),
        "gateway must echo the model alias, got: {body}"
    );

    // (c) Upstream text reaches the client
    let output_text = body["output"]
        .as_array()
        .and_then(|arr| arr.first())
        .and_then(|item| item["content"].as_array())
        .and_then(|c| c.first())
        .and_then(|b| b["text"].as_str())
        .unwrap_or("");
    assert!(
        output_text.contains("hi from upstream"),
        "response must contain upstream text, got: {body}"
    );

    // (d) Mock saw the right path — proves Responses→Chat translation happened
    let path = seen_path.lock().unwrap().clone();
    assert_eq!(
        path, "/chat/completions",
        "gateway must have forwarded to /chat/completions (OpenAI Chat), got: '{path}'"
    );
}

// ---------------------------------------------------------------------------
// Test: unknown model returns 404
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gateway_unknown_model_returns_404() {
    let router = AliasRouter::new(); // empty — no routes

    let gw_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let gw_addr = gw_listener.local_addr().expect("gateway addr");
    let gw_url = format!("http://{}", gw_addr);

    let gw_router = app(Arc::new(router));
    let _gw_handle = tokio::spawn(async move {
        axum::serve(gw_listener, gw_router).await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/v1/responses"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "nonexistent-model",
            "input": [
                {
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "hello" }]
                }
            ]
        }))
        .send()
        .await
        .expect("send request");

    assert_eq!(
        resp.status().as_u16(),
        404,
        "unknown model must return 404"
    );
}

// ---------------------------------------------------------------------------
// Test: streaming SSE passthrough with interleaved text+tool stream
// ---------------------------------------------------------------------------

/// Parse SSE text into a list of (event_name: Option<String>, data: String) pairs.
/// Follows the SSE spec: blank lines separate events; `event:` and `data:` fields
/// on consecutive lines belong to the same event.
fn parse_sse_events(text: &str) -> Vec<(Option<String>, String)> {
    let mut events = Vec::new();
    let mut current_event: Option<String> = None;
    let mut current_data: Option<String> = None;

    for line in text.lines() {
        let line = line.trim_end_matches('\r');
        if line.is_empty() {
            // End of one SSE event block
            if let Some(data) = current_data.take() {
                events.push((current_event.take(), data));
            } else {
                current_event = None;
            }
        } else if let Some(ev) = line.strip_prefix("event:") {
            current_event = Some(ev.trim().to_string());
        } else if let Some(d) = line.strip_prefix("data:") {
            current_data = Some(d.trim().to_string());
        }
    }
    // Handle stream that doesn't end with a blank line
    if let Some(data) = current_data {
        events.push((current_event, data));
    }
    events
}

#[tokio::test]
async fn gateway_e2e_streaming_responses_interleaved() {
    // 1. Start mock upstream that emits an interleaved SSE stream
    let (mock_base_url, _mock_handle) = start_mock_openai_chat_sse_server().await;

    // 2. Build AliasRouter pointing at the mock
    let mut router = AliasRouter::new();
    router.insert(
        "gpt-x".into(),
        ProviderConfig {
            kind: Some("openai-chat".into()),
            base_url: mock_base_url.clone(),
            api_key: "test-key".into(),
            model: "real-model".into(),
            max_tokens: 1024,
            custom_headers: Default::default(),
        },
    );

    // 3. Start gateway on ephemeral port
    let gw_listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind gateway");
    let gw_addr = gw_listener.local_addr().expect("gateway addr");
    let gw_url = format!("http://{}", gw_addr);

    let gw_router = app(Arc::new(router));
    let _gw_handle = tokio::spawn(async move {
        axum::serve(gw_listener, gw_router).await.ok();
    });

    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    // 4. POST /v1/responses with stream:true (OpenAI Responses API format)
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{gw_url}/v1/responses"))
        .header("Content-Type", "application/json")
        .json(&serde_json::json!({
            "model": "gpt-x",
            "stream": true,
            "input": [
                {
                    "role": "user",
                    "content": [{ "type": "input_text", "text": "hello" }]
                }
            ]
        }))
        .send()
        .await
        .expect("send stream request to gateway");

    // 5. Assert HTTP 200 with SSE content-type
    assert_eq!(resp.status().as_u16(), 200, "streaming must return 200, got: {}", resp.status());
    let content_type = resp.headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    assert!(
        content_type.contains("text/event-stream"),
        "content-type must be text/event-stream, got: '{content_type}'"
    );

    // 6. Read the full SSE body (the mock stream is finite — upstream closes after [DONE])
    let body_text = resp.text().await.expect("read SSE body");

    // 7. Parse SSE events
    let events = parse_sse_events(&body_text);
    assert!(!events.is_empty(), "SSE stream must contain at least one event; body was:\n{body_text}");

    // 8. Collect event names and parse sequence_number values from JSON data frames.
    //    The OpenAI Responses adapter emits named `event:` lines; [DONE] is a plain
    //    data frame without a name.
    let event_names: Vec<Option<&str>> = events.iter()
        .map(|(ev, _)| ev.as_deref())
        .collect();

    // (a) First event must be "response.created"
    assert_eq!(
        event_names.first().and_then(|e| *e),
        Some("response.created"),
        "first SSE event must be 'response.created'; got events: {event_names:?}"
    );

    // (b) A terminal event (response.completed or response.incomplete) must appear
    let has_terminal = event_names.iter().any(|e| {
        matches!(e.as_deref(), Some("response.completed") | Some("response.incomplete"))
    });
    assert!(has_terminal, "stream must contain response.completed or response.incomplete; events: {event_names:?}");

    // (c) Collect sequence_numbers from all JSON data frames
    let mut seq_numbers: Vec<i64> = Vec::new();
    for (_, data) in &events {
        if data == "[DONE]" {
            continue;
        }
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(data) {
            if let Some(n) = v.get("sequence_number").and_then(|v| v.as_i64()) {
                seq_numbers.push(n);
            }
        }
    }
    assert!(
        !seq_numbers.is_empty(),
        "at least one frame must carry a sequence_number; body was:\n{body_text}"
    );

    // (d) All sequence_number values must be strictly monotonically increasing
    for window in seq_numbers.windows(2) {
        assert!(
            window[0] < window[1],
            "sequence_number must be strictly increasing across the whole stream: {seq_numbers:?}"
        );
    }

    // (e) Both text deltas and function_call delta events must appear
    let has_text_delta = event_names.iter().any(|e| e.as_deref() == Some("response.output_text.delta"));
    let has_tool_delta = event_names.iter().any(|e| {
        matches!(e.as_deref(), Some("response.function_call_arguments.delta") | Some("response.output_item.added"))
    });
    assert!(
        has_text_delta,
        "stream must contain response.output_text.delta frames; events: {event_names:?}"
    );
    assert!(
        has_tool_delta,
        "stream must contain tool-call delta/start frames; events: {event_names:?}"
    );
}
