# srow-debug: AI-Friendly Debug & Logging System

## Overview

A standalone, reusable Rust crate (`srow-debug`) providing a local HTTP debug server with structured logging and UI view tree inspection. Designed for AI-driven dynamic debugging — all outputs are structured JSON, easy to query and parse programmatically.

The crate is framework-agnostic at its core, with optional GPUI integration via feature flag.

## Constraints

- **Debug-only**: All debug code guarded by `#[cfg(debug_assertions)]` — zero presence in release builds
- **Reusable**: Independent crate, no project-specific dependencies in core
- **Local-only**: HTTP server binds to `127.0.0.1`, not accessible externally
- **Non-invasive**: Integrates as an additional tracing Layer, existing console output unaffected

## V1 Capabilities

1. **Dynamic log level control** — adjust per-module log levels at runtime via HTTP API
2. **Log history query/filter** — ring buffer storage with structured query (by level, module, time, keyword)
3. **GPUI view tree snapshot** — structured JSON representation of the UI element hierarchy

## Architecture

### Crate Structure

```
crates/srow-debug/
├── Cargo.toml
├── src/
│   ├── lib.rs          // DebugServer builder + public API
│   ├── server.rs       // HTTP server (tiny_http)
│   ├── router.rs       // Route dispatch
│   ├── log_layer.rs    // tracing Layer: capture + dynamic level filter
│   ├── log_store.rs    // RingBuffer log storage
│   ├── inspect.rs      // Inspectable trait definition
│   └── gpui/
│       └── mod.rs      // #[cfg(feature = "gpui")] GPUI view tree implementation
```

### Dependencies

**Core (default):**
- `tiny_http` — lightweight HTTP server, minimal dependency footprint
- `tracing` + `tracing-subscriber` — Layer integration
- `serde` + `serde_json` — JSON serialization
- `parking_lot` — efficient RwLock for LogStore

**Optional (feature = "gpui"):**
- `gpui` — GPUI framework types for view tree traversal

### Cargo.toml

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

> **Note**: The `gpui` dependency format should match the workspace's existing approach (git reference or path dependency rather than crates.io version). Adjust during implementation.

### Workspace Integration

The workspace `Cargo.toml` must add `"crates/srow-debug"` to the `members` list. The consuming crate (`srow-app`) adds `srow-debug` as a dependency with `features = ["gpui"]`.

## HTTP API

All endpoints return JSON with `Content-Type: application/json` header. Server binds to `127.0.0.1:{port}` (default port: 9229, configurable via `SROW_DEBUG_PORT` env var).

### Endpoints

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/health` | Health check / service discovery |
| `GET` | `/api/logs` | Query log history with filters |
| `PUT` | `/api/logs/level` | Dynamically adjust log level filter |
| `GET` | `/api/logs/level` | Get current log level configuration |
| `GET` | `/api/inspect/tree` | Get view tree snapshot (requires Inspectable registered) |

### Error Response Format

All error responses use a consistent schema with appropriate HTTP status codes:

```json
{"error": "description of what went wrong"}
```

- `400` — invalid query parameter, malformed JSON body
- `404` — unknown endpoint
- `503` — requested capability not registered (e.g., no inspector for `/api/inspect/tree`)
- `504` — upstream timeout (e.g., UI thread did not respond to inspect request within 5 seconds)

### GET /api/logs

Query parameters:
- `level` — minimum level filter (`trace`, `debug`, `info`, `warn`, `error`)
- `module` — module path prefix match (e.g. `srow_core::agent`)
- `since` — unix timestamp in milliseconds, only return logs after this time
- `cursor` — sequence ID from previous query, for reliable pagination (monotonic, gap-free)
- `keyword` — substring match in message text (linear scan, performant at 10k buffer size)
- `limit` — max records to return (default: 100)

Response:
```json
{
  "total_matches": 3,
  "records": [
    {
      "seq": 10042,
      "timestamp": 1711100001234,
      "level": "WARN",
      "target": "srow_core::mcp::runtime",
      "message": "MCP server auto-connect failed",
      "fields": {"server": "filesystem"},
      "span_stack": ["agent_session", "mcp_init"]
    }
  ]
}
```

The `seq` field is a monotonically increasing sequence ID (in-memory only, resets on process restart). Use it as the `cursor` parameter in subsequent queries to reliably paginate without missing records that share a timestamp. `total_matches` is the total count of matching records in the buffer (may exceed `limit`).

### PUT /api/logs/level

Request body:
```json
{"filter": "srow_core::agent=trace,srow_ai=debug"}
```

Response:
```json
{"ok": true, "filter": "srow_core::agent=trace,srow_ai=debug"}
```

### GET /api/logs/level

Response:
```json
{"filter": "info"}
```

### GET /api/inspect/tree

Response:
```json
{
  "id": "root",
  "type_name": "RootView",
  "bounds": {"x": 0, "y": 0, "width": 1200, "height": 800},
  "properties": {},
  "children": [
    {
      "id": "side_panel",
      "type_name": "SidePanel",
      "bounds": {"x": 0, "y": 0, "width": 220, "height": 800},
      "properties": {"session_count": 3},
      "children": []
    }
  ]
}
```

### GET /api/health

Response:
```json
{"status": "ok", "uptime_secs": 42}
```

## Log System Design

### LogRecord

```rust
struct LogRecord {
    seq: u64,                                // monotonic sequence ID
    timestamp: i64,                          // unix millis
    level: Level,                            // TRACE/DEBUG/INFO/WARN/ERROR
    target: String,                          // module path, e.g. "srow_core::agent::engine"
    message: String,                         // formatted message
    fields: HashMap<String, serde_json::Value>,  // structured fields
    span_stack: Vec<String>,                 // current span chain
}
```

### LogCaptureLayer

Implements `tracing_subscriber::Layer<Registry>`. Dual responsibility:

1. **Capture**: intercepts all tracing events at `TRACE` level (captures everything), serializes into `LogRecord`, pushes to `LogStore`. The capture filter is independent of the console output filter — changing it via HTTP API does not affect what appears in the terminal.
2. **Dynamic filter**: holds a `parking_lot::RwLock<String>` storing the current filter directive string. When updated via `PUT /api/logs/level`, the filter takes effect immediately for subsequent captures. Default: `"trace"` (capture all). This only controls what the capture layer stores — console output remains governed by `RUST_LOG` / the fmt layer's own filter.

### LogHandle

`LogCaptureLayer::new()` returns a `(LogCaptureLayer, LogHandle)` tuple. `LogHandle` is the control interface:

```rust
struct LogHandle {
    store: Arc<RwLock<LogStore>>,
    filter: Arc<RwLock<String>>,  // current filter directive string
}

impl LogHandle {
    /// Query logs with filters
    fn query(&self, params: &LogQuery) -> Vec<LogRecord>;
    /// Update the dynamic log level filter
    fn set_filter(&self, filter_str: &str) -> Result<(), FilterError>;
    /// Get current filter as string
    fn current_filter(&self) -> String;
}
```

### LogStore

Ring buffer with fixed capacity (default: 10,000 records). Overflow overwrites oldest records.

- Thread-safe via `parking_lot::RwLock` (read-heavy workload)
- Supports query with combined filters (level, target prefix, time range, keyword, cursor)
- Returns results in chronological order
- Each record assigned a monotonic `seq` ID for cursor-based pagination

### Migration: Existing tracing Setup

**Important**: The current codebase uses `tracing_subscriber::fmt().init()` in both `srow-app/src/main.rs` and `srow-core/src/bin/cli.rs`. This is **incompatible** with the layered approach — `init()` can only be called once per process.

Migration required:

```rust
// BEFORE (current):
tracing_subscriber::fmt()
    .with_env_filter(EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info")))
    .init();

// AFTER (with srow-debug):
use tracing_subscriber::prelude::*;

let fmt_layer = tracing_subscriber::fmt::layer()
    .with_filter(EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info")));

#[cfg(debug_assertions)]
{
    let (log_layer, log_handle) = srow_debug::LogCaptureLayer::new(10_000);
    tracing_subscriber::registry()
        .with(log_layer)
        .with(fmt_layer)
        .init();
    // log_handle passed to DebugServer below...
}

#[cfg(not(debug_assertions))]
{
    tracing_subscriber::registry()
        .with(fmt_layer)
        .init();
}
```

## View Tree Inspection

### Inspectable Trait (framework-agnostic)

```rust
struct InspectNode {
    id: String,
    type_name: String,
    bounds: Option<Bounds>,
    properties: HashMap<String, serde_json::Value>,
    children: Vec<InspectNode>,
}

struct Bounds {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

/// Framework-agnostic inspection trait.
/// Implementations produce a snapshot (InspectNode tree) that can be
/// sent across threads — the trait itself does NOT hold UI state.
trait Inspectable: Send + Sync {
    fn inspect(&self) -> InspectNode;
}
```

**Thread-safety note**: `Inspectable` requires `Send + Sync` because the HTTP server runs on a background thread. Implementations must NOT hold direct references to UI-thread-only state (e.g., GPUI `WindowContext`). Instead, they use a cross-thread snapshot mechanism (see GPUI implementation below).

### DebugInspect Trait (opt-in per View)

```rust
/// Opt-in trait for views to expose custom debug properties.
/// Guarded by #[cfg(debug_assertions)] — in release builds,
/// srow-debug is not compiled, so this trait does not exist.
///
/// Usage in application views:
///   #[cfg(debug_assertions)]
///   impl srow_debug::DebugInspect for ChatPanel {
///       fn debug_properties(&self) -> HashMap<String, serde_json::Value> { ... }
///   }
#[cfg(debug_assertions)]
trait DebugInspect {
    fn debug_properties(&self) -> HashMap<String, serde_json::Value> {
        HashMap::new()
    }
}
```

Application views annotate their `impl DebugInspect` blocks with `#[cfg(debug_assertions)]` so that release builds compile cleanly without the srow-debug dependency.

### GPUI Implementation (feature = "gpui")

**Cross-thread snapshot mechanism**: Since GPUI view/layout data is only accessible from the main (UI) thread, while the HTTP server runs on a background thread, the implementation uses a request-response channel pattern with `std::sync::mpsc` (no extra dependencies):

1. HTTP handler sends an inspect request (with a one-shot reply sender) via `mpsc`
2. A callback registered on the GPUI main thread receives the request, traverses the view tree, builds an `InspectNode` snapshot
3. The snapshot (owned data, no references) is sent back via the reply channel
4. HTTP handler waits with a **5-second timeout** — if the UI thread is hung, returns HTTP 504

```rust
use std::sync::mpsc;
use std::time::Duration;

/// Registered as the Inspectable implementation for GPUI apps.
/// Holds a channel sender to request snapshots from the main thread.
struct GpuiInspector {
    /// Sends (reply_tx) to the UI thread; UI thread sends back InspectNode via reply_tx.
    request_tx: mpsc::Sender<mpsc::Sender<InspectNode>>,
}

impl Inspectable for GpuiInspector {
    fn inspect(&self) -> InspectNode {
        let (reply_tx, reply_rx) = mpsc::channel();
        self.request_tx.send(reply_tx).expect("UI thread alive");
        reply_rx.recv_timeout(Duration::from_secs(5))
            .expect("UI thread snapshot timed out")
    }
}
```

The main-thread side (registered during app init) handles the request by traversing the GPUI view tree:
- Extracts type name via `std::any::type_name`
- Extracts bounds from layout info
- Checks if View implements `DebugInspect` for additional properties
- Produces recursive `InspectNode` tree

**Risk**: GPUI (`gpui = "0.2"`) does not have a fully documented public API for programmatic view tree traversal. Implementation may require:
- Using `Window::root_view()` and traversing child views if the API supports it
- Alternatively, manually registering views in the tree during construction (explicit registration pattern)

This should be validated with a spike during implementation. If GPUI does not expose sufficient internals, the fallback is explicit registration: each View registers itself with the inspector during construction, and the inspector builds the tree from the registered entries.

## Initialization

### DebugServer Builder

```rust
struct DebugServer { /* ... */ }

impl DebugServer {
    fn builder() -> DebugServerBuilder;
}

struct DebugServerBuilder {
    port: u16,
    log_handle: Option<LogHandle>,
    inspector: Option<Box<dyn Inspectable>>,
}

impl DebugServerBuilder {
    /// Set the HTTP server port. Default: 9229.
    fn port(mut self, port: u16) -> Self;

    /// Register the log handle for log query and level control endpoints.
    fn with_log_handle(mut self, handle: LogHandle) -> Self;

    /// Register an Inspectable implementation for view tree endpoints.
    fn with_inspector(mut self, inspector: impl Inspectable + 'static) -> Self;

    /// Build the server. Returns error if port is in use.
    fn build(self) -> Result<DebugServer, DebugServerError>;
}

impl DebugServer {
    /// Start the HTTP server on a background thread. Non-blocking.
    /// Returns a JoinHandle for optional graceful shutdown.
    fn start(self) -> DebugServerHandle;
}
```

### DebugServerHandle (Lifecycle)

```rust
struct DebugServerHandle {
    shutdown_tx: Option<mpsc::Sender<()>>,   // Option for take() in Drop
    join_handle: Option<std::thread::JoinHandle<()>>,
}

impl DebugServerHandle {
    /// Explicitly shut down the server: send signal, then join the thread.
    fn shutdown(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        if let Some(handle) = self.join_handle.take() {
            let _ = handle.join();
        }
    }
}

impl Drop for DebugServerHandle {
    /// Safety net: if not explicitly shut down, signals and joins on drop.
    fn drop(&mut self) {
        self.shutdown();
    }
}
```

The `DebugServerHandle` is stored in the application (e.g., as a GPUI global or local variable). When the application exits and the handle is dropped, the server shuts down cleanly. `shutdown()` takes `&mut self` and uses `Option::take()` so it is safe to call explicitly and also safe when `Drop` runs afterwards.

### Full Initialization Example

```rust
// In srow-app/src/main.rs:
use tracing_subscriber::prelude::*;

let fmt_layer = tracing_subscriber::fmt::layer()
    .with_filter(EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info")));

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

    let inspector = srow_debug::gpui::GpuiInspector::new(/* channel setup */);

    let server = srow_debug::DebugServer::builder()
        .port(port)
        .with_log_handle(log_handle)
        .with_inspector(inspector)
        .build()
        .expect("debug server failed to start");

    server.start()  // returns DebugServerHandle
};

#[cfg(not(debug_assertions))]
{
    tracing_subscriber::registry()
        .with(fmt_layer)
        .init();
}
```

## Tracing Instrumentation Strategy

The debug server is only as useful as the tracing data flowing through it. V1 must include baseline instrumentation at key boundaries so AI can trace an operation end-to-end and pinpoint where it breaks.

### Required Spans (key boundaries)

Each span wraps a logical unit of work. When something fails, the `span_stack` in log records reveals exactly which stage broke.

**Agent engine loop** (`srow-core::agent::runtime::engine`):
- `agent_turn` span per iteration — covers prompt → LLM call → tool execution → result
- `llm_request` span — LLM API call with model name, token counts
- `tool_execution` span — tool name, input summary, success/failure

**Chat message flow** (`srow-ai::chat`):
- `chat_send` span — user message submission
- `chat_stream` span — streaming response, with chunk count on completion
- `chat_error` event — any error during the flow

**MCP/ACP protocol** (`srow-core::mcp`, `srow-core::agent::agent_client`):
- `mcp_request` span — server name, method, success/failure
- `acp_message` span — message type, direction (send/receive)

**GPUI event handling** (`srow-app`):
- `action_dispatch` span on key user actions (send message, switch session, etc.)

### Instrumentation guidelines

- Use `#[tracing::instrument]` for functions at these boundaries — minimal code change
- Include only the fields AI needs to correlate events (IDs, names, status), not raw payloads
- Error events (`tracing::error!`) must include the error type and message at minimum
- Do NOT instrument hot loops or per-frame rendering — only logical operations

### DebugInspect: Expose Diagnostic State, Not Just Counts

`DebugInspect::debug_properties()` implementations should expose state that helps diagnose issues, not just structural metadata:

```rust
// ❌ Too shallow — tells you nothing useful
fn debug_properties(&self) -> HashMap<String, Value> {
    json!({"message_count": 12}).as_object().unwrap().clone().into_iter().collect()
}

// ✅ Diagnostic — AI can see what's actually happening
fn debug_properties(&self) -> HashMap<String, Value> {
    json!({
        "message_count": 12,
        "status": "streaming",
        "last_error": null,
        "pending_tool_calls": ["bash", "read_file"],
        "current_model": "claude-sonnet-4-6"
    }).as_object().unwrap().clone().into_iter().collect()
}
```

The guideline: include state that answers "what is this component doing right now and is anything wrong?"

## Out of Scope (V1)

- Model/Entity state dump as dedicated endpoints (covered partially by DebugInspect properties)
- Performance metrics / profiling
- Runtime action triggers (force reconnect, reset state)
- Remote access / authentication
