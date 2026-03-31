# Agent Runtime Communication Enhancement

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Unify tool execution context, add progress reporting, content/details separation, steering/follow-up injection, and credential refresh — inspired by pi-mono agent patterns.

**Architecture:** Core structural change: merge `CancellationToken` + `ToolContext` + new `ProgressSink` into unified `ToolExecutionContext` trait. Rename `ToolResult` → `ToolOutput` with multi-modal content. Add `MessageInjector` for runtime steering. Add `CredentialSource` for dynamic API keys.

**Tech Stack:** Rust, async_trait, tokio, serde_json

---

## Impact Summary

| Crate | Files Changed | Nature |
|-------|--------------|--------|
| alva-types | ~5 files | New types + trait redesign |
| alva-agent-core | ~7 files | Run loop, middleware, events, new injector |
| alva-agent-tools | ~16 files | Adapt execute signature |
| alva-app-core | ~8 files | Adapt execute signature + BaseAgent |
| alva-protocol-mcp | ~1 file | Adapt McpToolAdapter |
| alva-test | ~1 file | Adapt MockTool |
| alva-provider | ~2 files | CredentialSource |

---

## Task 1: New Foundation Types in alva-types

**Files:**
- Create: `crates/alva-types/src/tool/execution.rs`
- Modify: `crates/alva-types/src/tool/types.rs` (remove `ToolContext`, `LocalToolContext`, `EmptyToolContext`)
- Modify: `crates/alva-types/src/tool/mod.rs` (add mod execution)
- Modify: `crates/alva-types/src/lib.rs` (update re-exports)

**Step 1: Create `execution.rs` with new types**

```rust
// crates/alva-types/src/tool/execution.rs

use std::any::Any;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::base::cancel::CancellationToken;
use super::types::ToolFs;

// ---------------------------------------------------------------------------
// ProgressEvent — intermediate progress from tool execution
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ProgressEvent {
    StdoutLine { line: String },
    StderrLine { line: String },
    Status { message: String },
    Custom { data: serde_json::Value },
}

// ---------------------------------------------------------------------------
// ToolContent — multi-modal content returned to the model
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ToolContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, media_type: String },
}

impl ToolContent {
    pub fn text(text: impl Into<String>) -> Self {
        Self::Text { text: text.into() }
    }

    pub fn image(data: String, media_type: String) -> Self {
        Self::Image { data, media_type }
    }

    /// Extract text content, if this is a Text variant.
    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { text } => Some(text),
            _ => None,
        }
    }

    /// Concatenate all text content into a single string (for model consumption).
    pub fn to_model_string(contents: &[ToolContent]) -> String {
        contents
            .iter()
            .filter_map(|c| c.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ---------------------------------------------------------------------------
// ToolOutput — replaces ToolResult, with content/details separation
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    /// Content returned to the model (can be multi-modal).
    pub content: Vec<ToolContent>,
    /// Whether the execution resulted in an error.
    pub is_error: bool,
    /// Rich details for UI rendering (not sent to model).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

impl ToolOutput {
    /// Convenience: create a text-only successful output.
    pub fn text(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(text)],
            is_error: false,
            details: None,
        }
    }

    /// Convenience: create a text-only error output.
    pub fn error(text: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::text(text)],
            is_error: true,
            details: None,
        }
    }

    /// Get the concatenated text content for model consumption.
    pub fn model_text(&self) -> String {
        ToolContent::to_model_string(&self.content)
    }
}

// ---------------------------------------------------------------------------
// ToolExecutionContext — unified context for tool execution
// ---------------------------------------------------------------------------

/// Unified execution context passed to every tool invocation.
///
/// Replaces the old separate `CancellationToken` + `ToolContext` parameters.
/// Provides both execution control (cancel, progress) and environment access
/// (session, config, filesystem) through a single trait.
#[async_trait::async_trait]
pub trait ToolExecutionContext: Send + Sync {
    // -- Execution control --

    /// Cooperative cancellation token.
    fn cancel_token(&self) -> &CancellationToken;

    /// Report intermediate progress during tool execution.
    /// Implementations bridge this to the agent event system.
    /// No-op by default.
    fn report_progress(&self, _event: ProgressEvent) {}

    // -- Environment access --

    /// Current session identifier.
    fn session_id(&self) -> &str;

    /// Read a configuration value by key.
    fn get_config(&self, _key: &str) -> Option<String> {
        None
    }

    /// Workspace / project root path. None for non-local contexts.
    fn workspace(&self) -> Option<&Path> {
        None
    }

    /// Whether dangerous operations (e.g., rm -rf) are allowed.
    fn allow_dangerous(&self) -> bool {
        false
    }

    /// Abstract FS interface (sandbox, remote, or mock).
    fn tool_fs(&self) -> Option<&dyn ToolFs> {
        None
    }

    /// Downcast support for context extensions.
    fn as_any(&self) -> &dyn Any;
}

// ---------------------------------------------------------------------------
// MinimalExecutionContext — no-op context for testing and simple use cases
// ---------------------------------------------------------------------------

/// Minimal execution context with no filesystem, no progress, no config.
/// Replaces the old `EmptyToolContext`.
pub struct MinimalExecutionContext {
    cancel: CancellationToken,
}

impl MinimalExecutionContext {
    pub fn new() -> Self {
        Self {
            cancel: CancellationToken::new(),
        }
    }

    pub fn with_cancel(cancel: CancellationToken) -> Self {
        Self { cancel }
    }
}

impl Default for MinimalExecutionContext {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolExecutionContext for MinimalExecutionContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
    fn session_id(&self) -> &str {
        ""
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
```

**Step 2: Update `types.rs` — remove old ToolContext, rename ToolResult**

In `crates/alva-types/src/tool/types.rs`:

- **Remove**: `ToolContext` trait (lines 48-73), `LocalToolContext` trait (lines 79-86), `EmptyToolContext` (lines 141-158)
- **Rename**: `ToolResult` → keep as a type alias for backward compat during migration, or remove entirely
- **Keep**: `ToolDefinition`, `ToolCall`, `ToolFs`, `ToolFsExecResult`, `ToolFsDirEntry`, `ToolRegistry`
- **Update `Tool` trait** (lines 164-195):

```rust
#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: self.description().to_string(),
            parameters: self.parameters_schema(),
        }
    }

    /// Execute the tool.
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: &dyn super::execution::ToolExecutionContext,
    ) -> Result<super::execution::ToolOutput, crate::base::error::AgentError>;
}
```

**Step 3: Update `mod.rs` and `lib.rs` re-exports**

In `crates/alva-types/src/tool/mod.rs`, add:
```rust
pub mod execution;
```

In `crates/alva-types/src/lib.rs`, update the re-export line (line 31):
```rust
// Old:
pub use tool::{EmptyToolContext, LocalToolContext, Tool, ToolCall, ToolContext, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult, ToolRegistry, ToolResult};

// New:
pub use tool::{Tool, ToolCall, ToolDefinition, ToolFs, ToolFsDirEntry, ToolFsExecResult, ToolRegistry};
pub use tool::execution::{MinimalExecutionContext, ProgressEvent, ToolContent, ToolExecutionContext, ToolOutput};
```

**Step 4: Verify alva-types compiles**

Run: `cargo check -p alva-types`
Expected: PASS (no downstream dependents checked yet)

**Step 5: Commit**

```bash
git add crates/alva-types/
git commit -m "refactor: unify ToolExecutionContext, replace ToolResult with ToolOutput"
```

---

## Task 2: Update AgentMessage Variants

**Files:**
- Modify: `crates/alva-types/src/base/message.rs:79-87`

**Step 1: Update AgentMessage enum**

Replace the existing `AgentMessage` enum (lines 79-87):

```rust
/// Wraps either a standard LLM message or application-level messages
/// that flow through the agent event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentMessage {
    /// Standard LLM message (user, assistant, system, tool).
    Standard(Message),
    /// User mid-turn intervention — injected after current tool execution completes.
    Steering(Message),
    /// System/middleware follow-up — appended when agent would otherwise stop.
    FollowUp(Message),
    /// State marker (checkpoint, phase change) — never sent to LLM.
    Marker(Marker),
    /// Generic extension point for application-specific messages.
    Extension {
        type_name: String,
        data: Value,
    },
}

/// Markers for state transitions and checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "marker_type")]
pub enum Marker {
    CheckpointCreated { id: String },
    PhaseChange { from: String, to: String },
}
```

**Step 2: Verify compiles**

Run: `cargo check -p alva-types`
Expected: FAIL — downstream code references `AgentMessage::Custom`. That's expected; we fix it in Task 5.

**Step 3: Commit**

```bash
git add crates/alva-types/src/base/message.rs
git commit -m "feat: add Steering, FollowUp, Marker variants to AgentMessage"
```

---

## Task 3: Update Agent Core — Middleware & Events

**Files:**
- Modify: `crates/alva-agent-core/src/middleware.rs` (ToolResult → ToolOutput in trait)
- Modify: `crates/alva-agent-core/src/event.rs` (ToolResult → ToolOutput, ProgressEvent)
- Modify: `crates/alva-agent-core/src/shared.rs` (no changes expected)

**Step 1: Update `middleware.rs`**

Replace all `ToolResult` references with `ToolOutput`:

- Line 9: `use alva_types::{AgentError, Message, ToolCall, ToolResult};` → `use alva_types::{AgentError, Message, ToolCall, ToolOutput};`
- `ToolCallFn` trait (line 29): `-> Result<ToolResult, AgentError>` → `-> Result<ToolOutput, AgentError>`
- `Middleware::after_tool_call` (line 100-107): `_result: &mut ToolResult` → `_result: &mut ToolOutput`
- `Middleware::wrap_tool_call` (line 112-121): `-> Result<ToolResult, MiddlewareError>` → `-> Result<ToolOutput, MiddlewareError>`
- `MiddlewareStack::run_after_tool_call`: same rename
- `MiddlewareStack::run_wrap_tool_call` and chain helpers: same rename
- `ChainedToolCall::call`: same rename

**Step 2: Update `event.rs`**

```rust
use alva_types::{StreamEvent, ToolCall, ToolOutput};
use alva_types::ProgressEvent;
use alva_types::AgentMessage;

#[derive(Debug, Clone)]
pub enum AgentEvent {
    AgentStart,
    AgentEnd { error: Option<String> },

    TurnStart,
    TurnEnd,

    MessageStart { message: AgentMessage },
    MessageUpdate { message: AgentMessage, delta: StreamEvent },
    MessageEnd { message: AgentMessage },

    ToolExecutionStart { tool_call: ToolCall },
    /// Intermediate progress from a running tool.
    ToolExecutionUpdate {
        tool_call_id: String,
        event: ProgressEvent,
    },
    ToolExecutionEnd {
        tool_call: ToolCall,
        result: ToolOutput,
    },
}
```

**Step 3: Commit**

```bash
git add crates/alva-agent-core/src/middleware.rs crates/alva-agent-core/src/event.rs
git commit -m "refactor: update middleware and events to use ToolOutput"
```

---

## Task 4: Update Run Loop — New Execute Signature + RuntimeExecutionContext

**Files:**
- Modify: `crates/alva-agent-core/src/run.rs`
- Create: `crates/alva-agent-core/src/runtime_context.rs`

**Step 1: Create `runtime_context.rs`**

The run loop needs a concrete `ToolExecutionContext` implementation that bridges tool progress to the event channel.

```rust
// crates/alva-agent-core/src/runtime_context.rs

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_types::base::cancel::CancellationToken;
use alva_types::tool::execution::{ProgressEvent, ToolExecutionContext};
use alva_types::tool::types::ToolFs;
use tokio::sync::mpsc;

use crate::event::AgentEvent;

/// Concrete ToolExecutionContext used by the agent run loop.
///
/// Bridges `report_progress()` calls from tools to `AgentEvent::ToolExecutionUpdate`
/// events on the agent's event channel.
pub struct RuntimeExecutionContext {
    cancel: CancellationToken,
    tool_call_id: String,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    workspace: Option<PathBuf>,
    allow_dangerous: bool,
    tool_fs: Option<Arc<dyn ToolFs>>,
}

impl RuntimeExecutionContext {
    pub fn new(
        cancel: CancellationToken,
        tool_call_id: String,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        session_id: String,
    ) -> Self {
        Self {
            cancel,
            tool_call_id,
            event_tx,
            session_id,
            workspace: None,
            allow_dangerous: false,
            tool_fs: None,
        }
    }

    pub fn with_workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    pub fn with_allow_dangerous(mut self, allow: bool) -> Self {
        self.allow_dangerous = allow;
        self
    }

    pub fn with_tool_fs(mut self, fs: Arc<dyn ToolFs>) -> Self {
        self.tool_fs = Some(fs);
        self
    }
}

impl ToolExecutionContext for RuntimeExecutionContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    fn report_progress(&self, event: ProgressEvent) {
        let _ = self.event_tx.send(AgentEvent::ToolExecutionUpdate {
            tool_call_id: self.tool_call_id.clone(),
            event,
        });
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }

    fn tool_fs(&self) -> Option<&dyn ToolFs> {
        self.tool_fs.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
```

**Step 2: Update `run.rs`**

Key changes:
1. `ActualToolCall` uses `RuntimeExecutionContext` instead of `EmptyToolContext`
2. Replace `ToolResult` with `ToolOutput` everywhere
3. Update `AgentMessage::Custom` pattern matches to handle new variants
4. Build `ContentBlock::ToolResult` from `ToolOutput.model_text()`

Update `ActualToolCall`:
```rust
struct ActualToolCall {
    tool: Arc<dyn Tool>,
    cancel: CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
}

#[async_trait]
impl ToolCallFn for ActualToolCall {
    async fn call(&self, _state: &mut AgentState, tool_call: &ToolCall) -> Result<ToolOutput, AgentError> {
        let ctx = RuntimeExecutionContext::new(
            self.cancel.clone(),
            tool_call.id.clone(),
            self.event_tx.clone(),
            self.session_id.clone(),
        );
        self.tool.execute(tool_call.arguments.clone(), &ctx).await
    }
}
```

Update LLM message building (line 138-142) to handle new AgentMessage variants:
```rust
for msg in &session_messages {
    match msg {
        AgentMessage::Standard(m) => llm_messages.push(m.clone()),
        AgentMessage::Steering(m) => llm_messages.push(m.clone()),
        // FollowUp, Marker, Extension are not sent to LLM
        _ => {}
    }
}
```

Update tool result construction to use `ToolOutput`:
```rust
// Blocked by middleware:
ToolOutput::error(format!("Tool call blocked: {}", reason))

// Tool not found:
ToolOutput::error(format!("Tool not found: {}", tool_call.name))
```

Update `ContentBlock::ToolResult` construction (lines 277-288):
```rust
let tool_message = Message {
    id: uuid::Uuid::new_v4().to_string(),
    role: MessageRole::Tool,
    content: vec![ContentBlock::ToolResult {
        id: tool_call.id.clone(),
        content: result.model_text(),  // Use model_text() instead of result.content
        is_error: result.is_error,
    }],
    tool_call_id: Some(tool_call.id.clone()),
    usage: None,
    timestamp: chrono::Utc::now().timestamp_millis(),
};
```

**Step 3: Update `run.rs` tests**

The `simple_echo` test and other tests in run.rs use `AgentState` which no longer has `EmptyToolContext` usage. These tests don't use tools so they should still work after the signature changes.

**Step 4: Add `runtime_context` module to `lib.rs`**

```rust
pub mod runtime_context;
pub use runtime_context::RuntimeExecutionContext;
```

**Step 5: Verify agent-core compiles**

Run: `cargo check -p alva-agent-core`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/alva-agent-core/
git commit -m "refactor: RuntimeExecutionContext bridges tool progress to AgentEvent"
```

---

## Task 5: Update Builtin Middleware

**Files:**
- Modify: `crates/alva-agent-core/src/builtins/tool_timeout.rs`
- Modify: `crates/alva-agent-core/src/builtins/dangling_tool_call.rs`

**Step 1: Update `tool_timeout.rs`**

Replace `ToolResult` with `ToolOutput`:
```rust
use alva_types::{ToolCall, ToolOutput};

// In wrap_tool_call return type:
) -> Result<ToolOutput, MiddlewareError> {

// In timeout error branch:
Err(_) => Ok(ToolOutput::error(format!(
    "Tool '{}' timed out after {:?}. Consider breaking the task into smaller steps.",
    tool_call.name, self.timeout
))),
```

**Step 2: Update `dangling_tool_call.rs` if it references ToolResult**

Check and update accordingly.

**Step 3: Commit**

```bash
git add crates/alva-agent-core/src/builtins/
git commit -m "refactor: update builtin middleware to use ToolOutput"
```

---

## Task 6: Update All Tool Implementations

**Files:** 24 files across 3 crates (see list below)

All tools follow the same mechanical transformation:

**Before:**
```rust
async fn execute(
    &self,
    input: Value,
    _cancel: &CancellationToken,
    ctx: &dyn ToolContext,
) -> Result<ToolResult, AgentError> {
```

**After:**
```rust
async fn execute(
    &self,
    input: Value,
    ctx: &dyn ToolExecutionContext,
) -> Result<ToolOutput, AgentError> {
```

**For each tool:**
1. Change `execute` signature
2. Replace `CancellationToken` usage with `ctx.cancel_token()`
3. Replace `ctx.local().ok_or(...)?.workspace()` with `ctx.workspace().ok_or(...)?`
4. Replace `ToolResult { content, is_error, details }` with `ToolOutput { content: vec![ToolContent::text(...)], is_error, details }`
   - Or use `ToolOutput::text(...)` / `ToolOutput::error(...)` convenience methods
5. Update imports

**Step 1: alva-agent-tools (16 files)**

Files:
- `crates/alva-agent-tools/src/execute_shell.rs`
- `crates/alva-agent-tools/src/create_file.rs`
- `crates/alva-agent-tools/src/file_edit.rs`
- `crates/alva-agent-tools/src/grep_search.rs`
- `crates/alva-agent-tools/src/list_files.rs`
- `crates/alva-agent-tools/src/ask_human.rs`
- `crates/alva-agent-tools/src/view_image.rs`
- `crates/alva-agent-tools/src/internet_search.rs`
- `crates/alva-agent-tools/src/read_url.rs`
- `crates/alva-agent-tools/src/browser/browser_start.rs`
- `crates/alva-agent-tools/src/browser/browser_stop.rs`
- `crates/alva-agent-tools/src/browser/browser_navigate.rs`
- `crates/alva-agent-tools/src/browser/browser_action.rs`
- `crates/alva-agent-tools/src/browser/browser_snapshot.rs`
- `crates/alva-agent-tools/src/browser/browser_screenshot.rs`
- `crates/alva-agent-tools/src/browser/browser_status.rs`

Example transformation for `execute_shell.rs`:

```rust
// Before:
use alva_types::{AgentError, CancellationToken, ToolResult};
use alva_types::tool::{Tool, ToolContext, ToolFs};

// After:
use alva_types::AgentError;
use alva_types::tool::Tool;
use alva_types::tool::execution::{ToolExecutionContext, ToolOutput, ToolContent};
use alva_types::tool::types::ToolFs;

// execute signature:
async fn execute(
    &self,
    input: Value,
    ctx: &dyn ToolExecutionContext,
) -> Result<ToolOutput, AgentError> {
    // ...
    let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
        tool_name: self.name().into(),
        message: "workspace required".into(),
    })?;
    let fallback = LocalToolFs::new(workspace);
    let fs = ctx.tool_fs().unwrap_or(&fallback);
    // ...
    Ok(ToolOutput {
        content: vec![ToolContent::text(output_text)],
        is_error: result.exit_code != 0,
        details: Some(json!({
            "stdout": result.stdout,
            "stderr": result.stderr,
            "exit_code": result.exit_code,
        })),
    })
}
```

**Step 2: alva-app-core tools (6 files)**

Files:
- `crates/alva-app-core/src/plugins/team.rs` (line 100)
- `crates/alva-app-core/src/plugins/agent_spawn.rs` (line 71)
- `crates/alva-app-core/src/plugins/task_spawn.rs` (line 120)
- `crates/alva-app-core/src/agent/agent_client/delegate.rs` (line 209)
- `crates/alva-app-core/src/mcp/tools.rs` (line 33)
- `crates/alva-app-core/src/skills/tools.rs` (lines 32 and 104)

**Step 3: alva-protocol-mcp (1 file)**

- `crates/alva-protocol-mcp/src/tool_adapter.rs` (line 49)

**Step 4: alva-test (1 file)**

- `crates/alva-test/src/mock_tool.rs` (line 69)

Update MockTool to return `ToolOutput` instead of `ToolResult`.

**Step 5: Verify full workspace compiles**

Run: `cargo check --workspace`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/alva-agent-tools/ crates/alva-app-core/ crates/alva-protocol-mcp/ crates/alva-test/
git commit -m "refactor: update all 24 Tool implementations to new execute signature"
```

---

## Task 7: MessageInjector + Double Loop

**Files:**
- Create: `crates/alva-agent-core/src/injector.rs`
- Modify: `crates/alva-agent-core/src/state.rs` (add injector to AgentState)
- Modify: `crates/alva-agent-core/src/run.rs` (double-loop refactor)
- Modify: `crates/alva-agent-core/src/lib.rs` (re-export)

**Step 1: Write failing test**

Add to `crates/alva-agent-core/tests/v2_integration.rs`:

```rust
#[tokio::test]
async fn steering_injects_mid_run() {
    // Model that returns tool_call on first turn, then text on second
    // After tool execution, a steering message should be injected
    // and the agent should process it before finishing.
    // (Detailed test with ToolCallModel + steering injection)
}

#[tokio::test]
async fn follow_up_continues_after_natural_stop() {
    // Agent finishes naturally (no tool calls).
    // A follow-up message is queued.
    // Agent should continue and process the follow-up.
}
```

**Step 2: Create `injector.rs`**

```rust
// crates/alva-agent-core/src/injector.rs

use std::collections::VecDeque;
use std::sync::Mutex;

use alva_types::AgentMessage;

/// Runtime message injection for agent steering and follow-up.
///
/// Thread-safe: can be called from UI thread while agent loop runs.
///
/// - **Steering**: replaces any pending steering message (max 1).
///   Consumed after current tool execution completes.
/// - **Follow-up**: accumulates. Consumed when agent would otherwise stop.
pub struct MessageInjector {
    steering: Mutex<Option<AgentMessage>>,
    follow_up: Mutex<VecDeque<AgentMessage>>,
}

impl MessageInjector {
    pub fn new() -> Self {
        Self {
            steering: Mutex::new(None),
            follow_up: Mutex::new(VecDeque::new()),
        }
    }

    /// Queue a steering message (replaces previous if any).
    pub fn steer(&self, msg: AgentMessage) {
        *self.steering.lock().unwrap() = Some(msg);
    }

    /// Queue a follow-up message (accumulates).
    pub fn follow_up(&self, msg: AgentMessage) {
        self.follow_up.lock().unwrap().push_back(msg);
    }

    /// Consume the steering message (called by run loop).
    pub(crate) fn take_steering(&self) -> Option<AgentMessage> {
        self.steering.lock().unwrap().take()
    }

    /// Consume all follow-up messages (called by run loop).
    pub(crate) fn take_follow_ups(&self) -> Vec<AgentMessage> {
        self.follow_up.lock().unwrap().drain(..).collect()
    }

    /// Whether there are any pending messages.
    pub fn has_pending(&self) -> bool {
        self.steering.lock().unwrap().is_some()
            || !self.follow_up.lock().unwrap().is_empty()
    }
}

impl Default for MessageInjector {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 3: Add `injector` to `AgentState`**

In `crates/alva-agent-core/src/state.rs`:
```rust
use std::sync::Arc;
use crate::injector::MessageInjector;

pub struct AgentState {
    pub model: Arc<dyn LanguageModel>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub session: Arc<dyn AgentSession>,
    pub extensions: Extensions,
    pub injector: Arc<MessageInjector>,
}
```

Update all `AgentState` construction sites (tests, run_child.rs, etc.) to include `injector: Arc::new(MessageInjector::new())`.

**Step 4: Refactor `run_loop` to double loop**

Replace the inner `run_loop` function in `run.rs`:

```rust
async fn run_loop(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError> {
    let mut total_iterations: u32 = 0;

    // Outer loop: processes follow-up messages
    'outer: loop {
        // Inner loop: LLM calls + tool execution + steering checks
        'inner: loop {
            if cancel.is_cancelled() {
                return Err(AgentError::Cancelled);
            }
            if total_iterations >= config.max_iterations {
                tracing::warn!(
                    max_iterations = config.max_iterations,
                    "agent loop exhausted max_iterations without finishing"
                );
                return Err(AgentError::MaxIterations(config.max_iterations));
            }
            total_iterations += 1;

            let _ = event_tx.send(AgentEvent::TurnStart);

            // ... (existing LLM call logic, unchanged) ...

            let tool_calls = extract_tool_calls(&response);

            if tool_calls.is_empty() {
                let _ = event_tx.send(AgentEvent::TurnEnd);
                break 'inner; // Natural finish — check follow-ups
            }

            // Execute tools
            for tool_call in &tool_calls {
                // ... (existing tool execution logic) ...
            }

            let _ = event_tx.send(AgentEvent::TurnEnd);

            // ★ Check steering after tool execution
            if let Some(steering_msg) = state.injector.take_steering() {
                state.session.append(steering_msg);
                continue 'inner; // Process steering with next LLM call
            }
        }

        // ★ Check follow-ups when inner loop ends naturally
        let follow_ups = state.injector.take_follow_ups();
        if follow_ups.is_empty() {
            break 'outer; // Truly done
        }
        for msg in follow_ups {
            state.session.append(msg);
        }
        // Continue outer loop — process follow-ups
    }

    Ok(())
}
```

**Step 5: Run tests**

Run: `cargo test -p alva-agent-core`
Expected: PASS (existing tests + new steering/follow-up tests)

**Step 6: Commit**

```bash
git add crates/alva-agent-core/
git commit -m "feat: MessageInjector + double-loop for steering and follow-up"
```

---

## Task 8: CredentialSource Abstraction

**Files:**
- Create: `crates/alva-types/src/provider/credential.rs`
- Modify: `crates/alva-types/src/provider/mod.rs`
- Modify: `crates/alva-types/src/lib.rs`
- Modify: `crates/alva-provider/src/config.rs`
- Modify: `crates/alva-provider/src/openai.rs`

**Step 1: Create `credential.rs`**

```rust
// crates/alva-types/src/provider/credential.rs

use async_trait::async_trait;
use crate::provider::ProviderError;

/// Abstraction for obtaining API credentials.
///
/// Implementations can be static (simple key), OAuth (refresh token),
/// or vault-backed (fetch from secret manager).
#[async_trait]
pub trait CredentialSource: Send + Sync {
    /// Get the current API key / bearer token.
    async fn get_api_key(&self) -> Result<String, ProviderError>;
}

/// Static credential — wraps a fixed API key string.
/// This is the default for backward compatibility.
#[derive(Clone)]
pub struct StaticCredential(String);

impl StaticCredential {
    pub fn new(key: impl Into<String>) -> Self {
        Self(key.into())
    }
}

#[async_trait]
impl CredentialSource for StaticCredential {
    async fn get_api_key(&self) -> Result<String, ProviderError> {
        Ok(self.0.clone())
    }
}
```

**Step 2: Update `openai.rs` to use CredentialSource**

```rust
pub struct OpenAIProvider {
    credential: Arc<dyn CredentialSource>,
    model: String,
    base_url: String,
    max_tokens: u32,
    client: Client,
}

impl OpenAIProvider {
    pub fn new(config: ProviderConfig) -> Self {
        Self {
            credential: Arc::new(StaticCredential::new(&config.api_key)),
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            client: Client::new(),
        }
    }

    pub fn with_credential(credential: Arc<dyn CredentialSource>, config: ProviderConfig) -> Self {
        Self {
            credential,
            model: config.model,
            base_url: config.base_url,
            max_tokens: config.max_tokens,
            client: Client::new(),
        }
    }
}
```

In `complete()`, replace `self.config.api_key` with:
```rust
let api_key = self.credential.get_api_key().await
    .map_err(|e| AgentError::LlmError(format!("credential error: {}", e)))?;

// ... then use api_key in the Authorization header
.header("Authorization", format!("Bearer {}", api_key))
```

**Step 3: Update re-exports**

In `crates/alva-types/src/lib.rs`:
```rust
pub use provider::{CredentialSource, StaticCredential, Provider, ProviderError, ProviderRegistry};
```

**Step 4: Verify compiles**

Run: `cargo check --workspace`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/alva-types/src/provider/ crates/alva-provider/
git commit -m "feat: CredentialSource abstraction for dynamic API key refresh"
```

---

## Task 9: Update Integration Tests & Remaining References

**Files:**
- Modify: `crates/alva-agent-core/tests/v2_integration.rs`
- Modify: `crates/alva-agent-core/src/run_child.rs`
- Any remaining files with `AgentMessage::Custom`, `ToolResult`, `EmptyToolContext`, `ToolContext` references

**Step 1: Fix all remaining compilation errors**

Run `cargo check --workspace` and fix each error. Common patterns:

- `AgentMessage::Custom { .. }` → `AgentMessage::Extension { .. }`
- `ToolResult { content, is_error, details }` → `ToolOutput::text(content)` or full struct
- `EmptyToolContext` → `MinimalExecutionContext::new()`
- `&dyn ToolContext` → `&dyn ToolExecutionContext`
- `cancel: &CancellationToken` removed from execute signature

**Step 2: Update `make_state()` in all test files to include `injector`**

```rust
fn make_state() -> AgentState {
    AgentState {
        model: Arc::new(EchoModel),
        tools: vec![],
        session: Arc::new(InMemorySession::new()),
        extensions: Extensions::new(),
        injector: Arc::new(MessageInjector::new()),
    }
}
```

**Step 3: Run full test suite**

Run: `cargo test --workspace`
Expected: PASS

**Step 4: Commit**

```bash
git add .
git commit -m "fix: update all tests and remaining references for new type system"
```

---

## Task 10: Add Progress Reporting to ExecuteShellTool

**Files:**
- Modify: `crates/alva-agent-tools/src/execute_shell.rs`

This task demonstrates the progress reporting capability end-to-end.

**Step 1: Update execute_shell to report stdout line by line**

```rust
async fn execute(
    &self,
    input: Value,
    ctx: &dyn ToolExecutionContext,
) -> Result<ToolOutput, AgentError> {
    let params: Input = serde_json::from_value(input)
        .map_err(|e| AgentError::ToolError { tool_name: self.name().into(), message: e.to_string() })?;

    let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
        tool_name: self.name().into(),
        message: "workspace required".into(),
    })?;
    let fallback = LocalToolFs::new(workspace);
    let fs = ctx.tool_fs().unwrap_or(&fallback);

    let timeout_ms = params.timeout_secs.unwrap_or(30) * 1000;
    let cwd = params.cwd.as_deref();

    match fs.exec(&params.command, cwd, timeout_ms).await {
        Ok(result) => {
            // Report stdout lines as progress
            for line in result.stdout.lines() {
                ctx.report_progress(ProgressEvent::StdoutLine {
                    line: line.to_string(),
                });
            }
            for line in result.stderr.lines() {
                ctx.report_progress(ProgressEvent::StderrLine {
                    line: line.to_string(),
                });
            }

            let summary = format!(
                "exit_code: {}, {} stdout lines, {} stderr lines",
                result.exit_code,
                result.stdout.lines().count(),
                result.stderr.lines().count(),
            );

            Ok(ToolOutput {
                content: vec![ToolContent::text(summary)],
                is_error: result.exit_code != 0,
                details: Some(json!({
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                    "exit_code": result.exit_code,
                })),
            })
        }
        Err(AgentError::ToolError { message, .. }) if message.contains("timed out") => {
            Ok(ToolOutput {
                content: vec![ToolContent::text("Command timed out")],
                is_error: true,
                details: Some(json!({ "timed_out": true })),
            })
        }
        Err(e) => Err(AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Failed to execute command: {}", e),
        }),
    }
}
```

**Step 2: Commit**

```bash
git add crates/alva-agent-tools/src/execute_shell.rs
git commit -m "feat: execute_shell reports stdout/stderr progress and separates content/details"
```

---

## Task 11: Final Verification

**Step 1: Full workspace build**

Run: `cargo build --workspace`
Expected: PASS

**Step 2: Full test suite**

Run: `cargo test --workspace`
Expected: PASS

**Step 3: Clippy**

Run: `cargo clippy --workspace -- -D warnings`
Expected: PASS (or only pre-existing warnings)

**Step 4: Final commit if any cleanup needed**

```bash
git add .
git commit -m "chore: final cleanup after runtime communication enhancement"
```
