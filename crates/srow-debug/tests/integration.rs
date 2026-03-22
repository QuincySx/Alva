use srow_debug::{DebugServer, LogCaptureLayer};
use std::io::Read;
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
