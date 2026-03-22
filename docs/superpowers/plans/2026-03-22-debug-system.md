# srow-debug Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build a standalone debug crate with local HTTP server, structured log capture, and GPUI view tree inspection for AI-driven debugging.

**Architecture:** `srow-debug` crate with core layer (HTTP server + tracing Layer + ring buffer) and optional GPUI feature. Consumer (`srow-app`) wraps all usage in `#[cfg(debug_assertions)]`. Cross-thread GPUI snapshot via `std::sync::mpsc` channels.

**Tech Stack:** `tiny_http` (HTTP), `tracing`/`tracing-subscriber` (logging Layer), `parking_lot` (RwLock), `serde`/`serde_json` (serialization), `gpui` (optional feature).

**Spec:** `docs/superpowers/specs/2026-03-22-debug-system-design.md`

---

## File Structure

### New files (srow-debug crate)

| File | Responsibility |
|------|----------------|
| `crates/srow-debug/Cargo.toml` | Crate config, dependencies, gpui feature flag |
| `crates/srow-debug/src/lib.rs` | Public API re-exports, module declarations |
| `crates/srow-debug/src/log_store.rs` | `LogRecord`, `LogQuery`, `LogStore` ring buffer |
| `crates/srow-debug/src/log_layer.rs` | `LogCaptureLayer` (tracing Layer) + `LogHandle` |
| `crates/srow-debug/src/inspect.rs` | `InspectNode`, `Bounds`, `Inspectable` trait, `DebugInspect` trait |
| `crates/srow-debug/src/server.rs` | `tiny_http` server loop, JSON response helpers |
| `crates/srow-debug/src/router.rs` | Route dispatch, request parsing, endpoint handlers |
| `crates/srow-debug/src/builder.rs` | `DebugServer`, `DebugServerBuilder`, `DebugServerHandle` |
| `crates/srow-debug/src/gpui/mod.rs` | `GpuiInspector` — cross-thread channel + `Inspectable` impl |

### Modified files

| File | Change |
|------|--------|
| `Cargo.toml` (workspace root) | Add `"crates/srow-debug"` to members |
| `crates/srow-app/Cargo.toml` | Add `srow-debug` dependency with `features = ["gpui"]` |
| `crates/srow-app/src/main.rs` | Migrate tracing init, start debug server |
| `crates/srow-core/src/bin/cli.rs` | Migrate tracing init to layered approach |
| `crates/srow-core/src/agent/runtime/engine/engine.rs` | Add `#[instrument]` spans to `run()`, `execute_tools()` |
| `crates/srow-core/src/agent/agent_client/session/client.rs` | Add spans to `handle_inbound()`, `send_prompt()` |
| `crates/srow-core/src/mcp/runtime.rs` | Add spans to `connect()`, `call_tool()` |
| `crates/srow-ai/src/chat/abstract_chat.rs` | Add spans to `send_message()` |

---

## Task 1: Crate scaffold + LogRecord + LogStore

**Files:**
- Create: `crates/srow-debug/Cargo.toml`
- Create: `crates/srow-debug/src/lib.rs`
- Create: `crates/srow-debug/src/log_store.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "srow-debug"
version = "0.1.0"
edition = "2021"

[features]
default = []
gpui = ["dep:gpui"]

[dependencies]
tiny_http = "0.12"
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter", "registry"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
parking_lot = "0.12"

[dependencies.gpui]
version = "0.2"
optional = true
```

- [ ] **Step 2: Create lib.rs with module declarations**

Start with only `mod log_store` — each subsequent task adds its module declaration when its file is created:

```rust
mod log_store;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
```

- [ ] **Step 3: Write LogStore tests first**

In `crates/srow-debug/src/log_store.rs`:

```rust
use parking_lot::RwLock;
use serde::Serialize;
use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};

#[derive(Debug, Clone, Serialize)]
pub struct LogRecord {
    pub seq: u64,
    pub timestamp: i64,
    pub level: String,
    pub target: String,
    pub message: String,
    pub fields: HashMap<String, serde_json::Value>,
    pub span_stack: Vec<String>,
}

#[derive(Debug, Default)]
pub struct LogQuery {
    pub level: Option<String>,
    pub module: Option<String>,
    pub since: Option<i64>,
    pub cursor: Option<u64>,
    pub keyword: Option<String>,
    pub limit: Option<usize>,
}

#[derive(Debug, Serialize)]
pub struct LogQueryResponse {
    pub total_matches: usize,
    pub records: Vec<LogRecord>,
}

pub struct LogStore {
    buffer: Vec<Option<LogRecord>>,
    capacity: usize,
    write_pos: usize,
    count: usize,
    seq_counter: u64,
}

impl LogStore {
    pub fn new(capacity: usize) -> Self {
        let mut buffer = Vec::with_capacity(capacity);
        buffer.resize_with(capacity, || None);
        Self {
            buffer,
            capacity,
            write_pos: 0,
            count: 0,
            seq_counter: 0,
        }
    }

    pub fn push(&mut self, mut record: LogRecord) {
        self.seq_counter += 1;
        record.seq = self.seq_counter;
        self.buffer[self.write_pos] = Some(record);
        self.write_pos = (self.write_pos + 1) % self.capacity;
        if self.count < self.capacity {
            self.count += 1;
        }
    }

    pub fn query(&self, params: &LogQuery) -> LogQueryResponse {
        let limit = params.limit.unwrap_or(100);
        let mut matches: Vec<&LogRecord> = Vec::new();

        // Iterate in chronological order
        let start = if self.count < self.capacity {
            0
        } else {
            self.write_pos
        };

        for i in 0..self.count {
            let idx = (start + i) % self.capacity;
            if let Some(ref record) = self.buffer[idx] {
                if self.matches_filter(record, params) {
                    matches.push(record);
                }
            }
        }

        let total_matches = matches.len();
        let records: Vec<LogRecord> = matches
            .into_iter()
            .rev()
            .take(limit)
            .rev()
            .cloned()
            .collect();

        LogQueryResponse {
            total_matches,
            records,
        }
    }

    fn matches_filter(&self, record: &LogRecord, params: &LogQuery) -> bool {
        if let Some(ref level) = params.level {
            if !self.level_matches(&record.level, level) {
                return false;
            }
        }
        if let Some(ref module) = params.module {
            if !record.target.starts_with(module) {
                return false;
            }
        }
        if let Some(since) = params.since {
            if record.timestamp < since {
                return false;
            }
        }
        if let Some(cursor) = params.cursor {
            if record.seq <= cursor {
                return false;
            }
        }
        if let Some(ref keyword) = params.keyword {
            if !record.message.contains(keyword) {
                return false;
            }
        }
        true
    }

    fn level_matches(&self, record_level: &str, min_level: &str) -> bool {
        let order = |l: &str| match l.to_uppercase().as_str() {
            "TRACE" => 0,
            "DEBUG" => 1,
            "INFO" => 2,
            "WARN" => 3,
            "ERROR" => 4,
            _ => 0,
        };
        order(record_level) >= order(min_level)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(level: &str, target: &str, message: &str, timestamp: i64) -> LogRecord {
        LogRecord {
            seq: 0,
            timestamp,
            level: level.to_string(),
            target: target.to_string(),
            message: message.to_string(),
            fields: HashMap::new(),
            span_stack: Vec::new(),
        }
    }

    #[test]
    fn push_and_query_basic() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "mod_a", "hello", 1000));
        store.push(make_record("WARN", "mod_b", "world", 2000));

        let result = store.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records.len(), 2);
        assert_eq!(result.records[0].seq, 1);
        assert_eq!(result.records[1].seq, 2);
    }

    #[test]
    fn ring_buffer_overflow() {
        let mut store = LogStore::new(3);
        for i in 0..5 {
            store.push(make_record("INFO", "m", &format!("msg{}", i), i as i64));
        }
        let result = store.query(&LogQuery::default());
        assert_eq!(result.total_matches, 3);
        // Should contain the last 3 records (msg2, msg3, msg4)
        assert_eq!(result.records[0].message, "msg2");
        assert_eq!(result.records[2].message, "msg4");
    }

    #[test]
    fn filter_by_level() {
        let mut store = LogStore::new(100);
        store.push(make_record("DEBUG", "m", "dbg", 1000));
        store.push(make_record("WARN", "m", "wrn", 2000));
        store.push(make_record("ERROR", "m", "err", 3000));

        let result = store.query(&LogQuery {
            level: Some("WARN".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].level, "WARN");
        assert_eq!(result.records[1].level, "ERROR");
    }

    #[test]
    fn filter_by_module_prefix() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "srow_core::agent::engine", "a", 1000));
        store.push(make_record("INFO", "srow_ai::chat", "b", 2000));

        let result = store.query(&LogQuery {
            module: Some("srow_core".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].target, "srow_core::agent::engine");
    }

    #[test]
    fn filter_by_cursor() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "m", "first", 1000));
        store.push(make_record("INFO", "m", "second", 2000));
        store.push(make_record("INFO", "m", "third", 3000));

        let result = store.query(&LogQuery {
            cursor: Some(1),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].message, "second");
    }

    #[test]
    fn filter_by_keyword() {
        let mut store = LogStore::new(100);
        store.push(make_record("INFO", "m", "connection failed", 1000));
        store.push(make_record("INFO", "m", "all good", 2000));

        let result = store.query(&LogQuery {
            keyword: Some("failed".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].message, "connection failed");
    }

    #[test]
    fn limit_results() {
        let mut store = LogStore::new(100);
        for i in 0..10 {
            store.push(make_record("INFO", "m", &format!("msg{}", i), i as i64));
        }
        let result = store.query(&LogQuery {
            limit: Some(3),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 10);
        assert_eq!(result.records.len(), 3);
        // Should return the LAST 3 (most recent)
        assert_eq!(result.records[0].message, "msg7");
        assert_eq!(result.records[2].message, "msg9");
    }

    #[test]
    fn combined_filters() {
        let mut store = LogStore::new(100);
        store.push(make_record("DEBUG", "srow_core::agent", "step 1", 1000));
        store.push(make_record("WARN", "srow_core::agent", "step 2 failed", 2000));
        store.push(make_record("WARN", "srow_ai::chat", "chat failed", 3000));
        store.push(make_record("ERROR", "srow_core::mcp", "mcp error", 4000));

        let result = store.query(&LogQuery {
            level: Some("WARN".to_string()),
            module: Some("srow_core".to_string()),
            keyword: Some("failed".to_string()),
            ..Default::default()
        });
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].message, "step 2 failed");
    }
}
```

- [ ] **Step 4: Add srow-debug to workspace members**

In root `Cargo.toml`, add `"crates/srow-debug"` to members list.

- [ ] **Step 5: Run tests**

Run: `cargo test -p srow-debug`
Expected: All 7 tests pass.

- [ ] **Step 6: Commit**

```bash
git add crates/srow-debug/ Cargo.toml
git commit -m "feat(srow-debug): add crate scaffold with LogStore ring buffer"
```

---

## Task 2: LogCaptureLayer + LogHandle

**Files:**
- Create: `crates/srow-debug/src/log_layer.rs`

- [ ] **Step 1: Write LogCaptureLayer test**

Append to `log_layer.rs`:

```rust
use std::sync::Arc;
use parking_lot::RwLock;
use tracing::Subscriber;
use tracing_subscriber::layer::Context;
use tracing_subscriber::registry::LookupSpan;
use tracing_subscriber::Layer;

use crate::log_store::{LogQuery, LogQueryResponse, LogRecord, LogStore};

pub struct LogCaptureLayer {
    store: Arc<RwLock<LogStore>>,
    filter: Arc<RwLock<String>>,
}

#[derive(Clone)]
pub struct LogHandle {
    store: Arc<RwLock<LogStore>>,
    filter: Arc<RwLock<String>>,
}

impl LogCaptureLayer {
    pub fn new(capacity: usize) -> (Self, LogHandle) {
        let store = Arc::new(RwLock::new(LogStore::new(capacity)));
        let filter = Arc::new(RwLock::new("trace".to_string()));
        let layer = Self {
            store: Arc::clone(&store),
            filter: Arc::clone(&filter),
        };
        let handle = LogHandle {
            store,
            filter,
        };
        (layer, handle)
    }
}

impl LogHandle {
    pub fn query(&self, params: &LogQuery) -> LogQueryResponse {
        self.store.read().query(params)
    }

    pub fn set_filter(&self, filter_str: &str) {
        let mut f = self.filter.write();
        *f = filter_str.to_string();
    }

    pub fn current_filter(&self) -> String {
        self.filter.read().clone()
    }
}

impl<S> Layer<S> for LogCaptureLayer
where
    S: Subscriber + for<'a> LookupSpan<'a>,
{
    fn on_event(&self, event: &tracing::Event<'_>, ctx: Context<'_, S>) {
        use std::collections::HashMap;
        use std::fmt;

        // Check dynamic filter
        let filter_str = self.filter.read().clone();
        if !should_capture(event, &filter_str) {
            return;
        }

        // Extract message
        let mut visitor = MessageVisitor::default();
        event.record(&mut visitor);

        // Extract span stack
        let mut span_stack = Vec::new();
        if let Some(scope) = ctx.event_scope(event) {
            for span in scope.from_root() {
                span_stack.push(span.name().to_string());
            }
        }

        let record = LogRecord {
            seq: 0, // assigned by LogStore::push
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64,
            level: event.metadata().level().to_string(),
            target: event.metadata().target().to_string(),
            message: visitor.message,
            fields: visitor.fields,
            span_stack,
        };

        self.store.write().push(record);
    }
}

fn should_capture(event: &tracing::Event<'_>, filter_str: &str) -> bool {
    // Simple level-based filtering
    // For module-level filtering like "srow_core=trace,srow_ai=debug",
    // parse the filter string and match against event target and level
    let target = event.metadata().target();
    let event_level = level_order(event.metadata().level());

    // Parse comma-separated directives: "module=level,module=level" or just "level"
    for directive in filter_str.split(',') {
        let directive = directive.trim();
        if directive.is_empty() {
            continue;
        }
        if let Some((module, level)) = directive.split_once('=') {
            if target.starts_with(module.trim()) {
                return event_level >= level_order_str(level.trim());
            }
        } else {
            // Global level directive
            return event_level >= level_order_str(directive);
        }
    }
    true // default: capture everything
}

fn level_order(level: &tracing::Level) -> u8 {
    match *level {
        tracing::Level::TRACE => 0,
        tracing::Level::DEBUG => 1,
        tracing::Level::INFO => 2,
        tracing::Level::WARN => 3,
        tracing::Level::ERROR => 4,
    }
}

fn level_order_str(s: &str) -> u8 {
    match s.to_lowercase().as_str() {
        "trace" => 0,
        "debug" => 1,
        "info" => 2,
        "warn" => 3,
        "error" => 4,
        _ => 0,
    }
}

#[derive(Default)]
struct MessageVisitor {
    message: String,
    fields: std::collections::HashMap<String, serde_json::Value>,
}

impl tracing::field::Visit for MessageVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
        if field.name() == "message" {
            self.message = format!("{:?}", value);
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(format!("{:?}", value)));
        }
    }

    fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
        if field.name() == "message" {
            self.message = value.to_string();
        } else {
            self.fields
                .insert(field.name().to_string(), serde_json::Value::String(value.to_string()));
        }
    }

    fn record_i64(&mut self, field: &tracing::field::Field, value: i64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_u64(&mut self, field: &tracing::field::Field, value: u64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_bool(&mut self, field: &tracing::field::Field, value: bool) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }

    fn record_f64(&mut self, field: &tracing::field::Field, value: f64) {
        self.fields
            .insert(field.name().to_string(), serde_json::json!(value));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tracing_subscriber::prelude::*;

    #[test]
    fn captures_log_events() {
        let (layer, handle) = LogCaptureLayer::new(100);

        let _guard = tracing_subscriber::registry()
            .with(layer)
            .set_default();

        tracing::info!(target: "test_mod", "hello world");
        tracing::warn!(target: "test_mod", server = "fs", "connection lost");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2);
        assert_eq!(result.records[0].level, "INFO");
        assert_eq!(result.records[1].level, "WARN");
        assert!(result.records[1].fields.contains_key("server"));
    }

    #[test]
    fn captures_span_stack() {
        let (layer, handle) = LogCaptureLayer::new(100);

        let _guard = tracing_subscriber::registry()
            .with(layer)
            .set_default();

        let outer = tracing::info_span!("outer_span");
        let _outer_guard = outer.enter();
        let inner = tracing::info_span!("inner_span");
        let _inner_guard = inner.enter();
        tracing::info!("nested event");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 1);
        assert_eq!(result.records[0].span_stack, vec!["outer_span", "inner_span"]);
    }

    #[test]
    fn dynamic_filter_change() {
        let (layer, handle) = LogCaptureLayer::new(100);

        let _guard = tracing_subscriber::registry()
            .with(layer)
            .set_default();

        // Default filter: "trace" — captures everything
        tracing::debug!(target: "test_mod", "debug msg");
        assert_eq!(handle.query(&LogQuery::default()).total_matches, 1);

        // Change filter to only capture WARN+
        handle.set_filter("warn");
        tracing::debug!(target: "test_mod", "should be filtered");
        tracing::warn!(target: "test_mod", "should pass");

        let result = handle.query(&LogQuery::default());
        assert_eq!(result.total_matches, 2); // 1 original debug + 1 warn (debug filtered)
    }
}
```

- [ ] **Step 2: Update lib.rs — add log_layer module**

```rust
mod log_store;
mod log_layer;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
pub use log_layer::{LogCaptureLayer, LogHandle};
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p srow-debug`
Expected: All tests pass (LogStore + LogCaptureLayer).

- [ ] **Step 4: Commit**

```bash
git add crates/srow-debug/src/log_layer.rs crates/srow-debug/src/lib.rs
git commit -m "feat(srow-debug): add LogCaptureLayer tracing Layer with dynamic filter"
```

---

## Task 3: Inspectable trait + InspectNode types

**Files:**
- Create: `crates/srow-debug/src/inspect.rs`

- [ ] **Step 1: Write inspect.rs**

```rust
use serde::Serialize;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize)]
pub struct InspectNode {
    pub id: String,
    pub type_name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bounds: Option<Bounds>,
    pub properties: HashMap<String, serde_json::Value>,
    pub children: Vec<InspectNode>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Bounds {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Framework-agnostic inspection trait.
/// Implementations produce a snapshot (InspectNode tree) that can be
/// sent across threads — the trait itself does NOT hold UI state.
pub trait Inspectable: Send + Sync {
    fn inspect(&self) -> InspectNode;
}

/// Opt-in trait for views to expose custom debug properties.
/// Guarded by #[cfg(debug_assertions)] — does not exist in release builds.
/// Application views must also guard their impl blocks with #[cfg(debug_assertions)].
#[cfg(debug_assertions)]
pub trait DebugInspect {
    fn debug_properties(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspect_node_serializes_to_json() {
        let node = InspectNode {
            id: "root".to_string(),
            type_name: "RootView".to_string(),
            bounds: Some(Bounds {
                x: 0.0,
                y: 0.0,
                width: 1200.0,
                height: 800.0,
            }),
            properties: HashMap::new(),
            children: vec![InspectNode {
                id: "child".to_string(),
                type_name: "Panel".to_string(),
                bounds: None,
                properties: {
                    let mut m = HashMap::new();
                    m.insert("count".to_string(), serde_json::json!(5));
                    m
                },
                children: vec![],
            }],
        };

        let json = serde_json::to_value(&node).unwrap();
        assert_eq!(json["id"], "root");
        assert_eq!(json["children"][0]["type_name"], "Panel");
        assert_eq!(json["children"][0]["properties"]["count"], 5);
        // bounds should be omitted when None
        assert!(json["children"][0].get("bounds").is_none());
    }
}
```

- [ ] **Step 2: Update lib.rs — add inspect module**

```rust
mod log_store;
mod log_layer;
mod inspect;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
pub use log_layer::{LogCaptureLayer, LogHandle};
pub use inspect::{InspectNode, Bounds, Inspectable};
#[cfg(debug_assertions)]
pub use inspect::DebugInspect;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p srow-debug`
Expected: All tests pass.

- [ ] **Step 4: Commit**

```bash
git add crates/srow-debug/src/inspect.rs crates/srow-debug/src/lib.rs
git commit -m "feat(srow-debug): add Inspectable trait and InspectNode types"
```

---

## Task 4: HTTP server + router

**Files:**
- Create: `crates/srow-debug/src/server.rs`
- Create: `crates/srow-debug/src/router.rs`

- [ ] **Step 1: Write server.rs — tiny_http wrapper**

```rust
use std::io::Read;
use std::sync::Arc;
use tiny_http::{Header, Response, Server, StatusCode};

pub(crate) struct HttpServer {
    server: Server,
}

impl HttpServer {
    pub fn new(port: u16) -> Result<Self, std::io::Error> {
        let addr = format!("127.0.0.1:{}", port);
        let server = Server::http(&addr)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::AddrInUse, e.to_string()))?;
        Ok(Self { server })
    }

    pub fn incoming_requests(&self) -> tiny_http::IncomingRequests<'_> {
        self.server.incoming_requests()
    }
}

pub(crate) fn json_response(status: u16, body: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let data = body.as_bytes().to_vec();
    let len = data.len();
    let cursor = std::io::Cursor::new(data);
    Response::new(
        StatusCode(status),
        vec![
            Header::from_bytes(&b"Content-Type"[..], &b"application/json"[..]).unwrap(),
        ],
        cursor,
        Some(len),
        None,
    )
}

pub(crate) fn error_response(status: u16, message: &str) -> Response<std::io::Cursor<Vec<u8>>> {
    let body = serde_json::json!({"error": message}).to_string();
    json_response(status, &body)
}

pub(crate) fn read_body(request: &mut tiny_http::Request) -> Result<String, std::io::Error> {
    let mut body = String::new();
    request.as_reader().read_to_string(&mut body)?;
    Ok(body)
}

pub(crate) fn parse_query_param(url: &str, key: &str) -> Option<String> {
    let query = url.split('?').nth(1)?;
    for pair in query.split('&') {
        let mut parts = pair.splitn(2, '=');
        if let (Some(k), Some(v)) = (parts.next(), parts.next()) {
            if k == key {
                return Some(v.to_string());
            }
        }
    }
    None
}
```

- [ ] **Step 2: Write router.rs — endpoint dispatch**

```rust
use std::sync::Arc;
use std::time::Instant;

use crate::log_layer::LogHandle;
use crate::log_store::LogQuery;
use crate::inspect::Inspectable;
use crate::server::{error_response, json_response, parse_query_param, read_body};

pub(crate) struct Router {
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
    start_time: Instant,
}

impl Router {
    pub fn new(
        log_handle: Option<LogHandle>,
        inspector: Option<Arc<dyn Inspectable>>,
    ) -> Self {
        Self {
            log_handle,
            inspector,
            start_time: Instant::now(),
        }
    }

    pub fn handle(&self, request: &mut tiny_http::Request) {
        let url = request.url().to_string();
        let path = url.split('?').next().unwrap_or(&url);
        let method = request.method().as_str();

        let response = match (method, path) {
            ("GET", "/api/health") => self.handle_health(),
            ("GET", "/api/logs") => self.handle_get_logs(&url),
            ("GET", "/api/logs/level") => self.handle_get_log_level(),
            ("PUT", "/api/logs/level") => self.handle_set_log_level(request),
            ("GET", "/api/inspect/tree") => self.handle_inspect_tree(),
            _ => error_response(404, &format!("unknown endpoint: {} {}", method, path)),
        };

        let _ = request.respond(response);
    }

    fn handle_health(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let uptime = self.start_time.elapsed().as_secs();
        let body = serde_json::json!({"status": "ok", "uptime_secs": uptime}).to_string();
        json_response(200, &body)
    }

    fn handle_get_logs(&self, url: &str) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };

        let query = LogQuery {
            level: parse_query_param(url, "level"),
            module: parse_query_param(url, "module"),
            since: parse_query_param(url, "since").and_then(|s| s.parse().ok()),
            cursor: parse_query_param(url, "cursor").and_then(|s| s.parse().ok()),
            keyword: parse_query_param(url, "keyword"),
            limit: parse_query_param(url, "limit").and_then(|s| s.parse().ok()),
        };

        let result = handle.query(&query);
        let body = serde_json::to_string(&result).unwrap();
        json_response(200, &body)
    }

    fn handle_get_log_level(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };
        let filter = handle.current_filter();
        let body = serde_json::json!({"filter": filter}).to_string();
        json_response(200, &body)
    }

    fn handle_set_log_level(
        &self,
        request: &mut tiny_http::Request,
    ) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref handle) = self.log_handle else {
            return error_response(503, "log system not registered");
        };

        let body = match read_body(request) {
            Ok(b) => b,
            Err(_) => return error_response(400, "failed to read request body"),
        };

        let parsed: serde_json::Value = match serde_json::from_str(&body) {
            Ok(v) => v,
            Err(_) => return error_response(400, "malformed JSON body"),
        };

        let Some(filter) = parsed.get("filter").and_then(|v| v.as_str()) else {
            return error_response(400, "missing 'filter' field in JSON body");
        };

        handle.set_filter(filter);
        let body = serde_json::json!({"ok": true, "filter": filter}).to_string();
        json_response(200, &body)
    }

    fn handle_inspect_tree(&self) -> tiny_http::Response<std::io::Cursor<Vec<u8>>> {
        let Some(ref inspector) = self.inspector else {
            return error_response(503, "inspector not registered");
        };
        let tree = inspector.inspect();
        let body = serde_json::to_string(&tree).unwrap();
        json_response(200, &body)
    }
}
```

- [ ] **Step 3: Update lib.rs — add server and router modules**

```rust
mod log_store;
mod log_layer;
mod inspect;
mod server;
mod router;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
pub use log_layer::{LogCaptureLayer, LogHandle};
pub use inspect::{InspectNode, Bounds, Inspectable};
#[cfg(debug_assertions)]
pub use inspect::DebugInspect;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p srow-debug`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/srow-debug/src/server.rs crates/srow-debug/src/router.rs crates/srow-debug/src/lib.rs
git commit -m "feat(srow-debug): add HTTP server and router with all API endpoints"
```

---

## Task 5: DebugServer builder + DebugServerHandle

**Files:**
- Create: `crates/srow-debug/src/builder.rs`

- [ ] **Step 1: Write builder.rs**

```rust
use std::sync::mpsc;
use std::sync::Arc;
use std::thread;

use crate::inspect::Inspectable;
use crate::log_layer::LogHandle;
use crate::router::Router;
use crate::server::HttpServer;

pub struct DebugServer {
    server: HttpServer,  // bound in build(), passed to start()
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
}

pub struct DebugServerBuilder {
    port: u16,
    log_handle: Option<LogHandle>,
    inspector: Option<Arc<dyn Inspectable>>,
}

pub struct DebugServerHandle {
    shutdown_tx: Option<mpsc::Sender<()>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl DebugServer {
    pub fn builder() -> DebugServerBuilder {
        DebugServerBuilder {
            port: 9229,
            log_handle: None,
            inspector: None,
        }
    }
}

impl DebugServerBuilder {
    pub fn port(mut self, port: u16) -> Self {
        self.port = port;
        self
    }

    pub fn with_log_handle(mut self, handle: LogHandle) -> Self {
        self.log_handle = Some(handle);
        self
    }

    pub fn with_inspector(mut self, inspector: impl Inspectable + 'static) -> Self {
        self.inspector = Some(Arc::new(inspector));
        self
    }

    pub fn build(self) -> Result<DebugServer, std::io::Error> {
        // Bind the port NOW so build() fails if port is in use
        let server = HttpServer::new(self.port)?;
        Ok(DebugServer {
            server,
            log_handle: self.log_handle,
            inspector: self.inspector,
        })
    }
}

impl DebugServer {
    pub fn start(self) -> DebugServerHandle {
        let (shutdown_tx, _shutdown_rx) = mpsc::channel::<()>();
        let log_handle = self.log_handle;
        let inspector = self.inspector;
        let server = Arc::new(self.server.into_inner());
        let server_clone = Arc::clone(&server);

        let join_handle = thread::spawn(move || {
            tracing::info!("Debug server started");
            let router = Router::new(log_handle, inspector);

            for mut request in server.incoming_requests() {
                router.handle(&mut request);
            }
            tracing::info!("Debug server shut down");
        });

        DebugServerHandle {
            shutdown_tx: Some(shutdown_tx),
            // Store the server Arc so we can call unblock() on shutdown
            server: Some(server_clone),
            join_handle: Some(join_handle),
        }
    }
}
```

Update `DebugServerHandle` to use `Server::unblock()` for graceful shutdown (tiny_http supports this):

```rust
pub struct DebugServerHandle {
    shutdown_tx: Option<mpsc::Sender<()>>,
    server: Option<Arc<tiny_http::Server>>,
    join_handle: Option<thread::JoinHandle<()>>,
}

impl DebugServerHandle {
    pub fn shutdown(&mut self) {
        // Unblock the server's blocking accept loop
        if let Some(server) = self.server.take() {
            server.unblock();
        }
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DebugServerHandle {
    fn drop(&mut self) {
        self.shutdown();
    }
}
```

`HttpServer` needs an `into_inner()` method to expose the underlying `tiny_http::Server`:

```rust
// In server.rs, add:
impl HttpServer {
    pub(crate) fn into_inner(self) -> tiny_http::Server {
        self.server
    }
}
```

> **Key**: `tiny_http::Server::unblock()` causes `incoming_requests()` to return `None`, cleanly breaking the for loop. No more hanging on shutdown.

- [ ] **Step 2: Update lib.rs — add builder module**

```rust
mod log_store;
mod log_layer;
mod inspect;
mod server;
mod router;
mod builder;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
pub use log_layer::{LogCaptureLayer, LogHandle};
pub use inspect::{InspectNode, Bounds, Inspectable};
#[cfg(debug_assertions)]
pub use inspect::DebugInspect;
pub use builder::{DebugServer, DebugServerBuilder, DebugServerHandle};
```

- [ ] **Step 3: Verify full crate compiles**

Run: `cargo check -p srow-debug`
Expected: Compiles with no errors.

- [ ] **Step 4: Run all tests**

Run: `cargo test -p srow-debug`
Expected: All existing tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/srow-debug/src/builder.rs crates/srow-debug/src/lib.rs
git commit -m "feat(srow-debug): add DebugServer builder and lifecycle handle"
```

---

## Task 6: Integration test — full HTTP server round-trip

**Files:**
- Create: `crates/srow-debug/tests/integration.rs`

- [ ] **Step 1: Write integration test**

```rust
use srow_debug::{DebugServer, LogCaptureLayer};
use tracing_subscriber::prelude::*;
use std::io::Read;

#[test]
fn health_endpoint() {
    let server = DebugServer::builder()
        .port(19230) // unique test port
        .build()
        .unwrap();
    let mut handle = server.start();

    // Give server a moment to bind
    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19230", "/api/health");
    assert!(resp.contains("ok"));

    handle.shutdown();
}

#[test]
fn log_query_and_level_control() {
    let (layer, log_handle) = LogCaptureLayer::new(1000);

    let _guard = tracing_subscriber::registry()
        .with(layer)
        .set_default();

    let server = DebugServer::builder()
        .port(19231)
        .with_log_handle(log_handle)
        .build()
        .unwrap();
    let mut handle = server.start();

    std::thread::sleep(std::time::Duration::from_millis(100));

    // Log some events
    tracing::info!(target: "test_mod", "hello");
    tracing::warn!(target: "test_mod", "warning");

    // Query logs
    let resp = http_get("127.0.0.1:19231", "/api/logs");
    assert!(resp.contains("hello"));
    assert!(resp.contains("warning"));

    // Query with level filter
    let resp = http_get("127.0.0.1:19231", "/api/logs?level=warn");
    assert!(!resp.contains("hello"));
    assert!(resp.contains("warning"));

    // Get current level
    let resp = http_get("127.0.0.1:19231", "/api/logs/level");
    assert!(resp.contains("trace"));

    // Set new level
    let resp = http_put(
        "127.0.0.1:19231",
        "/api/logs/level",
        r#"{"filter": "warn"}"#,
    );
    assert!(resp.contains("ok"));

    // Verify level changed
    let resp = http_get("127.0.0.1:19231", "/api/logs/level");
    assert!(resp.contains("warn"));

    handle.shutdown();
}

#[test]
fn inspect_tree_without_inspector() {
    let server = DebugServer::builder()
        .port(19232)
        .build()
        .unwrap();
    let mut handle = server.start();

    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19232", "/api/inspect/tree");
    assert!(resp.contains("error"));
    assert!(resp.contains("not registered"));

    handle.shutdown();
}

#[test]
fn unknown_endpoint_returns_404() {
    let server = DebugServer::builder()
        .port(19233)
        .build()
        .unwrap();
    let mut handle = server.start();

    std::thread::sleep(std::time::Duration::from_millis(100));

    let resp = http_get("127.0.0.1:19233", "/api/nonexistent");
    assert!(resp.contains("error"));
    assert!(resp.contains("unknown endpoint"));

    handle.shutdown();
}

// Simple HTTP helpers using raw std::net (zero extra dependencies)
fn http_get(addr: &str, path: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    use std::io::Write;
    write!(stream, "GET {} HTTP/1.1\r\nHost: {}\r\nConnection: close\r\n\r\n", path, addr).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}

fn http_put(addr: &str, path: &str, body: &str) -> String {
    let mut stream = std::net::TcpStream::connect(addr).unwrap();
    use std::io::Write;
    write!(
        stream,
        "PUT {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        path, addr, body.len(), body
    ).unwrap();
    let mut response = String::new();
    stream.read_to_string(&mut response).unwrap();
    response.split("\r\n\r\n").nth(1).unwrap_or("").to_string()
}
```


- [ ] **Step 2: Run integration tests**

Run: `cargo test -p srow-debug --test integration`
Expected: All 4 tests pass.

- [ ] **Step 3: Commit**

```bash
git add crates/srow-debug/tests/integration.rs
git commit -m "test(srow-debug): add integration tests for HTTP API endpoints"
```

---

## Task 7: GPUI Inspector (feature = "gpui")

**Files:**
- Create: `crates/srow-debug/src/gpui/mod.rs`

- [ ] **Step 1: Spike — verify GPUI view tree traversal API**

Before writing the implementation, check what GPUI `0.2` exposes:

Run: `grep -r "fn root_view\|fn children\|fn layout\|fn bounds\|fn content_bounds\|fn type_name" $(find ~/.cargo/registry/src -path "*/gpui-0.2*/src" -type d 2>/dev/null | head -1) || echo "Check gpui source manually"`

Determine whether GPUI's `Window` exposes `root_view()` and whether views expose child enumeration. If not, use the **explicit registration** fallback from the spec.

- [ ] **Step 2: Write GpuiInspector with explicit registration pattern**

The explicit registration approach works regardless of GPUI API limitations:

```rust
use std::sync::Arc;
use parking_lot::RwLock;

use crate::inspect::{InspectNode, Inspectable};

/// Registered view entry — views register themselves during construction.
pub struct ViewEntry {
    pub id: String,
    pub type_name: String,
    pub parent_id: Option<String>,
    pub snapshot_fn: Box<dyn Fn() -> InspectNode + Send + Sync>,
}

/// Thread-safe registry where GPUI views register themselves.
/// The GpuiInspector reads from this registry to build the tree.
pub struct ViewRegistry {
    entries: RwLock<Vec<ViewEntry>>,
}

impl ViewRegistry {
    pub fn new() -> Arc<Self> {
        Arc::new(Self {
            entries: RwLock::new(Vec::new()),
        })
    }

    pub fn register(&self, entry: ViewEntry) {
        self.entries.write().push(entry);
    }

    pub fn unregister(&self, id: &str) {
        self.entries.write().retain(|e| e.id != id);
    }

    fn build_tree(&self) -> InspectNode {
        let entries = self.entries.read();

        // Collect snapshots
        let snapshots: Vec<(Option<String>, InspectNode)> = entries
            .iter()
            .map(|e| (e.parent_id.clone(), (e.snapshot_fn)()))
            .collect();

        // Build tree from flat list
        build_tree_from_flat(snapshots)
    }
}

fn build_tree_from_flat(mut items: Vec<(Option<String>, InspectNode)>) -> InspectNode {
    use std::collections::HashMap;

    if items.is_empty() {
        return InspectNode {
            id: "empty".to_string(),
            type_name: "Empty".to_string(),
            bounds: None,
            properties: HashMap::new(),
            children: vec![],
        };
    }

    // Find root nodes (no parent)
    let mut children_map: HashMap<String, Vec<InspectNode>> = HashMap::new();
    let mut roots = Vec::new();

    for (parent_id, node) in items {
        match parent_id {
            Some(pid) => children_map
                .entry(pid)
                .or_default()
                .push(node),
            None => roots.push(node),
        }
    }

    // Attach children to parents (single level for now)
    fn attach_children(
        node: &mut InspectNode,
        children_map: &mut HashMap<String, Vec<InspectNode>>,
    ) {
        if let Some(children) = children_map.remove(&node.id) {
            for mut child in children {
                attach_children(&mut child, children_map);
                node.children.push(child);
            }
        }
    }

    if roots.len() == 1 {
        let mut root = roots.remove(0);
        attach_children(&mut root, &mut children_map);
        root
    } else {
        let mut root = InspectNode {
            id: "root".to_string(),
            type_name: "Root".to_string(),
            bounds: None,
            properties: HashMap::new(),
            children: roots,
        };
        for child in &mut root.children {
            attach_children(child, &mut children_map);
        }
        root
    }
}

/// GPUI Inspector — implements Inspectable using the ViewRegistry.
pub struct GpuiInspector {
    registry: Arc<ViewRegistry>,
}

impl GpuiInspector {
    pub fn new(registry: Arc<ViewRegistry>) -> Self {
        Self { registry }
    }

    pub fn registry(&self) -> &Arc<ViewRegistry> {
        &self.registry
    }
}

impl Inspectable for GpuiInspector {
    fn inspect(&self) -> InspectNode {
        self.registry.build_tree()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn registry_builds_tree() {
        let registry = ViewRegistry::new();

        registry.register(ViewEntry {
            id: "root".to_string(),
            type_name: "RootView".to_string(),
            parent_id: None,
            snapshot_fn: Box::new(|| InspectNode {
                id: "root".to_string(),
                type_name: "RootView".to_string(),
                bounds: None,
                properties: HashMap::new(),
                children: vec![],
            }),
        });

        registry.register(ViewEntry {
            id: "panel".to_string(),
            type_name: "ChatPanel".to_string(),
            parent_id: Some("root".to_string()),
            snapshot_fn: Box::new(|| InspectNode {
                id: "panel".to_string(),
                type_name: "ChatPanel".to_string(),
                bounds: None,
                properties: {
                    let mut m = HashMap::new();
                    m.insert("msg_count".to_string(), serde_json::json!(5));
                    m
                },
                children: vec![],
            }),
        });

        let inspector = GpuiInspector::new(registry);
        let tree = inspector.inspect();

        assert_eq!(tree.id, "root");
        assert_eq!(tree.children.len(), 1);
        assert_eq!(tree.children[0].type_name, "ChatPanel");
        assert_eq!(tree.children[0].properties["msg_count"], 5);
    }

    #[test]
    fn unregister_removes_view() {
        let registry = ViewRegistry::new();
        registry.register(ViewEntry {
            id: "temp".to_string(),
            type_name: "TempView".to_string(),
            parent_id: None,
            snapshot_fn: Box::new(|| InspectNode {
                id: "temp".to_string(),
                type_name: "TempView".to_string(),
                bounds: None,
                properties: HashMap::new(),
                children: vec![],
            }),
        });

        let inspector = GpuiInspector::new(registry.clone());
        assert_eq!(inspector.inspect().type_name, "TempView");

        registry.unregister("temp");
        assert_eq!(inspector.inspect().type_name, "Empty");
    }
}
```

> **Design note**: This uses the explicit registration fallback from the spec. Views call `registry.register()` during construction and `registry.unregister()` on drop. This works regardless of GPUI's internal API exposure and is actually more flexible — non-GPUI views can also register.

- [ ] **Step 3: Update lib.rs — add gpui module**

```rust
mod log_store;
mod log_layer;
mod inspect;
mod server;
mod router;
mod builder;

#[cfg(feature = "gpui")]
pub mod gpui;

pub use log_store::{LogRecord, LogQuery, LogQueryResponse};
pub use log_layer::{LogCaptureLayer, LogHandle};
pub use inspect::{InspectNode, Bounds, Inspectable};
#[cfg(debug_assertions)]
pub use inspect::DebugInspect;
pub use builder::{DebugServer, DebugServerBuilder, DebugServerHandle};
```

- [ ] **Step 4: Run tests**

Run: `cargo test -p srow-debug`
Expected: All tests pass.

- [ ] **Step 5: Commit**

```bash
git add crates/srow-debug/src/gpui/ crates/srow-debug/src/lib.rs
git commit -m "feat(srow-debug): add GPUI inspector with explicit view registration"
```

---

## Task 8: Integrate into srow-app — tracing migration + debug server

**Files:**
- Modify: `crates/srow-app/Cargo.toml`
- Modify: `crates/srow-app/src/main.rs`
- Modify: `crates/srow-core/src/bin/cli.rs`

- [ ] **Step 1: Add srow-debug dependency to srow-app**

In `crates/srow-app/Cargo.toml`, add:

```toml
srow-debug = { path = "../srow-debug", features = ["gpui"] }
```

- [ ] **Step 2: Migrate tracing init in srow-app/src/main.rs**

Replace the current tracing init (lines 19-24):

```rust
// BEFORE:
tracing_subscriber::fmt()
    .with_env_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    )
    .init();
```

With:

```rust
// AFTER:
use tracing_subscriber::prelude::*;

let fmt_layer = tracing_subscriber::fmt::layer()
    .with_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    );

#[cfg(debug_assertions)]
let _debug_handle = {
    let (log_layer, log_handle) = srow_debug::LogCaptureLayer::new(10_000);

    tracing_subscriber::registry()
        .with(log_layer)
        .with(fmt_layer)
        .init();

    let port: u16 = std::env::var("SROW_DEBUG_PORT")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(9229);

    let view_registry = srow_debug::gpui::ViewRegistry::new();
    let inspector = srow_debug::gpui::GpuiInspector::new(view_registry.clone());

    let server = srow_debug::DebugServer::builder()
        .port(port)
        .with_log_handle(log_handle)
        .with_inspector(inspector)
        .build()
        .expect("debug server failed to start");

    server.start()
};

#[cfg(not(debug_assertions))]
{
    tracing_subscriber::registry()
        .with(fmt_layer)
        .init();
}
```

> **Note**: The `view_registry` should be stored somewhere accessible to views (e.g., as a GPUI global) so views can register themselves. The exact mechanism depends on how GPUI globals work in this codebase — `SharedRuntime` is the existing pattern. Follow the same pattern for `ViewRegistry`.

- [ ] **Step 3: Migrate tracing init in srow-core/src/bin/cli.rs**

Replace the current tracing init (lines 34-39) with the layered approach. CLI doesn't need the debug server or GPUI inspector, just the compatible init:

```rust
use tracing_subscriber::prelude::*;

let fmt_layer = tracing_subscriber::fmt::layer()
    .with_filter(
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
    );

tracing_subscriber::registry()
    .with(fmt_layer)
    .init();
```

- [ ] **Step 4: Verify both binaries compile**

Run: `cargo check -p srow-app && cargo check -p srow-core`
Expected: Both compile.

- [ ] **Step 5: Verify debug server starts (manual test)**

Run: `cargo run -p srow-app`
Expected: See log line "Debug server listening on http://127.0.0.1:9229" in console output.

Then in another terminal:
Run: `curl http://127.0.0.1:9229/api/health`
Expected: `{"status":"ok","uptime_secs":...}`

- [ ] **Step 6: Commit**

```bash
git add crates/srow-app/Cargo.toml crates/srow-app/src/main.rs crates/srow-core/src/bin/cli.rs
git commit -m "feat(srow-app): integrate srow-debug server with tracing migration"
```

---

## Task 9: Tracing instrumentation — Agent engine

**Files:**
- Modify: `crates/srow-core/src/agent/runtime/engine/engine.rs`

- [ ] **Step 1: Add #[instrument] to engine::run()**

At the top of `engine.rs`, ensure `use tracing::{info, warn, error, info_span, instrument};` is imported.

Add `#[instrument]` to key methods:

On `pub async fn run(...)` (line 63):
```rust
#[tracing::instrument(
    name = "agent_turn",
    skip(self, initial_message),
    fields(session_id = %session_id)
)]
pub async fn run(&mut self, session_id: &str, initial_message: LLMMessage) -> Result<(), EngineError> {
```

- [ ] **Step 2: Add spans within the run() loop**

Inside the main loop, wrap the LLM call section (around line 184) with a span. Note: check the actual field name for the model identifier in `AgentEngine` — it may be in `self.config` or `self.llm`. Use whatever field is available:

```rust
let llm_span = tracing::info_span!("llm_request");
let _llm_guard = llm_span.enter();
// ... existing LLM stream code runs inside this span
```

Add tool execution span in `execute_tools()` (line 456):
```rust
#[tracing::instrument(
    name = "tool_execution",
    skip(self, calls, ctx),
    fields(tool_count = calls.len())
)]
async fn execute_tools(...) {
```

Inside the tool loop, log each tool:
```rust
tracing::info!(tool_name = %call.name, "executing tool");
```

- [ ] **Step 3: Add error events**

At the existing `tracing::error!("LLM stream error: {}", error)` (line 308), add structured fields:

```rust
tracing::error!(error_type = "llm_stream", error = %error, "LLM stream error");
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p srow-core`
Expected: Compiles.

- [ ] **Step 5: Commit**

```bash
git add crates/srow-core/src/agent/runtime/engine/engine.rs
git commit -m "feat(srow-core): add tracing spans to agent engine loop"
```

---

## Task 10: Tracing instrumentation — Chat, MCP, ACP

**Files:**
- Modify: `crates/srow-ai/src/chat/abstract_chat.rs`
- Modify: `crates/srow-core/src/mcp/runtime.rs`
- Modify: `crates/srow-core/src/agent/agent_client/session/client.rs`

- [ ] **Step 1: Add spans to AbstractChat::send_message()**

In `abstract_chat.rs`, at `send_message()` (line 84):
```rust
#[tracing::instrument(name = "chat_send", skip(self, parts, options))]
pub async fn send_message(&self, parts: Vec<UIMessagePart>, options: SendOptions) {
```

- [ ] **Step 2: Add spans to McpManager methods**

In `mcp/runtime.rs`:

On `connect()` (line 60):
```rust
#[tracing::instrument(name = "mcp_connect", skip(self), fields(server_id = %server_id))]
pub async fn connect(&self, server_id: &str) -> Result<(), SkillError> {
```

On `call_tool()` (line 124):
```rust
#[tracing::instrument(name = "mcp_call_tool", skip(self, arguments), fields(server_id = %server_id, tool_name = %tool_name))]
pub async fn call_tool(&self, server_id: &str, tool_name: &str, arguments: serde_json::Value) -> ... {
```

- [ ] **Step 3: Add spans to AcpSession methods**

In `session/client.rs`:

On `send_prompt()` (line 66):
```rust
#[tracing::instrument(name = "acp_send_prompt", skip(self, prompt), fields(session_id = %self.session_id, resume = resume))]
pub async fn send_prompt(&self, prompt: String, resume: bool) -> ... {
```

On `handle_inbound()` (line 88):
```rust
#[tracing::instrument(name = "acp_inbound", skip(self, msg), fields(session_id = %self.session_id))]
pub async fn handle_inbound(&self, msg: AcpInboundMessage) {
```

- [ ] **Step 4: Verify all crates compile**

Run: `cargo check --workspace`
Expected: All crates compile.

- [ ] **Step 5: Commit**

```bash
git add crates/srow-ai/src/chat/abstract_chat.rs crates/srow-core/src/mcp/runtime.rs crates/srow-core/src/agent/agent_client/session/client.rs
git commit -m "feat: add tracing spans to chat, MCP, and ACP boundaries"
```

---

## Task 11: Register views with ViewRegistry

**Files:**
- Modify: `crates/srow-app/src/main.rs` (store ViewRegistry as GPUI global)
- Modify: `crates/srow-app/src/views/root_view.rs`
- Modify: `crates/srow-app/src/views/chat_panel/chat_panel.rs`
- Modify: `crates/srow-app/src/views/agent_panel/agent_panel.rs`

- [ ] **Step 1: Make ViewRegistry a GPUI global**

In `main.rs`, after creating the `view_registry`, store it as a GPUI global so views can access it:

```rust
// In srow-app, define a wrapper:
#[cfg(debug_assertions)]
pub struct DebugViewRegistry(pub std::sync::Arc<srow_debug::gpui::ViewRegistry>);
#[cfg(debug_assertions)]
impl gpui::Global for DebugViewRegistry {}

// In main(), after creating view_registry:
#[cfg(debug_assertions)]
cx.set_global(DebugViewRegistry(view_registry.clone()));
```

- [ ] **Step 2: Register RootView**

In `root_view.rs`, at the end of `new()`:

```rust
#[cfg(debug_assertions)]
{
    if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
        let side_panel = self.side_panel.clone();
        let chat_panel = self.chat_panel.clone();
        let agent_panel = self.agent_panel.clone();
        registry.0.register(srow_debug::gpui::ViewEntry {
            id: "root_view".to_string(),
            type_name: "RootView".to_string(),
            parent_id: None,
            snapshot_fn: Box::new(move || {
                srow_debug::InspectNode {
                    id: "root_view".to_string(),
                    type_name: "RootView".to_string(),
                    bounds: None,
                    properties: std::collections::HashMap::new(),
                    children: vec![],
                }
            }),
        });
    }
}
```

- [ ] **Step 3: Register ChatPanel with diagnostic properties**

In `chat_panel.rs`, register with diagnostic state. The `snapshot_fn` closure should capture references to the model/state and expose real diagnostic properties:

```rust
#[cfg(debug_assertions)]
{
    if let Some(registry) = cx.try_global::<crate::DebugViewRegistry>() {
        // Capture state references for diagnostic properties
        // Adjust these captures based on what ChatPanel actually holds
        // (e.g., message_list entity, chat model, etc.)
        registry.0.register(srow_debug::gpui::ViewEntry {
            id: "chat_panel".to_string(),
            type_name: "ChatPanel".to_string(),
            parent_id: Some("root_view".to_string()),
            snapshot_fn: Box::new(move || {
                // Expose diagnostic state — what is this component doing?
                let mut props = std::collections::HashMap::new();
                // TODO: Capture actual state from ChatPanel's entities.
                // Example of what this should look like when wired up:
                //   props.insert("message_count".into(), json!(chat.messages().len()));
                //   props.insert("status".into(), json!(format!("{:?}", chat.status())));
                //   props.insert("last_error".into(), json!(null));
                srow_debug::InspectNode {
                    id: "chat_panel".to_string(),
                    type_name: "ChatPanel".to_string(),
                    bounds: None,
                    properties: props,
                    children: vec![],
                }
            }),
        });
    }
}
```

> **Note**: The exact state captured depends on what `ChatPanel` holds and what is `Send + Sync`. GPUI `Entity` handles are not `Send`, so the snapshot_fn should read state and copy values during registration setup, or use `Arc<Mutex<>>` wrappers. The implementer should wire up real state as the code compiles. The key guideline from the spec: expose state that answers "what is this component doing right now and is anything wrong?"

- [ ] **Step 4: Register AgentPanel similarly**

Same pattern for `agent_panel.rs` with `parent_id: Some("root_view".to_string())`.

- [ ] **Step 5: Verify compilation and test inspect endpoint**

Run: `cargo check -p srow-app`

Manual test:
Run: `cargo run -p srow-app`
Then: `curl http://127.0.0.1:9229/api/inspect/tree | python3 -m json.tool`

Expected: JSON tree with root_view, chat_panel, agent_panel as children.

- [ ] **Step 6: Commit**

```bash
git add crates/srow-app/src/main.rs crates/srow-app/src/views/
git commit -m "feat(srow-app): register views with debug ViewRegistry for inspection"
```

---

## Task 12: End-to-end verification

- [ ] **Step 1: Run full test suite**

Run: `cargo test --workspace`
Expected: All tests pass.

- [ ] **Step 2: Manual end-to-end test**

Start the app:
```bash
RUST_LOG=debug cargo run -p srow-app
```

In another terminal, verify all endpoints:
```bash
# Health
curl http://127.0.0.1:9229/api/health

# Logs (should have startup logs)
curl "http://127.0.0.1:9229/api/logs?limit=5" | python3 -m json.tool

# Log level
curl http://127.0.0.1:9229/api/logs/level

# Change log level
curl -X PUT http://127.0.0.1:9229/api/logs/level -d '{"filter": "srow_core=trace"}'

# View tree
curl http://127.0.0.1:9229/api/inspect/tree | python3 -m json.tool

# Error handling
curl http://127.0.0.1:9229/api/nonexistent
```

- [ ] **Step 3: Commit final state**

```bash
git add -A
git commit -m "feat(srow-debug): complete V1 debug system with logging, inspection, and instrumentation"
```
