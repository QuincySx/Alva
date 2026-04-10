//! HTTP-level E2E test: mock OpenAI server -> Provider -> Agent -> Events.
//!
//! Starts a local TCP server speaking the OpenAI SSE streaming protocol,
//! sends a real HTTP request through OpenAIChatProvider -> BaseAgent -> streaming
//! events. Tests the full network -> provider -> agent -> event chain.

use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

use alva_app_core::base_agent::BaseAgent;
use alva_app_core::AgentEvent;
use alva_llm_provider::{ProviderConfig, OpenAIChatProvider};
use alva_types::{LanguageModel, StreamEvent};

// ---------------------------------------------------------------------------
// Mock OpenAI HTTP server
// ---------------------------------------------------------------------------

/// Start a minimal HTTP server that returns streaming chat completion responses.
///
/// Handles `POST /chat/completions` with SSE text streaming.
/// Returns the server URL and a JoinHandle (server runs until dropped).
async fn start_mock_openai_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{}", addr);

    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };

            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let request = String::from_utf8_lossy(&buf[..n]);

                if request.contains("POST") && request.contains("/chat/completions") {
                    // SSE streaming response mimicking the OpenAI API
                    let sse_body = [
                        r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                        "",
                        r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Hello "}}]}"#,
                        "",
                        r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"from "}}]}"#,
                        "",
                        r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"mock!"}}]}"#,
                        "",
                        r#"data: {"id":"chatcmpl-test","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":10,"completion_tokens":3,"total_tokens":13}}"#,
                        "",
                        "data: [DONE]",
                        "",
                        "",
                    ]
                    .join("\n");

                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: text/event-stream\r\n\
                         Cache-Control: no-cache\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {}",
                        sse_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                } else {
                    let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            });
        }
    });

    (url, handle)
}

/// Start a mock server that returns a tool call followed by a text response.
async fn start_mock_openai_tool_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind to ephemeral port");
    let addr = listener.local_addr().expect("local addr");
    let url = format!("http://{}", addr);

    let call_count = Arc::new(std::sync::atomic::AtomicUsize::new(0));

    let handle = tokio::spawn(async move {
        loop {
            let (mut stream, _) = match listener.accept().await {
                Ok(conn) => conn,
                Err(_) => break,
            };
            let call_count = call_count.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 16384];
                let n = stream.read(&mut buf).await.unwrap_or(0);
                if n == 0 {
                    return;
                }
                let request = String::from_utf8_lossy(&buf[..n]);

                if request.contains("POST") && request.contains("/chat/completions") {
                    let count = call_count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);

                    let sse_body = if count == 0 {
                        // First call: return a tool call
                        [
                            r#"data: {"id":"chatcmpl-tc","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                            "",
                            r#"data: {"id":"chatcmpl-tc","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"id":"call_http_1","function":{"name":"my_http_tool","arguments":"{\"q\":"}}]}}]}"#,
                            "",
                            r#"data: {"id":"chatcmpl-tc","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"tool_calls":[{"index":0,"function":{"arguments":"\"test\"}"}}]}}]}"#,
                            "",
                            r#"data: {"id":"chatcmpl-tc","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":20,"completion_tokens":10,"total_tokens":30}}"#,
                            "",
                            "data: [DONE]",
                            "",
                            "",
                        ]
                        .join("\n")
                    } else {
                        // Second call: return text response
                        [
                            r#"data: {"id":"chatcmpl-final","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"role":"assistant"}}]}"#,
                            "",
                            r#"data: {"id":"chatcmpl-final","object":"chat.completion.chunk","choices":[{"index":0,"delta":{"content":"Tool result received!"}}]}"#,
                            "",
                            r#"data: {"id":"chatcmpl-final","object":"chat.completion.chunk","choices":[],"usage":{"prompt_tokens":30,"completion_tokens":5,"total_tokens":35}}"#,
                            "",
                            "data: [DONE]",
                            "",
                            "",
                        ]
                        .join("\n")
                    };

                    let response = format!(
                        "HTTP/1.1 200 OK\r\n\
                         Content-Type: text/event-stream\r\n\
                         Cache-Control: no-cache\r\n\
                         Connection: close\r\n\
                         \r\n\
                         {}",
                        sse_body
                    );
                    let _ = stream.write_all(response.as_bytes()).await;
                } else {
                    let response = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n";
                    let _ = stream.write_all(response.as_bytes()).await;
                }
            });
        }
    });

    (url, handle)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Drain events from an agent prompt, returning all collected events.
async fn collect_events(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<AgentEvent>,
) -> Vec<AgentEvent> {
    let mut events = Vec::new();
    while let Some(event) = rx.recv().await {
        let is_end = matches!(event, AgentEvent::AgentEnd { .. });
        events.push(event);
        if is_end {
            break;
        }
    }
    events
}

// ---------------------------------------------------------------------------
// Test: Full HTTP streaming pipeline — text only
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_http_streaming_full_pipeline() {
    let (server_url, _server_handle) = start_mock_openai_server().await;

    let config = ProviderConfig {
        api_key: "test-key".to_string(),
        model: "test-model".to_string(),
        base_url: server_url,
        max_tokens: 1000,
        custom_headers: std::collections::HashMap::new(),
    };
    let model: Arc<dyn LanguageModel> = Arc::new(OpenAIChatProvider::new(config));

    let tmp = tempfile::tempdir().expect("tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("You are a test bot.")
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Say hello");
    let events = collect_events(rx).await;

    // Collect streamed text
    let mut streamed_text = String::new();
    let mut got_usage = false;

    for event in &events {
        match event {
            AgentEvent::MessageUpdate { delta, .. } => match delta {
                StreamEvent::TextDelta { text } => {
                    streamed_text.push_str(text);
                }
                StreamEvent::Usage(u) => {
                    assert_eq!(u.total_tokens, 13);
                    got_usage = true;
                }
                _ => {}
            },
            _ => {}
        }
    }

    assert_eq!(
        streamed_text, "Hello from mock!",
        "streamed text should match mock server response"
    );
    assert!(got_usage, "should receive usage data from mock server");

    // Verify lifecycle
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::AgentStart)));
    assert!(events
        .iter()
        .any(|e| matches!(e, AgentEvent::MessageEnd { .. })));
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "should complete without error"
    );
}

// ---------------------------------------------------------------------------
// Test: Full HTTP pipeline with tool calls (streaming tool_calls SSE)
// ---------------------------------------------------------------------------

#[tokio::test]
async fn e2e_http_tool_call_pipeline() {
    let (server_url, _server_handle) = start_mock_openai_tool_server().await;

    let config = ProviderConfig {
        api_key: "test-key".to_string(),
        model: "test-model".to_string(),
        base_url: server_url,
        max_tokens: 1000,
        custom_headers: std::collections::HashMap::new(),
    };
    let model: Arc<dyn LanguageModel> = Arc::new(OpenAIChatProvider::new(config));

    let mock_tool =
        alva_test::mock_tool::MockTool::new("my_http_tool")
            .with_result(alva_types::ToolOutput::text("http tool result"));
    let mock_tool_clone = mock_tool.clone();

    let tmp = tempfile::tempdir().expect("tempdir");
    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("Test.")
        .tool(Box::new(mock_tool))
        .build(model)
        .await
        .expect("build");

    let rx = agent.prompt_text("Use the tool.");
    let events = collect_events(rx).await;

    // Verify tool was called via HTTP streaming
    let got_tool_start = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionStart { tool_call } if tool_call.name == "my_http_tool"));
    assert!(
        got_tool_start,
        "should see ToolExecutionStart for my_http_tool"
    );

    let got_tool_end = events
        .iter()
        .any(|e| matches!(e, AgentEvent::ToolExecutionEnd { .. }));
    assert!(got_tool_end, "should see ToolExecutionEnd");

    // Verify the final text response came through
    let mut final_text = String::new();
    for event in &events {
        if let AgentEvent::MessageUpdate {
            delta: StreamEvent::TextDelta { text },
            ..
        } = event
        {
            final_text.push_str(text);
        }
    }
    assert!(
        final_text.contains("Tool result received"),
        "should contain final text response, got: '{}'",
        final_text
    );

    // Mock tool should have been called
    assert_eq!(
        mock_tool_clone.calls().len(),
        1,
        "tool should have been called once"
    );

    // Verify args were streamed and reassembled correctly
    let tool_args = &mock_tool_clone.calls()[0];
    assert_eq!(
        tool_args,
        &serde_json::json!({"q": "test"}),
        "streamed tool args should be reassembled correctly"
    );

    assert!(
        events
            .iter()
            .any(|e| matches!(e, AgentEvent::AgentEnd { error: None })),
        "should complete without error"
    );
}
