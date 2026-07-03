// INPUT:  alva_app_debug::{ActionRegistry, DebugServer, LogCaptureLayer, RegisteredView}, std::net, tracing_subscriber
// OUTPUT: (none -- integration test module)
// POS:    End-to-end tests for the debug HTTP server covering health, logs, inspect, action, and shutdown endpoints.
use alva_app_debug::{ActionRegistry, DebugServer, LogCaptureLayer, RegisteredView};
use std::io::Read;
use std::sync::Arc;
use tracing_subscriber::prelude::*;

#[test]
fn health_endpoint() {
    let server = DebugServer::builder().port(0).build().unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/health");
    assert!(resp.contains("ok"), "expected 'ok' in response: {}", resp);

    handle.shutdown();
}

#[test]
fn log_query_and_level_control() {
    let (layer, log_handle) = LogCaptureLayer::new(1000);
    let _guard = tracing_subscriber::registry().with(layer).set_default();

    let server = DebugServer::builder()
        .port(0)
        .with_log_handle(log_handle)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    tracing::info!(target: "test_mod", "hello");
    tracing::warn!(target: "test_mod", "warning");

    let resp = http_get(&addr, "/api/logs");
    assert!(resp.contains("hello"), "expected 'hello' in: {}", resp);
    assert!(resp.contains("warning"), "expected 'warning' in: {}", resp);

    let resp = http_get(&addr, "/api/logs?level=warn");
    assert!(
        !resp.contains("hello"),
        "should not contain 'hello' in: {}",
        resp
    );
    assert!(resp.contains("warning"), "expected 'warning' in: {}", resp);

    // Check current log level
    let resp = http_get(&addr, "/api/logs/level");
    assert!(resp.contains("trace"), "expected 'trace' in: {}", resp);

    // Change log level
    let resp = http_put(&addr, "/api/logs/level", r#"{"filter": "warn"}"#);
    assert!(resp.contains("ok"), "expected 'ok' in: {}", resp);

    // Verify it changed
    let resp = http_get(&addr, "/api/logs/level");
    assert!(resp.contains("warn"), "expected 'warn' in: {}", resp);

    handle.shutdown();
}

#[test]
fn inspect_tree_without_inspector() {
    let server = DebugServer::builder().port(0).build().unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/tree");
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
    let server = DebugServer::builder().port(0).build().unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/nonexistent");
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
                    let text = args.get("text").and_then(|v| v.as_str()).unwrap_or("empty");
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
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
    let server = DebugServer::builder().port(0).build().unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/state?view=chat_panel");
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/state?view=unknown");
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/state");
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
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/views");
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
    let server = DebugServer::builder().port(0).build().unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_get(&addr, "/api/inspect/views");
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
        .port(0)
        .with_shutdown_flag(shutdown_flag.clone())
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(&addr, "/api/shutdown", "");
    assert!(resp.contains(r#""ok":true"#), "expected ok in: {}", resp);
    assert!(
        shutdown_flag.load(std::sync::atomic::Ordering::SeqCst),
        "expected shutdown flag to be set"
    );

    handle.shutdown();
}

// ---------------------------------------------------------------------------
// Loop 148 gap-fill: router.rs error paths not previously covered
//
// Pre-existing tests cover happy paths and the high-level "without
// registry" 503 cases. The following pin the validation error paths
// — malformed JSON and missing-field 400 responses — that frontend
// consumers rely on for actionable error messages. A refactor that
// surfaced these as 500 / hung connection / unwrap panic would
// silently degrade the developer experience.
// ---------------------------------------------------------------------------

#[test]
fn set_log_level_malformed_json_returns_400_with_error_message() {
    let (layer, log_handle) = LogCaptureLayer::new(100);
    let _guard = tracing_subscriber::registry().with(layer).set_default();
    let server = DebugServer::builder()
        .port(0)
        .with_log_handle(log_handle)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    // Body is not valid JSON at all — must NOT panic / 500; must 400
    // with a diagnostic that lets the frontend display "fix your JSON".
    let resp = http_put(&addr, "/api/logs/level", "{not json");
    assert!(
        resp.contains("malformed JSON body"),
        "expected diagnostic 'malformed JSON body' in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn set_log_level_missing_filter_field_returns_400_with_field_name() {
    let (layer, log_handle) = LogCaptureLayer::new(100);
    let _guard = tracing_subscriber::registry().with(layer).set_default();
    let server = DebugServer::builder()
        .port(0)
        .with_log_handle(log_handle)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    // Valid JSON but missing the required 'filter' field — diagnostic
    // MUST name the missing field by name so frontend can guide user.
    let resp = http_put(&addr, "/api/logs/level", r#"{"other_key": "warn"}"#);
    assert!(
        resp.contains("missing 'filter' field"),
        "expected diagnostic naming 'filter' field in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_malformed_json_returns_400_with_error_message() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(&addr, "/api/action", "{not json");
    assert!(
        resp.contains("malformed JSON body"),
        "expected diagnostic 'malformed JSON body' in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_missing_target_field_returns_400_with_field_name() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
        "/api/action",
        r#"{"method": "send_message", "args": {}}"#,
    );
    assert!(
        resp.contains("missing 'target' field"),
        "expected diagnostic naming 'target' field in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_missing_method_field_returns_400_with_field_name() {
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    let resp = http_post(
        &addr,
        "/api/action",
        r#"{"target": "chat_panel", "args": {}}"#,
    );
    assert!(
        resp.contains("missing 'method' field"),
        "expected diagnostic naming 'method' field in: {}",
        resp
    );

    handle.shutdown();
}

#[test]
fn action_with_missing_args_field_defaults_to_empty_object() {
    // SILENT BACKWARD-COMPAT CONTRACT: when the client omits `args`
    // entirely, router defaults it to `{}` rather than rejecting.
    // Frontend callers like `actions.call(target, method)` (no args)
    // rely on this. A refactor that started 400'ing missing-args
    // would silently break every action call site that doesn't
    // pass arguments.
    let registry = make_test_registry();
    let server = DebugServer::builder()
        .port(0)
        .with_action_registry(registry)
        .build()
        .unwrap();
    let addr = format!("127.0.0.1:{}", server.local_port());
    let mut handle = server.start();

    // make_test_registry registers chat_panel.send_message which
    // checks args.text — without args this hits a downstream error
    // (NOT a 400 from missing 'args' field). The router accepting
    // missing 'args' is what we pin here.
    let resp = http_post(
        &addr,
        "/api/action",
        r#"{"target": "chat_panel", "method": "send_message"}"#,
    );
    assert!(
        !resp.contains("missing 'args' field"),
        "router MUST NOT reject missing 'args' (it defaults to {{}}): {}",
        resp
    );
    // Sanity: the request reached the registry (got past JSON validation).
    // Either it returns ok (if method tolerates missing args) or returns
    // an error_type from the registry layer — both prove router defaulted
    // args correctly.
    assert!(
        resp.contains(r#""ok":true"#) || resp.contains(r#""ok":false"#),
        "expected a structured ok/error response from registry layer (not a 400 'missing args'): {}",
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
