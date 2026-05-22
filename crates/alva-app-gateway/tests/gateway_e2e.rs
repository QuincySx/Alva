//! End-to-end test: gateway HTTP server → mock upstream OpenAI Chat → response.
//!
//! Topology:
//!   reqwest client
//!     →  gateway (axum, ephemeral port)
//!         →  mock OpenAI server (tokio TcpListener, ephemeral port)
//!
//! Verifies the full non-streaming path:
//!   OpenAI Responses inbound → OpenAI Chat upstream → OpenAI Responses outbound.

use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use alva_app_gateway::app;
use alva_llm_provider::{AliasRouter, ProviderConfig};

// ---------------------------------------------------------------------------
// Mock upstream OpenAI-Chat-compatible server
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
// E2E test: Responses → Chat upstream → Responses response
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
// Test: streaming request returns 501
// ---------------------------------------------------------------------------

#[tokio::test]
async fn gateway_streaming_returns_501() {
    // Start mock (won't be reached for stream=true, but router needs a valid alias)
    let (mock_base_url, _seen_path, _mock_handle) = start_mock_openai_chat_server().await;

    let mut router = AliasRouter::new();
    router.insert(
        "gpt-x".into(),
        ProviderConfig {
            kind: Some("openai-chat".into()),
            base_url: mock_base_url,
            api_key: "test-key".into(),
            model: "real-model".into(),
            max_tokens: 1024,
            custom_headers: Default::default(),
        },
    );

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
        .expect("send stream request");

    assert_eq!(
        resp.status().as_u16(),
        501,
        "streaming must return 501 Not Implemented"
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
