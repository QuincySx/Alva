use alva_app_debug::{ActionRegistry, DebugServer, LogCaptureLayer, RegisteredView};
use std::io::Read;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[test]
fn health_endpoint() {
    let server = DebugServer::builder().port(19230).build().unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19230", "/api/health");
    assert!(resp.contains("ok"), "expected 'ok' in response: {}", resp);

    handle.shutdown();
}

#[test]
fn log_query_and_level_control() {
    let (layer, log_handle) = LogCaptureLayer::new(1000);
    let _guard = tracing_subscriber::registry().with(layer).set_default();

    let server = DebugServer::builder()
        .port(19231)
        .with_log_handle(log_handle)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    tracing::info!(target: "test_mod", "hello");
    tracing::warn!(target: "test_mod", "warning");

    let resp = http_get("127.0.0.1:19231", "/api/logs");
    assert!(resp.contains("hello"), "expected 'hello' in: {}", resp);
    assert!(resp.contains("warning"), "expected 'warning' in: {}", resp);

    let resp = http_get("127.0.0.1:19231", "/api/logs?level=warn");
    assert!(
        !resp.contains("hello"),
        "should not contain 'hello' in: {}",
        resp
    );
    assert!(
        resp.contains("warning"),
        "expected 'warning' in: {}",
        resp
    );

    // Check current log level
    let resp = http_get("127.0.0.1:19231", "/api/logs/level");
    assert!(resp.contains("trace"), "expected 'trace' in: {}", resp);

    // Change log level
    let resp = http_put(
        "127.0.0.1:19231",
        "/api/logs/level",
        r#"{"filter": "warn"}"#,
    );
    assert!(resp.contains("ok"), "expected 'ok' in: {}", resp);

    // Verify it changed
    let resp = http_get("127.0.0.1:19231", "/api/logs/level");
    assert!(resp.contains("warn"), "expected 'warn' in: {}", resp);

    handle.shutdown();
}

#[test]
fn inspect_tree_without_inspector() {
    let server = DebugServer::builder().port(19232).build().unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19232", "/api/inspect/tree");
    assert!(resp.contains("error"), "expected 'error' in: {}", resp);
    assert!(
        resp.contains("not registered"),
        "expected 'not registered' in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn unknown_endpoint_returns_404() {
    let server = DebugServer::builder().port(19233).build().unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19233", "/api/nonexistent");
    assert!(resp.contains("error"), "expected 'error' in: {}", resp);
    assert!(
        resp.contains("unknown endpoint"),
        "expected 'unknown endpoint' in: {}",
        resp
    );

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// New endpoint tests: action, inspect/state, inspect/views, shutdown
// ---------------------------------------------------------------------------

fn make_test_registry() -> Arc<ActionRegistry> {
    let registry = Arc::new(ActionRegistry::new());
    registry.register(
        "chat_panel",
        RegisteredView {
            action_fn: Box::new(|method, args| match method {
                "send_message" => {
                    let text = args
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("empty");
                    Ok(serde_json::json!({"sent": text}))
                }
                _ => Err(format!("unknown method: {method}")),
            }),
            state_fn: Box::new(|| Some(serde_json::json!({"messages": 3, "loading": false}))),
            methods: vec!["send_message".into(), "clear".into()],
        },
    );
    registry
}

#[test]
fn action_endpoint_success() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19240)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_post(
        "127.0.0.1:19240",
        "/api/action",
        r#"{"target":"chat_panel","method":"send_message","args":{"text":"hello"}}"#,
    );
    assert!(resp.contains(r#""ok":true"#), "expected ok in: {}", resp);
    assert!(
        resp.contains(r#""sent":"hello""#),
        "expected sent result in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_endpoint_unknown_target() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19241)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_post(
        "127.0.0.1:19241",
        "/api/action",
        r#"{"target":"nope","method":"foo","args":{}}"#,
    );
    assert!(
        resp.contains("target_not_found"),
        "expected target_not_found in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_endpoint_unknown_method() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19242)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_post(
        "127.0.0.1:19242",
        "/api/action",
        r#"{"target":"chat_panel","method":"nonexistent","args":{}}"#,
    );
    assert!(
        resp.contains("method_not_found"),
        "expected method_not_found in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_endpoint_without_registry() {
    let server = DebugServer::builder().port(19243).build().unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_post(
        "127.0.0.1:19243",
        "/api/action",
        r#"{"target":"x","method":"y","args":{}}"#,
    );
    assert!(
        resp.contains("not registered"),
        "expected 'not registered' in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn inspect_state_endpoint() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19244)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19244", "/api/inspect/state?view=chat_panel");
    assert!(
        resp.contains(r#""view":"chat_panel""#),
        "expected view field in: {}",
        resp
    );
    assert!(
        resp.contains(r#""messages":3"#),
        "expected messages in state: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn inspect_state_unknown_view() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19245)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19245", "/api/inspect/state?view=unknown");
    assert!(
        resp.contains("state_error"),
        "expected state_error in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn inspect_state_missing_param() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19246)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19246", "/api/inspect/state");
    assert!(
        resp.contains("missing"),
        "expected 'missing' error in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn inspect_views_endpoint() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(19247)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19247", "/api/inspect/views");
    assert!(
        resp.contains("chat_panel"),
        "expected 'chat_panel' in views list: {}",
        resp
    );
    assert!(
        resp.contains("send_message"),
        "expected 'send_message' method in views: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn inspect_views_without_registry() {
    let server = DebugServer::builder().port(19248).build().unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19248", "/api/inspect/views");
    assert!(
        resp.contains("not registered"),
        "expected 'not registered' in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn shutdown_endpoint() {
    let shutdown_flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let server = DebugServer::builder()
        .port(19249)
        .with_shutdown_flag(shutdown_flag.clone())
        .build()
        .unwrap();
    let mut handle = server.start();
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_post("127.0.0.1:19249", "/api/shutdown", "");
    assert!(resp.contains(r#""ok":true"#), "expected ok in: {}", resp);
    assert!(
        shutdown_flag.load(std::sync::atomic::Ordering::SeqCst),
        "expected shutdown flag to be set"
    );

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// HTTP helpers using raw std::net (zero extra dependencies)
// ---------------------------------------------------------------------------

fn http_get(addr: &str, path: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    use std::io::Write;
    write!(
        stream,
        "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n",
        path, addr
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    extract_body(&response)
}

fn http_post(addr: &str, path: &str, body: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    use std::io::Write;
    write!(
        stream,
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, addr, body.len(), body
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    extract_body(&response)
}

fn http_put(addr: &str, path: &str, body: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    use std::io::Write;
    write!(
        stream,
        "PUT {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, addr, body.len(), body
    )
    .unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    extract_body(&response)
}

/// Extract the response body, handling both regular and chunked transfer encoding.
fn extract_body(raw: &str) -> String {
    let Some((_headers, body_part)) = raw.split_once("\r\n\r\n") else {
        return String::new();
    };

    // If the response uses chunked transfer encoding, decode it.
    if raw.contains("Transfer-Encoding: chunked") {
        decode_chunked(body_part)
    } else {
        body_part.to_string()
    }
}

/// Minimal chunked transfer encoding decoder.
fn decode_chunked(data: &str) -> String {
    let mut result = String::new();
    let mut remaining = data;

    loop {
        // Find the chunk size line
        let Some(newline_pos) = remaining.find("\r\n") else {
            break;
        };
        let size_str = remaining[..newline_pos].trim();
        let size = usize::from_str_radix(size_str, 16).unwrap_or(0);
        if size == 0 {
            break;
        }
        let chunk_start = newline_pos + 2;
        let chunk_end = chunk_start + size;
        if chunk_end > remaining.len() {
            // Grab whatever is available
            result.push_str(&remaining[chunk_start..]);
            break;
        }
        result.push_str(&remaining[chunk_start..chunk_end]);
        // Skip past the chunk data and trailing \r\n
        remaining = &remaining[(chunk_end + 2).min(remaining.len())..];
    }

    result
}
