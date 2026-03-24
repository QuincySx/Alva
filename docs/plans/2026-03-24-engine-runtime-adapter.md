# Engine Runtime Adapter Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement `alva-engine-runtime` (EngineRuntime trait + types) and `alva-engine-adapter-claude` (Claude Agent SDK bridge adapter) as two new workspace crates.

**Architecture:** Two crates — `alva-engine-runtime` defines the abstract interface (`EngineRuntime` trait, `RuntimeEvent`, `RuntimeRequest`), `alva-engine-adapter-claude` implements it by spawning a Node.js bridge process that communicates with the Claude Agent SDK via JSON-line stdin/stdout protocol.

**Tech Stack:** Rust, tokio (async process/io/channels), serde_json (JSON-line protocol), async-trait, futures-core (Stream), Node.js bridge script (embedded via `include_str!`)

**Spec:** `docs/superpowers/specs/2026-03-24-engine-runtime-adapter-design.md`

---

### Task 1: Scaffold alva-engine-runtime crate

**Files:**
- Create: `crates/alva-engine-runtime/Cargo.toml`
- Create: `crates/alva-engine-runtime/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-engine-runtime"
version = "0.1.0"
edition = "2021"

[dependencies]
alva-types = { path = "../alva-types" }
async-trait = "0.1"
futures-core = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
```

**Step 2: Create empty lib.rs**

```rust
pub mod error;
pub mod event;
pub mod request;
pub mod runtime;

pub use error::RuntimeError;
pub use event::{RuntimeEvent, RuntimeUsage, RuntimeCapabilities};
pub use request::{RuntimeRequest, RuntimeOptions};
pub use runtime::EngineRuntime;
```

**Step 3: Create placeholder modules**

Create `src/error.rs`, `src/event.rs`, `src/request.rs`, `src/runtime.rs` — each as empty files for now (they'll be filled in subsequent tasks).

**Step 4: Add to workspace**

Add `"crates/alva-engine-runtime"` to the `members` array in the root `Cargo.toml`.

**Step 5: Verify it compiles**

Run: `cargo check -p alva-engine-runtime`
Expected: Compilation succeeds (with warnings about unused imports, which is fine at this stage).

**Step 6: Commit**

```bash
git add crates/alva-engine-runtime/ Cargo.toml
git commit -m "feat(alva-engine-runtime): scaffold crate with empty modules"
```

---

### Task 2: Implement RuntimeError

**Files:**
- Create: `crates/alva-engine-runtime/src/error.rs`

**Step 1: Write the error enum**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Engine not ready: {0}")]
    NotReady(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Permission request not found: {0}")]
    PermissionNotFound(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for RuntimeError {
    fn from(e: std::io::Error) -> Self {
        RuntimeError::ProcessError(e.to_string())
    }
}

impl From<serde_json::Error> for RuntimeError {
    fn from(e: serde_json::Error) -> Self {
        RuntimeError::ProtocolError(e.to_string())
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-engine-runtime`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-runtime/src/error.rs
git commit -m "feat(alva-engine-runtime): add RuntimeError with From impls"
```

---

### Task 3: Implement RuntimeEvent and related types

**Files:**
- Create: `crates/alva-engine-runtime/src/event.rs`

**Step 1: Write event types**

```rust
use alva_types::{ContentBlock, MessageRole, StreamEvent, ToolResult};
use serde::{Deserialize, Serialize};

/// Unified event type emitted by all engine adapters.
///
/// **Termination semantics:** `Completed` is the only terminal event.
/// On errors, adapters emit `Error { recoverable: false }` followed by
/// `Completed { result: None }`. Consumers should wait for `Completed`
/// to finalize cleanup.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum RuntimeEvent {
    /// Session has started.
    SessionStarted {
        session_id: String,
        model: Option<String>,
        tools: Vec<String>,
    },

    /// Complete assistant message.
    ///
    /// `content` does NOT contain ToolUse/ToolResult blocks — those are
    /// extracted into separate ToolStart/ToolEnd events.
    Message {
        id: String,
        role: MessageRole,
        content: Vec<ContentBlock>,
    },

    /// Streaming delta (reuses alva_types::StreamEvent).
    MessageDelta {
        id: String,
        delta: StreamEvent,
    },

    /// Tool call started.
    ToolStart {
        id: String,
        name: String,
        input: serde_json::Value,
    },

    /// Tool call ended.
    ///
    /// Adapters must maintain a `HashMap<tool_use_id, tool_name>` to
    /// populate `name` since SDK tool_result only carries tool_use_id.
    ToolEnd {
        id: String,
        name: String,
        result: ToolResult,
        duration_ms: Option<u64>,
    },

    /// Permission approval required from the user.
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: serde_json::Value,
        description: Option<String>,
    },

    /// Session completed (always the final event).
    Completed {
        session_id: String,
        result: Option<String>,
        usage: Option<RuntimeUsage>,
    },

    /// Error during execution.
    Error {
        message: String,
        recoverable: bool,
    },
}

/// Engine-level usage statistics.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RuntimeUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub num_turns: u32,
}

/// Declares what an engine supports.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeCapabilities {
    pub streaming: bool,
    pub tool_control: bool,
    pub permission_callback: bool,
    pub resume: bool,
    pub cancel: bool,
}

/// Permission decision sent back to the engine.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PermissionDecision {
    Allow {
        updated_input: Option<serde_json::Value>,
    },
    Deny {
        message: String,
    },
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-engine-runtime`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-runtime/src/event.rs
git commit -m "feat(alva-engine-runtime): add RuntimeEvent, RuntimeUsage, RuntimeCapabilities, PermissionDecision"
```

---

### Task 4: Implement RuntimeRequest

**Files:**
- Create: `crates/alva-engine-runtime/src/request.rs`

**Step 1: Write request types**

```rust
use std::collections::HashMap;
use std::path::PathBuf;

/// Request to execute an agent session.
#[derive(Debug, Clone)]
pub struct RuntimeRequest {
    /// User prompt.
    pub prompt: String,

    /// Resume an existing session (pass session_id).
    pub resume_session: Option<String>,

    /// Custom system prompt.
    pub system_prompt: Option<String>,

    /// Working directory for the agent.
    pub working_directory: Option<PathBuf>,

    /// Runtime options.
    pub options: RuntimeOptions,
}

/// Engine-agnostic runtime options.
#[derive(Debug, Clone, Default)]
pub struct RuntimeOptions {
    /// Enable streaming deltas.
    pub streaming: bool,

    /// Maximum agentic turns.
    pub max_turns: Option<u32>,

    /// Engine-specific pass-through configuration.
    pub extra: HashMap<String, serde_json::Value>,
}

impl RuntimeRequest {
    /// Create a simple request with just a prompt.
    pub fn new(prompt: impl Into<String>) -> Self {
        Self {
            prompt: prompt.into(),
            resume_session: None,
            system_prompt: None,
            working_directory: None,
            options: RuntimeOptions::default(),
        }
    }

    /// Set the working directory.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.working_directory = Some(cwd.into());
        self
    }

    /// Enable streaming.
    pub fn with_streaming(mut self) -> Self {
        self.options.streaming = true;
        self
    }
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-engine-runtime`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-runtime/src/request.rs
git commit -m "feat(alva-engine-runtime): add RuntimeRequest with builder methods"
```

---

### Task 5: Implement EngineRuntime trait

**Files:**
- Create: `crates/alva-engine-runtime/src/runtime.rs`

**Step 1: Write the trait**

```rust
use std::pin::Pin;

use async_trait::async_trait;
use futures_core::Stream;

use crate::error::RuntimeError;
use crate::event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent};
use crate::request::RuntimeRequest;

/// Unified agent engine runtime interface.
///
/// All engine adapters implement this trait. Consumers depend only on
/// this interface and remain agnostic to the underlying engine.
#[async_trait]
pub trait EngineRuntime: Send + Sync {
    /// Execute an agent session and return an event stream.
    ///
    /// Returns `Err` if the engine fails to start (e.g., process spawn failure).
    /// Runtime errors during execution are emitted as `RuntimeEvent::Error`
    /// in the stream, always followed by a terminal `RuntimeEvent::Completed`.
    ///
    /// The returned Stream is `'static` and does not borrow from `&self`.
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError>;

    /// Cancel a running session.
    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError>;

    /// Respond to a permission request from the engine.
    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError>;

    /// Query engine capabilities.
    fn capabilities(&self) -> RuntimeCapabilities;
}
```

**Step 2: Fix lib.rs exports**

Update `lib.rs` to also export `PermissionDecision`:

```rust
pub mod error;
pub mod event;
pub mod request;
pub mod runtime;

pub use error::RuntimeError;
pub use event::{PermissionDecision, RuntimeCapabilities, RuntimeEvent, RuntimeUsage};
pub use request::{RuntimeOptions, RuntimeRequest};
pub use runtime::EngineRuntime;
```

**Step 3: Verify it compiles**

Run: `cargo check -p alva-engine-runtime`
Expected: PASS — all types resolve, trait is object-safe.

**Step 4: Verify object safety**

Add a quick compile-time check at the bottom of `runtime.rs`:

```rust
// Compile-time object-safety check.
#[allow(dead_code)]
fn _assert_object_safe(_: &dyn EngineRuntime) {}
```

Run: `cargo check -p alva-engine-runtime`
Expected: PASS — if this compiles, the trait is object-safe.

**Step 5: Commit**

```bash
git add crates/alva-engine-runtime/
git commit -m "feat(alva-engine-runtime): add EngineRuntime trait (object-safe, async)"
```

---

### Task 6: Scaffold alva-engine-adapter-claude crate

**Files:**
- Create: `crates/alva-engine-adapter-claude/Cargo.toml`
- Create: `crates/alva-engine-adapter-claude/src/lib.rs`
- Create: `crates/alva-engine-adapter-claude/bridge/index.mjs` (placeholder)
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-engine-adapter-claude"
version = "0.1.0"
edition = "2021"

[dependencies]
alva-engine-runtime = { path = "../alva-engine-runtime" }
alva-types = { path = "../alva-types" }
async-trait = "0.1"
futures-core = "0.3"
tokio = { version = "1", features = ["process", "io-util", "sync", "rt", "macros", "time"] }
tokio-stream = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }
dirs = "5"
```

**Step 2: Create lib.rs**

```rust
mod bridge;
mod config;
mod mapping;
mod process;
mod protocol;

mod adapter;

pub use adapter::ClaudeAdapter;
pub use config::{ClaudeAdapterConfig, PermissionMode};
```

**Step 3: Create placeholder modules**

Create empty files: `src/adapter.rs`, `src/bridge.rs`, `src/config.rs`, `src/mapping.rs`, `src/process.rs`, `src/protocol.rs`.

**Step 4: Create placeholder bridge script**

Create `bridge/index.mjs` with a comment:

```javascript
// Placeholder — will be implemented in Task 11.
```

**Step 5: Add to workspace**

Add `"crates/alva-engine-adapter-claude"` to the root `Cargo.toml` workspace members.

**Step 6: Verify it compiles**

Run: `cargo check -p alva-engine-adapter-claude`
Expected: PASS (with warnings about empty modules).

**Step 7: Commit**

```bash
git add crates/alva-engine-adapter-claude/ Cargo.toml
git commit -m "feat(alva-engine-adapter-claude): scaffold crate with empty modules"
```

---

### Task 7: Implement ClaudeAdapterConfig

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/config.rs`

**Step 1: Write config types**

```rust
use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Configuration for the Claude Agent SDK bridge adapter.
#[derive(Debug, Clone, Default)]
pub struct ClaudeAdapterConfig {
    /// Node.js executable path (default: "node").
    pub node_path: Option<String>,

    /// Path to @anthropic-ai/claude-agent-sdk package.
    /// Default: resolved from npm global or project node_modules.
    pub sdk_package_path: Option<String>,

    /// API key. Falls back to ANTHROPIC_API_KEY env var if unset.
    pub api_key: Option<String>,

    /// Model name (e.g., "claude-sonnet-4-6").
    pub model: Option<String>,

    /// Permission mode for tool execution.
    pub permission_mode: PermissionMode,

    /// Tools to auto-approve without prompting.
    pub allowed_tools: Vec<String>,

    /// Tools to always deny.
    pub disallowed_tools: Vec<String>,

    /// Maximum budget in USD.
    pub max_budget_usd: Option<f64>,

    /// MCP server configurations.
    pub mcp_servers: HashMap<String, serde_json::Value>,

    /// Additional environment variables for the subprocess.
    pub env: HashMap<String, String>,
}

/// Permission mode for the Claude engine session.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PermissionMode {
    #[default]
    Default,
    AcceptEdits,
    BypassPermissions,
    Plan,
    DontAsk,
}

impl PermissionMode {
    /// Returns the SDK wire value.
    pub fn as_sdk_str(&self) -> &str {
        match self {
            Self::Default => "default",
            Self::AcceptEdits => "acceptEdits",
            Self::BypassPermissions => "bypassPermissions",
            Self::Plan => "plan",
            Self::DontAsk => "dontAsk",
        }
    }
}

/// Serializable config sent to the bridge script as JSON via process arg.
#[derive(Debug, Serialize)]
pub(crate) struct BridgeConfig {
    pub prompt: String,
    pub cwd: Option<String>,
    pub model: Option<String>,
    pub permission_mode: String,
    pub allowed_tools: Vec<String>,
    pub disallowed_tools: Vec<String>,
    pub max_budget_usd: Option<f64>,
    pub mcp_servers: HashMap<String, serde_json::Value>,
    pub env: HashMap<String, String>,
    pub api_key: Option<String>,
    pub sdk_executable_path: Option<String>,
    pub system_prompt: Option<String>,
    pub streaming: bool,
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-engine-adapter-claude`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/config.rs
git commit -m "feat(alva-engine-adapter-claude): add ClaudeAdapterConfig and BridgeConfig"
```

---

### Task 8: Implement SDK protocol types

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/protocol.rs`

**Step 1: Write protocol types**

```rust
use serde::Deserialize;
use serde_json::Value;

/// Messages received from the bridge script via stdout.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum BridgeMessage {
    #[serde(rename = "sdk_message")]
    SdkMessage { message: SdkMessage },

    #[serde(rename = "permission_request")]
    PermissionRequest {
        request_id: String,
        tool_name: String,
        tool_input: Value,
    },

    #[serde(rename = "done")]
    Done,

    #[serde(rename = "error")]
    Error { message: String },
}

/// SDK message types we care about. Unknown types are silently ignored.
#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkMessage {
    #[serde(rename = "system")]
    System {
        subtype: Option<String>,
        session_id: Option<String>,
        model: Option<String>,
        tools: Option<Vec<String>>,
    },

    #[serde(rename = "assistant")]
    Assistant {
        uuid: Option<String>,
        session_id: Option<String>,
        message: Option<SdkAssistantPayload>,
    },

    #[serde(rename = "stream_event")]
    StreamEvent {
        uuid: Option<String>,
        event: Option<Value>,
    },

    #[serde(rename = "result")]
    Result {
        subtype: Option<String>,
        session_id: Option<String>,
        result: Option<String>,
        total_cost_usd: Option<f64>,
        duration_ms: Option<u64>,
        num_turns: Option<u32>,
        usage: Option<SdkUsage>,
    },

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct SdkAssistantPayload {
    pub content: Option<Vec<SdkContentBlock>>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type")]
pub enum SdkContentBlock {
    #[serde(rename = "text")]
    Text { text: String },

    #[serde(rename = "thinking")]
    Thinking { thinking: String },

    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: Value,
    },

    #[serde(rename = "tool_result")]
    ToolResult {
        tool_use_id: String,
        content: Option<String>,
        is_error: Option<bool>,
    },

    #[serde(other)]
    Other,
}

#[derive(Debug, Deserialize)]
pub struct SdkUsage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
}

/// Outbound messages sent to the bridge script via stdin.
#[derive(Debug, serde::Serialize)]
#[serde(tag = "type")]
pub enum BridgeOutbound {
    #[serde(rename = "permission_response")]
    PermissionResponse {
        request_id: String,
        decision: BridgePermissionDecision,
    },

    #[serde(rename = "cancel")]
    Cancel,

    #[serde(rename = "shutdown")]
    Shutdown,
}

#[derive(Debug, serde::Serialize)]
#[serde(tag = "behavior")]
pub enum BridgePermissionDecision {
    #[serde(rename = "allow")]
    Allow {
        #[serde(skip_serializing_if = "Option::is_none")]
        updated_input: Option<Value>,
    },
    #[serde(rename = "deny")]
    Deny { message: String },
}
```

**Step 2: Write deserialization tests**

Add at the bottom of `protocol.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_system_init() {
        let json = r#"{"type":"sdk_message","message":{"type":"system","subtype":"init","session_id":"abc","model":"claude-sonnet-4-6","tools":["Read","Write"]}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::System { subtype, session_id, model, tools } } => {
                assert_eq!(subtype.as_deref(), Some("init"));
                assert_eq!(session_id.as_deref(), Some("abc"));
                assert_eq!(model.as_deref(), Some("claude-sonnet-4-6"));
                assert_eq!(tools.as_ref().unwrap().len(), 2);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_result_success() {
        let json = r#"{"type":"sdk_message","message":{"type":"result","subtype":"success","session_id":"abc","result":"done","total_cost_usd":0.05,"duration_ms":1200,"num_turns":3,"usage":{"input_tokens":100,"output_tokens":200}}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::Result { subtype, total_cost_usd, .. } } => {
                assert_eq!(subtype.as_deref(), Some("success"));
                assert!((total_cost_usd.unwrap() - 0.05).abs() < f64::EPSILON);
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_assistant_with_tool_use() {
        let json = r#"{"type":"sdk_message","message":{"type":"assistant","uuid":"u1","session_id":"s1","message":{"content":[{"type":"text","text":"hello"},{"type":"tool_use","id":"t1","name":"Read","input":{"file_path":"/tmp/test.rs"}}]}}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        match msg {
            BridgeMessage::SdkMessage { message: SdkMessage::Assistant { message: Some(payload), .. } } => {
                let blocks = payload.content.unwrap();
                assert_eq!(blocks.len(), 2);
                assert!(matches!(&blocks[0], SdkContentBlock::Text { text } if text == "hello"));
                assert!(matches!(&blocks[1], SdkContentBlock::ToolUse { name, .. } if name == "Read"));
            }
            other => panic!("unexpected: {other:?}"),
        }
    }

    #[test]
    fn test_parse_permission_request() {
        let json = r#"{"type":"permission_request","request_id":"r1","tool_name":"Bash","tool_input":{"command":"ls"}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, BridgeMessage::PermissionRequest { request_id, .. } if request_id == "r1"));
    }

    #[test]
    fn test_parse_unknown_sdk_message() {
        let json = r#"{"type":"sdk_message","message":{"type":"some_future_type","data":123}}"#;
        let msg: BridgeMessage = serde_json::from_str(json).unwrap();
        assert!(matches!(msg, BridgeMessage::SdkMessage { message: SdkMessage::Unknown }));
    }

    #[test]
    fn test_serialize_permission_response() {
        let msg = BridgeOutbound::PermissionResponse {
            request_id: "r1".into(),
            decision: BridgePermissionDecision::Allow { updated_input: None },
        };
        let json = serde_json::to_string(&msg).unwrap();
        assert!(json.contains("permission_response"));
        assert!(json.contains("allow"));
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p alva-engine-adapter-claude`
Expected: All 6 tests pass.

**Step 4: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/protocol.rs
git commit -m "feat(alva-engine-adapter-claude): add SDK protocol types with deserialization tests"
```

---

### Task 9: Implement event mapping

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/mapping.rs`

**Step 1: Write the mapper**

```rust
use std::collections::HashMap;

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_types::{ContentBlock, MessageRole, StreamEvent, ToolResult};

use crate::protocol::{BridgeMessage, SdkContentBlock, SdkMessage};

/// Stateful mapper that converts bridge messages to runtime events.
///
/// Maintains a tool_use_id → tool_name lookup to populate ToolEnd.name,
/// since SDK tool_result blocks only carry tool_use_id.
pub(crate) struct EventMapper {
    session_id: String,
    tool_names: HashMap<String, String>,
}

impl EventMapper {
    pub fn new() -> Self {
        Self {
            session_id: String::new(),
            tool_names: HashMap::new(),
        }
    }

    /// Map a bridge message to zero or more runtime events.
    pub fn map(&mut self, msg: BridgeMessage) -> Vec<RuntimeEvent> {
        match msg {
            BridgeMessage::SdkMessage { message } => self.map_sdk_message(message),
            BridgeMessage::PermissionRequest { request_id, tool_name, tool_input } => {
                vec![RuntimeEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    tool_input,
                    description: None,
                }]
            }
            BridgeMessage::Done => vec![],
            BridgeMessage::Error { message } => vec![
                RuntimeEvent::Error { message: message.clone(), recoverable: false },
                RuntimeEvent::Completed {
                    session_id: self.session_id.clone(),
                    result: None,
                    usage: None,
                },
            ],
        }
    }

    fn map_sdk_message(&mut self, msg: SdkMessage) -> Vec<RuntimeEvent> {
        match msg {
            SdkMessage::System { subtype, session_id, model, tools } => {
                if subtype.as_deref() == Some("init") {
                    if let Some(sid) = &session_id {
                        self.session_id = sid.clone();
                    }
                    vec![RuntimeEvent::SessionStarted {
                        session_id: session_id.unwrap_or_default(),
                        model,
                        tools: tools.unwrap_or_default(),
                    }]
                } else {
                    vec![]
                }
            }

            SdkMessage::Assistant { uuid, message, .. } => {
                let Some(payload) = message else { return vec![] };
                let Some(blocks) = payload.content else { return vec![] };
                let msg_id = uuid.unwrap_or_default();
                let mut events = Vec::new();

                // Split: text/reasoning → Message, tool_use → ToolStart, tool_result → ToolEnd
                let mut text_blocks = Vec::new();
                for block in blocks {
                    match block {
                        SdkContentBlock::Text { text } => {
                            text_blocks.push(ContentBlock::Text { text });
                        }
                        SdkContentBlock::Thinking { thinking } => {
                            text_blocks.push(ContentBlock::Reasoning { text: thinking });
                        }
                        SdkContentBlock::ToolUse { id, name, input } => {
                            self.tool_names.insert(id.clone(), name.clone());
                            events.push(RuntimeEvent::ToolStart { id, name, input });
                        }
                        SdkContentBlock::ToolResult { tool_use_id, content, is_error } => {
                            let name = self.tool_names.get(&tool_use_id).cloned().unwrap_or_default();
                            events.push(RuntimeEvent::ToolEnd {
                                id: tool_use_id,
                                name,
                                result: ToolResult {
                                    content: content.unwrap_or_default(),
                                    is_error: is_error.unwrap_or(false),
                                    details: None,
                                },
                                duration_ms: None,
                            });
                        }
                        SdkContentBlock::Other => {}
                    }
                }

                if !text_blocks.is_empty() {
                    events.insert(0, RuntimeEvent::Message {
                        id: msg_id,
                        role: MessageRole::Assistant,
                        content: text_blocks,
                    });
                }

                events
            }

            SdkMessage::StreamEvent { uuid, event } => {
                let msg_id = uuid.unwrap_or_default();
                let Some(event_val) = event else { return vec![] };

                // Parse the stream event delta
                if let Some(delta) = parse_stream_delta(&event_val) {
                    vec![RuntimeEvent::MessageDelta { id: msg_id, delta }]
                } else {
                    vec![]
                }
            }

            SdkMessage::Result { subtype, session_id, result, total_cost_usd, duration_ms, num_turns, usage } => {
                let sid = session_id.unwrap_or_else(|| self.session_id.clone());
                let is_error = subtype.as_deref() != Some("success");

                let usage_data = RuntimeUsage {
                    input_tokens: usage.as_ref().and_then(|u| u.input_tokens).unwrap_or(0) as u32,
                    output_tokens: usage.as_ref().and_then(|u| u.output_tokens).unwrap_or(0) as u32,
                    total_cost_usd,
                    duration_ms: duration_ms.unwrap_or(0),
                    num_turns: num_turns.unwrap_or(0),
                };

                let mut events = Vec::new();
                if is_error {
                    events.push(RuntimeEvent::Error {
                        message: format!("Session ended with: {}", subtype.as_deref().unwrap_or("unknown")),
                        recoverable: false,
                    });
                }
                events.push(RuntimeEvent::Completed {
                    session_id: sid,
                    result: if is_error { None } else { result },
                    usage: Some(usage_data),
                });
                events
            }

            SdkMessage::Unknown => vec![],
        }
    }
}

/// Parse a raw stream event JSON into a StreamEvent.
fn parse_stream_delta(event: &serde_json::Value) -> Option<StreamEvent> {
    let event_type = event.get("type")?.as_str()?;
    match event_type {
        "content_block_delta" => {
            let delta = event.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;
            match delta_type {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    Some(StreamEvent::TextDelta { text })
                }
                "thinking_delta" => {
                    let text = delta.get("thinking")?.as_str()?.to_string();
                    Some(StreamEvent::ReasoningDelta { text })
                }
                "input_json_delta" => {
                    let partial = delta.get("partial_json")?.as_str()?.to_string();
                    // index → tool call id (from content_block_start)
                    let id = event.get("index").and_then(|v| v.as_u64()).unwrap_or(0).to_string();
                    Some(StreamEvent::ToolCallDelta {
                        id,
                        name: None,
                        arguments_delta: partial,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}
```

**Step 2: Write mapping tests**

Add at the bottom of `mapping.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::*;

    #[test]
    fn test_map_system_init() {
        let mut mapper = EventMapper::new();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::System {
                subtype: Some("init".into()),
                session_id: Some("s1".into()),
                model: Some("claude-sonnet-4-6".into()),
                tools: Some(vec!["Read".into(), "Write".into()]),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::SessionStarted { session_id, .. } if session_id == "s1"));
    }

    #[test]
    fn test_map_assistant_splits_tool_use() {
        let mut mapper = EventMapper::new();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Assistant {
                uuid: Some("u1".into()),
                session_id: Some("s1".into()),
                message: Some(SdkAssistantPayload {
                    content: Some(vec![
                        SdkContentBlock::Text { text: "Let me read that.".into() },
                        SdkContentBlock::ToolUse {
                            id: "tc1".into(),
                            name: "Read".into(),
                            input: serde_json::json!({"file_path": "/tmp/a.rs"}),
                        },
                    ]),
                }),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Message { content, .. } if content.len() == 1));
        assert!(matches!(&events[1], RuntimeEvent::ToolStart { name, .. } if name == "Read"));
    }

    #[test]
    fn test_map_tool_result_resolves_name() {
        let mut mapper = EventMapper::new();
        // First: register tool name via ToolUse
        mapper.tool_names.insert("tc1".into(), "Bash".into());
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Assistant {
                uuid: Some("u2".into()),
                session_id: Some("s1".into()),
                message: Some(SdkAssistantPayload {
                    content: Some(vec![SdkContentBlock::ToolResult {
                        tool_use_id: "tc1".into(),
                        content: Some("output".into()),
                        is_error: Some(false),
                    }]),
                }),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::ToolEnd { name, .. } if name == "Bash"));
    }

    #[test]
    fn test_map_result_error_emits_error_then_completed() {
        let mut mapper = EventMapper::new();
        mapper.session_id = "s1".into();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Result {
                subtype: Some("error_max_turns".into()),
                session_id: Some("s1".into()),
                result: None,
                total_cost_usd: Some(0.1),
                duration_ms: Some(5000),
                num_turns: Some(10),
                usage: None,
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Error { recoverable: false, .. }));
        assert!(matches!(&events[1], RuntimeEvent::Completed { result: None, .. }));
    }

    #[test]
    fn test_map_bridge_error() {
        let mut mapper = EventMapper::new();
        let events = mapper.map(BridgeMessage::Error { message: "crash".into() });
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Error { .. }));
        assert!(matches!(&events[1], RuntimeEvent::Completed { .. }));
    }
}
```

**Step 3: Run tests**

Run: `cargo test -p alva-engine-adapter-claude`
Expected: All tests pass.

**Step 4: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/mapping.rs
git commit -m "feat(alva-engine-adapter-claude): add EventMapper with tool-name tracking and tests"
```

---

### Task 10: Implement bridge script management

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/bridge.rs`

**Step 1: Write bridge.rs**

```rust
use std::path::PathBuf;

use alva_engine_runtime::RuntimeError;

const BRIDGE_SCRIPT: &str = include_str!("../bridge/index.mjs");
const BRIDGE_DIR_NAME: &str = "alva-engine-claude-bridge";

/// Ensure the bridge script exists in a user-level cache directory.
///
/// The script content is embedded at compile time via `include_str!`.
/// Only rewrites if content differs, avoiding unnecessary I/O.
///
/// **Contains sync I/O** — callers should use `spawn_blocking` in async context.
pub(crate) fn ensure_bridge_script() -> Result<PathBuf, RuntimeError> {
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join(BRIDGE_DIR_NAME);
    std::fs::create_dir_all(&base).map_err(|e| {
        RuntimeError::ProcessError(format!("Failed to create bridge directory: {e}"))
    })?;

    let script_path = base.join("index.mjs");
    let needs_write = match std::fs::read_to_string(&script_path) {
        Ok(existing) => existing != BRIDGE_SCRIPT,
        Err(_) => true,
    };

    if needs_write {
        std::fs::write(&script_path, BRIDGE_SCRIPT).map_err(|e| {
            RuntimeError::ProcessError(format!("Failed to write bridge script: {e}"))
        })?;
    }

    Ok(script_path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ensure_bridge_script_creates_file() {
        let path = ensure_bridge_script().unwrap();
        assert!(path.exists());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, BRIDGE_SCRIPT);
    }

    #[test]
    fn test_ensure_bridge_script_idempotent() {
        let path1 = ensure_bridge_script().unwrap();
        let path2 = ensure_bridge_script().unwrap();
        assert_eq!(path1, path2);
    }
}
```

**Step 2: Run tests**

Run: `cargo test -p alva-engine-adapter-claude -- bridge`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/bridge.rs
git commit -m "feat(alva-engine-adapter-claude): add bridge script management with include_str! embedding"
```

---

### Task 11: Implement the Node.js bridge script

**Files:**
- Create: `crates/alva-engine-adapter-claude/bridge/index.mjs`

**Step 1: Write the full bridge script**

```javascript
#!/usr/bin/env node
// bridge/index.mjs — Embedded in alva-engine-adapter-claude, written to cache at runtime.
//
// Protocol:
//   stdin  (Rust → Bridge): JSON-line control messages
//   stdout (Bridge → Rust): JSON-line events
//
// Rust process ←→ this script ←→ Claude Agent SDK ←→ Claude Code subprocess

import { createInterface } from "readline";

// Config is passed as the first CLI argument (JSON string).
const config = JSON.parse(process.argv[2] || "{}");

// --- stdout emitter ---
function emit(type, data = {}) {
  process.stdout.write(JSON.stringify({ type, ...data }) + "\n");
}

// --- stdin control message handler ---
const pendingPermissions = new Map();
let abortController = new AbortController();

const rl = createInterface({ input: process.stdin, crlfDelay: Infinity });
rl.on("line", (line) => {
  let msg;
  try { msg = JSON.parse(line); } catch { return; }

  if (msg.type === "permission_response" && msg.request_id) {
    const resolve = pendingPermissions.get(msg.request_id);
    if (resolve) {
      pendingPermissions.delete(msg.request_id);
      const decision = msg.decision || {};
      if (decision.behavior === "allow") {
        resolve({ behavior: "allow", updatedInput: decision.updated_input });
      } else {
        resolve({ behavior: "deny", message: decision.message || "Denied" });
      }
    }
  } else if (msg.type === "cancel") {
    abortController.abort();
  } else if (msg.type === "shutdown") {
    abortController.abort();
    setTimeout(() => process.exit(0), 1000);
  }
});

// --- canUseTool callback (bridges permissions to Rust) ---
async function canUseTool(toolName, toolInput, { signal }) {
  const requestId = crypto.randomUUID();
  emit("permission_request", {
    request_id: requestId,
    tool_name: toolName,
    tool_input: toolInput,
  });
  return new Promise((resolve) => {
    pendingPermissions.set(requestId, resolve);
    const onAbort = () => {
      pendingPermissions.delete(requestId);
      resolve({ behavior: "deny", message: "Aborted" });
    };
    if (signal) {
      signal.addEventListener("abort", onAbort, { once: true });
    }
    // Timeout: auto-deny after 60s if no response
    setTimeout(() => {
      if (pendingPermissions.has(requestId)) {
        pendingPermissions.delete(requestId);
        resolve({ behavior: "deny", message: "Permission timeout" });
      }
    }, 60_000);
  });
}

// --- Dynamic import of SDK ---
async function loadSdk() {
  try {
    return await import("@anthropic-ai/claude-agent-sdk");
  } catch {
    // If the SDK is not in node_modules, try the explicit path
    if (config.sdk_package_path) {
      return await import(config.sdk_package_path);
    }
    throw new Error(
      "Cannot find @anthropic-ai/claude-agent-sdk. " +
      "Install it via: npm install -g @anthropic-ai/claude-agent-sdk"
    );
  }
}

// --- Main ---
async function main() {
  const { query } = await loadSdk();

  const options = {
    cwd: config.cwd || process.cwd(),
    abortController,
    permissionMode: config.permission_mode || "default",
    allowedTools: config.allowed_tools || [],
    disallowedTools: config.disallowed_tools || [],
    includePartialMessages: config.streaming !== false,
    env: { ...process.env, ...(config.env || {}) },
  };

  if (config.model) options.model = config.model;
  if (config.max_budget_usd != null) options.maxBudgetUsd = config.max_budget_usd;
  if (config.system_prompt) options.systemPrompt = config.system_prompt;
  if (config.api_key) options.env.ANTHROPIC_API_KEY = config.api_key;
  if (config.sdk_executable_path) options.pathToClaudeCodeExecutable = config.sdk_executable_path;

  // MCP servers
  if (config.mcp_servers && Object.keys(config.mcp_servers).length > 0) {
    options.mcpServers = config.mcp_servers;
  }

  // Permission callback (only in default mode)
  if (config.permission_mode === "default" || !config.permission_mode) {
    options.canUseTool = canUseTool;
  }

  const result = query({ prompt: config.prompt, options });

  for await (const message of result) {
    emit("sdk_message", { message });
  }

  emit("done");
}

main().catch((err) => {
  emit("error", { message: err?.message || String(err) });
  process.exit(1);
});
```

**Step 2: Verify bridge compiles into crate**

Run: `cargo check -p alva-engine-adapter-claude`
Expected: PASS (the `include_str!` in bridge.rs should find the file).

**Step 3: Commit**

```bash
git add crates/alva-engine-adapter-claude/bridge/index.mjs
git commit -m "feat(alva-engine-adapter-claude): add Node.js bridge script with canUseTool callback"
```

---

### Task 12: Implement BridgeProcess

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/process.rs`

**Step 1: Write process management**

```rust
use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use alva_engine_runtime::RuntimeError;

use crate::protocol::{BridgeMessage, BridgeOutbound};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration for spawning the bridge process.
pub(crate) struct BridgeSpawnConfig {
    pub node_path: String,
    pub script_path: String,
    pub config_json: String,
    pub env: Vec<(String, String)>,
}

/// Manages the Node.js bridge child process lifecycle.
pub(crate) struct BridgeProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout_lines: Lines<BufReader<ChildStdout>>,
}

impl BridgeProcess {
    /// Spawn the Node.js bridge process.
    pub async fn spawn(config: BridgeSpawnConfig) -> Result<Self, RuntimeError> {
        let mut cmd = Command::new(&config.node_path);
        cmd.arg(&config.script_path)
            .arg(&config.config_json)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        for (key, val) in &config.env {
            cmd.env(key, val);
        }

        let mut child = cmd.spawn().map_err(|e| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Node.js bridge ({}): {}",
                config.node_path, e
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::ProcessError("Failed to capture stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::ProcessError("Failed to capture stdout".into())
        })?;
        let stderr = child.stderr.take();

        // Spawn stderr monitoring task
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if is_fatal_stderr(&line) {
                        warn!(target: "claude_bridge", "Fatal stderr: {}", line);
                    } else {
                        debug!(target: "claude_bridge", "stderr: {}", line);
                    }
                }
            });
        }

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout_lines: BufReader::new(stdout).lines(),
        })
    }

    /// Send a JSON-line control message to the bridge via stdin.
    pub async fn send(&mut self, msg: &BridgeOutbound) -> Result<(), RuntimeError> {
        let json = serde_json::to_string(msg)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read the next JSON-line message from stdout.
    /// Returns None when stdout is closed (process exited).
    pub async fn recv(&mut self) -> Result<Option<BridgeMessage>, RuntimeError> {
        match self.stdout_lines.next_line().await? {
            Some(line) => {
                let msg = serde_json::from_str(&line).map_err(|e| {
                    RuntimeError::ProtocolError(format!("Invalid JSON from bridge: {e} — line: {line}"))
                })?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    /// Graceful shutdown: send shutdown message, wait, then kill.
    pub async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        let _ = self.send(&BridgeOutbound::Shutdown).await;
        match timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => self.kill().await,
        }
    }

    /// Force-kill the process.
    pub async fn kill(&mut self) -> Result<(), RuntimeError> {
        self.child.kill().await.map_err(|e| {
            RuntimeError::ProcessError(format!("Failed to kill bridge process: {e}"))
        })
    }
}

fn is_fatal_stderr(line: &str) -> bool {
    let lower = line.to_lowercase();
    let patterns = [
        "authentication_error", "authentication error",
        "invalid_api_key", "invalid api key",
        "unauthorized", "rate_limit", "rate limit",
        "quota_exceeded", "billing", "overloaded",
        "connection_refused", "econnrefused",
    ];
    patterns.iter().any(|p| lower.contains(p))
}
```

**Step 2: Verify it compiles**

Run: `cargo check -p alva-engine-adapter-claude`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/process.rs
git commit -m "feat(alva-engine-adapter-claude): add BridgeProcess with spawn, send, recv, shutdown"
```

---

### Task 13: Implement ClaudeAdapter

**Files:**
- Create: `crates/alva-engine-adapter-claude/src/adapter.rs`

**Step 1: Write the adapter**

```rust
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};

use crate::bridge::ensure_bridge_script;
use crate::config::{BridgeConfig, ClaudeAdapterConfig};
use crate::mapping::EventMapper;
use crate::process::{BridgeProcess, BridgeSpawnConfig};
use crate::protocol::{BridgeOutbound, BridgePermissionDecision};

/// Claude Agent SDK bridge adapter.
///
/// Implements `EngineRuntime` by spawning a Node.js bridge process that
/// communicates with the Claude Agent SDK via stdin/stdout JSON-line protocol.
pub struct ClaudeAdapter {
    config: ClaudeAdapterConfig,
    /// Active sessions: session_id → sender for control messages.
    sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<BridgeOutbound>>>>,
}

impl ClaudeAdapter {
    pub fn new(config: ClaudeAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for ClaudeAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        // Write bridge script (sync I/O — acceptable here since it's a one-time idempotent write)
        let script_path = ensure_bridge_script()?;

        let bridge_config = BridgeConfig {
            prompt: request.prompt,
            cwd: request.working_directory.map(|p| p.to_string_lossy().into_owned()),
            model: self.config.model.clone(),
            permission_mode: self.config.permission_mode.as_sdk_str().to_string(),
            allowed_tools: self.config.allowed_tools.clone(),
            disallowed_tools: self.config.disallowed_tools.clone(),
            max_budget_usd: self.config.max_budget_usd,
            mcp_servers: self.config.mcp_servers.clone(),
            env: self.config.env.clone(),
            api_key: self.config.api_key.clone(),
            sdk_executable_path: self.config.sdk_package_path.clone(),
            system_prompt: request.system_prompt,
            streaming: request.options.streaming,
        };

        let config_json = serde_json::to_string(&bridge_config)?;
        let node_path = self.config.node_path.clone().unwrap_or_else(|| "node".into());

        let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let (control_tx, mut control_rx) = mpsc::unbounded_channel::<BridgeOutbound>();
        let sessions = self.sessions.clone();

        tokio::spawn(async move {
            // Spawn bridge process
            let spawn_config = BridgeSpawnConfig {
                node_path,
                script_path: script_path.to_string_lossy().into_owned(),
                config_json,
                env: vec![],
            };

            let mut process = match BridgeProcess::spawn(spawn_config).await {
                Ok(p) => p,
                Err(e) => {
                    let _ = event_tx.send(RuntimeEvent::Error {
                        message: e.to_string(),
                        recoverable: false,
                    });
                    let _ = event_tx.send(RuntimeEvent::Completed {
                        session_id: String::new(),
                        result: None,
                        usage: None,
                    });
                    return;
                }
            };

            let mut mapper = EventMapper::new();

            // Forward control messages (permission responses, cancel) to stdin
            let forward_handle = tokio::spawn({
                // We need a separate reference to process stdin.
                // Since we can't split BridgeProcess easily, we use the control channel
                // and forward in the main loop below.
                async move {
                    // This task just drains the channel; actual sending happens in main loop.
                    let mut msgs = Vec::new();
                    while let Some(msg) = control_rx.recv().await {
                        msgs.push(msg);
                    }
                    msgs
                }
            });

            // Main event loop: read stdout, map events, send to consumer
            loop {
                // Check for pending control messages first
                while let Ok(ctrl) = control_rx.try_recv() {
                    if let Err(e) = process.send(&ctrl).await {
                        error!(target: "claude_adapter", "Failed to send control message: {e}");
                    }
                }

                match process.recv().await {
                    Ok(Some(msg)) => {
                        let is_done = matches!(&msg, crate::protocol::BridgeMessage::Done);
                        let events = mapper.map(msg);

                        // Register session once we know the ID
                        for event in &events {
                            if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                                sessions.lock().await.insert(session_id.clone(), control_tx.clone());
                            }
                        }

                        for event in events {
                            let is_completed = matches!(&event, RuntimeEvent::Completed { .. });
                            if event_tx.send(event).is_err() {
                                break;
                            }
                            if is_completed {
                                let _ = process.shutdown().await;
                                return;
                            }
                        }

                        if is_done {
                            // Bridge script ended without a Result message — force Completed
                            let _ = event_tx.send(RuntimeEvent::Completed {
                                session_id: mapper.session_id().to_string(),
                                result: None,
                                usage: None,
                            });
                            let _ = process.shutdown().await;
                            return;
                        }
                    }
                    Ok(None) => {
                        // stdout closed — process exited
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Bridge process exited unexpectedly".into(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    Err(e) => {
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: e.to_string(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        let _ = process.kill().await;
                        return;
                    }
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let tx = sessions.get(session_id).ok_or_else(|| {
            RuntimeError::SessionNotFound(session_id.into())
        })?;
        tx.send(BridgeOutbound::Cancel).map_err(|_| {
            RuntimeError::ProcessError("Session channel closed".into())
        })
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let tx = sessions.get(session_id).ok_or_else(|| {
            RuntimeError::SessionNotFound(session_id.into())
        })?;

        let bridge_decision = match decision {
            PermissionDecision::Allow { updated_input } => {
                BridgePermissionDecision::Allow { updated_input }
            }
            PermissionDecision::Deny { message } => {
                BridgePermissionDecision::Deny { message }
            }
        };

        tx.send(BridgeOutbound::PermissionResponse {
            request_id: request_id.into(),
            decision: bridge_decision,
        }).map_err(|_| {
            RuntimeError::ProcessError("Session channel closed".into())
        })
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: false,
            permission_callback: true,
            resume: false,  // v1: not implemented
            cancel: true,
        }
    }
}
```

**Step 2: Add `session_id()` accessor to EventMapper**

In `mapping.rs`, add:

```rust
impl EventMapper {
    // ... existing methods ...

    pub fn session_id(&self) -> &str {
        &self.session_id
    }
}
```

**Step 3: Fix the control message forwarding**

The adapter spawns a task but uses `try_recv` in the main loop. Remove the unused `forward_handle` task and use `try_recv` directly (already in the loop). Delete the `forward_handle` spawn block.

**Step 4: Verify it compiles**

Run: `cargo check -p alva-engine-adapter-claude`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/alva-engine-adapter-claude/src/adapter.rs crates/alva-engine-adapter-claude/src/mapping.rs
git commit -m "feat(alva-engine-adapter-claude): implement ClaudeAdapter with EngineRuntime trait"
```

---

### Task 14: Final integration — verify full build

**Step 1: Build both crates**

Run: `cargo build -p alva-engine-runtime -p alva-engine-adapter-claude`
Expected: PASS

**Step 2: Run all tests**

Run: `cargo test -p alva-engine-runtime -p alva-engine-adapter-claude`
Expected: All tests pass.

**Step 3: Check the full workspace still builds**

Run: `cargo check --workspace`
Expected: PASS — new crates don't break existing ones.

**Step 4: Commit**

```bash
git add -A
git commit -m "feat: complete alva-engine-runtime + alva-engine-adapter-claude implementation"
```

---

### Task 15: Update memory with architecture decision

Update the project memory at `/Users/smallraw/.claude/projects/-Users-smallraw-Development-QuincyWork-srow-agent/memory/project_engine_architecture.md` to reflect the final naming (`EngineRuntime` not `AgentRuntime`) and implementation status.
