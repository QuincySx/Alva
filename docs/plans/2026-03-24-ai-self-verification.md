# AI 自验证闭环 Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build infrastructure that lets AI self-verify code changes — Phase A via `cargo test` TDD, Phase B via UI automation with MCP tools.

**Architecture:** Phase A creates `alva-test` crate with MockLanguageModel, MockTool, and fixtures. Then adds L1/L2 tests for alva-core agent loop and srow-core BaseAgent. Phase B extends srow-debug with ActionRegistry + action dispatch + state dump + screenshot endpoints, then wraps them in a thin MCP server (`srow-devtools-mcp`).

**Tech Stack:** Rust, GPUI `#[gpui::test]`, tokio, serde_json, tiny_http, rmcp (MCP SDK), core-graphics (macOS window ID)

**Spec:** `docs/superpowers/specs/2026-03-24-ai-self-verification-design.md`

---

## Phase A: 业务逻辑自验证基建

### Task 1: Create `alva-test` crate scaffold

**Files:**
- Create: `crates/alva-test/Cargo.toml`
- Create: `crates/alva-test/src/lib.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-test"
version = "0.1.0"
edition = "2021"
description = "Shared test utilities: mocks, fixtures, and assertions for the alva framework"

[dependencies]
alva-types = { path = "../alva-types" }
async-trait = "0.1"
serde_json = "1"
tokio = { version = "1", features = ["sync", "rt", "macros"] }
futures = "0.3"
```

**Step 2: Create lib.rs**

```rust
pub mod mock_provider;
pub mod mock_tool;
pub mod fixtures;
pub mod assertions;
```

Create empty module files:
- `crates/alva-test/src/mock_provider.rs` — `// MockLanguageModel`
- `crates/alva-test/src/mock_tool.rs` — `// MockTool`
- `crates/alva-test/src/fixtures.rs` — `// Test data factories`
- `crates/alva-test/src/assertions.rs` — `// Domain assertion helpers`

**Step 3: Add to workspace**

Add `"crates/alva-test"` to workspace members in root `Cargo.toml`.

**Step 4: Verify**

Run: `cargo check -p alva-test`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/alva-test/ Cargo.toml
git commit -m "feat(alva-test): scaffold shared test utilities crate"
```

---

### Task 2: Implement MockLanguageModel

**Files:**
- Modify: `crates/alva-test/src/mock_provider.rs`
- Create: `crates/alva-test/src/mock_provider_test.rs` (inline tests)

The mock must implement `alva_types::LanguageModel` (defined in `crates/alva-types/src/model.rs:18-35`).

**Step 1: Write the failing test**

Add to `crates/alva-test/src/mock_provider.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{Message, MessageRole, ContentBlock, ModelConfig};

    #[tokio::test]
    async fn test_mock_returns_preset_response() {
        let response = Message {
            id: "resp-1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "Hello!".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let mock = MockLanguageModel::new()
            .with_response(response.clone());

        let result = mock.complete(&[], &[], &ModelConfig::default()).await.unwrap();
        assert_eq!(result.content, response.content);
    }

    #[tokio::test]
    async fn test_mock_records_calls() {
        let response = Message {
            id: "resp-1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "ok".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let mock = MockLanguageModel::new()
            .with_response(response);

        let input_msg = Message {
            id: "msg-1".into(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: "hi".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let _ = mock.complete(&[input_msg.clone()], &[], &ModelConfig::default()).await;
        let calls = mock.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].len(), 1);
        assert_eq!(calls[0][0].id, "msg-1");
    }

    #[tokio::test]
    async fn test_mock_sequential_responses() {
        let r1 = Message {
            id: "r1".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "first".into() }],
            tool_call_id: None, usage: None, timestamp: 0,
        };
        let r2 = Message {
            id: "r2".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "second".into() }],
            tool_call_id: None, usage: None, timestamp: 0,
        };
        let mock = MockLanguageModel::new()
            .with_response(r1)
            .with_response(r2);

        let res1 = mock.complete(&[], &[], &ModelConfig::default()).await.unwrap();
        let res2 = mock.complete(&[], &[], &ModelConfig::default()).await.unwrap();
        assert_eq!(res1.content[0], ContentBlock::Text { text: "first".into() });
        assert_eq!(res2.content[0], ContentBlock::Text { text: "second".into() });
    }

    #[tokio::test]
    async fn test_mock_stream_emits_events() {
        use futures::StreamExt;
        use alva_types::StreamEvent;

        let mock = MockLanguageModel::new()
            .with_stream_events(vec![
                StreamEvent::Start,
                StreamEvent::TextDelta { text: "Hello".into() },
                StreamEvent::TextDelta { text: " world".into() },
                StreamEvent::Done,
            ]);

        let mut stream = mock.stream(&[], &[], &ModelConfig::default());
        let events: Vec<_> = stream.collect::<Vec<_>>().await;
        assert_eq!(events.len(), 4);
        assert!(matches!(events[0], StreamEvent::Start));
        assert!(matches!(events[3], StreamEvent::Done));
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p alva-test`
Expected: FAIL — `MockLanguageModel` not defined

**Step 3: Implement MockLanguageModel**

```rust
use std::sync::{Arc, Mutex};
use std::pin::Pin;
use async_trait::async_trait;
use futures::stream;
use alva_types::{
    AgentError, LanguageModel, Message, ModelConfig, StreamEvent, Tool,
};

/// A programmable mock for LanguageModel.
///
/// - `with_response()` queues responses returned by `complete()` in order.
/// - `with_stream_events()` sets events returned by `stream()`.
/// - `calls()` returns recorded input messages for each `complete()` call.
/// - `with_error()` queues an error instead of a response.
pub struct MockLanguageModel {
    responses: Arc<Mutex<Vec<Result<Message, AgentError>>>>,
    stream_events: Arc<Mutex<Vec<StreamEvent>>>,
    recorded_calls: Arc<Mutex<Vec<Vec<Message>>>>,
    call_index: Arc<Mutex<usize>>,
}

impl MockLanguageModel {
    pub fn new() -> Self {
        Self {
            responses: Arc::new(Mutex::new(Vec::new())),
            stream_events: Arc::new(Mutex::new(Vec::new())),
            recorded_calls: Arc::new(Mutex::new(Vec::new())),
            call_index: Arc::new(Mutex::new(0)),
        }
    }

    pub fn with_response(self, msg: Message) -> Self {
        self.responses.lock().unwrap().push(Ok(msg));
        self
    }

    pub fn with_error(self, err: AgentError) -> Self {
        self.responses.lock().unwrap().push(Err(err));
        self
    }

    pub fn with_stream_events(self, events: Vec<StreamEvent>) -> Self {
        *self.stream_events.lock().unwrap() = events;
        self
    }

    /// Returns recorded input messages for each complete() call.
    pub fn calls(&self) -> Vec<Vec<Message>> {
        self.recorded_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl LanguageModel for MockLanguageModel {
    async fn complete(
        &self,
        messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<Message, AgentError> {
        self.recorded_calls.lock().unwrap().push(messages.to_vec());
        let mut idx = self.call_index.lock().unwrap();
        let responses = self.responses.lock().unwrap();
        let result = if *idx < responses.len() {
            match &responses[*idx] {
                Ok(msg) => Ok(msg.clone()),
                Err(e) => Err(AgentError::Model(e.to_string())),
            }
        } else {
            Err(AgentError::Model("no more mock responses".into()))
        };
        *idx += 1;
        result
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        let events = self.stream_events.lock().unwrap().clone();
        Box::pin(stream::iter(events))
    }

    fn model_id(&self) -> &str {
        "mock-model"
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p alva-test`
Expected: PASS (all 4 tests)

**Step 5: Commit**

```bash
git add crates/alva-test/src/mock_provider.rs
git commit -m "feat(alva-test): implement MockLanguageModel with preset responses and call recording"
```

---

### Task 3: Implement MockTool

**Files:**
- Modify: `crates/alva-test/src/mock_tool.rs`

The mock must implement `alva_types::Tool` (defined in `crates/alva-types/src/tool.rs:112-143`).

**Step 1: Write the failing test**

Add to `crates/alva-test/src/mock_tool.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{CancellationToken, EmptyToolContext};

    #[tokio::test]
    async fn test_mock_tool_returns_preset() {
        let tool = MockTool::new("test_tool")
            .with_result(ToolResult { content: "done".into(), is_error: false, details: None });

        let ctx = EmptyToolContext;
        let cancel = CancellationToken::new();
        let result = tool.execute(serde_json::json!({}), &cancel, &ctx).await.unwrap();
        assert_eq!(result.content, "done");
        assert!(!result.is_error);
    }

    #[tokio::test]
    async fn test_mock_tool_records_calls() {
        let tool = MockTool::new("recorder")
            .with_result(ToolResult { content: "ok".into(), is_error: false, details: None });

        let ctx = EmptyToolContext;
        let cancel = CancellationToken::new();
        let input = serde_json::json!({"path": "/tmp/test"});
        let _ = tool.execute(input.clone(), &cancel, &ctx).await;
        let calls = tool.calls();
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0], input);
    }

    #[tokio::test]
    async fn test_mock_tool_error() {
        let tool = MockTool::new("failing")
            .with_error("tool exploded");

        let ctx = EmptyToolContext;
        let cancel = CancellationToken::new();
        let result = tool.execute(serde_json::json!({}), &cancel, &ctx).await;
        assert!(result.is_err());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p alva-test`
Expected: FAIL — `MockTool` not defined

**Step 3: Implement MockTool**

```rust
use std::sync::{Arc, Mutex};
use async_trait::async_trait;
use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use serde_json::Value;

/// A programmable mock for the Tool trait.
///
/// - `with_result()` sets the return value for `execute()`.
/// - `with_error()` makes `execute()` return an error.
/// - `calls()` returns all recorded input arguments.
pub struct MockTool {
    name: String,
    result: Arc<Mutex<Option<Result<ToolResult, AgentError>>>>,
    recorded_calls: Arc<Mutex<Vec<Value>>>,
}

impl MockTool {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            result: Arc::new(Mutex::new(None)),
            recorded_calls: Arc::new(Mutex::new(Vec::new())),
        }
    }

    pub fn with_result(self, result: ToolResult) -> Self {
        *self.result.lock().unwrap() = Some(Ok(result));
        self
    }

    pub fn with_error(self, msg: &str) -> Self {
        *self.result.lock().unwrap() = Some(Err(AgentError::Tool(msg.to_string())));
        self
    }

    pub fn calls(&self) -> Vec<Value> {
        self.recorded_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl Tool for MockTool {
    fn name(&self) -> &str { &self.name }
    fn description(&self) -> &str { "Mock tool for testing" }
    fn parameters_schema(&self) -> Value { serde_json::json!({"type": "object"}) }

    async fn execute(
        &self,
        input: Value,
        _cancel: &CancellationToken,
        _ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        self.recorded_calls.lock().unwrap().push(input);
        self.result.lock().unwrap()
            .clone()
            .unwrap_or(Ok(ToolResult { content: String::new(), is_error: false, details: None }))
    }
}
```

**Step 4: Run tests**

Run: `cargo test -p alva-test`
Expected: PASS (all 7 tests — 4 provider + 3 tool)

**Step 5: Commit**

```bash
git add crates/alva-test/src/mock_tool.rs
git commit -m "feat(alva-test): implement MockTool with preset results and call recording"
```

---

### Task 4: Implement test fixtures

**Files:**
- Modify: `crates/alva-test/src/fixtures.rs`

**Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::MessageRole;

    #[test]
    fn test_make_user_message() {
        let msg = make_user_message("hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn test_make_assistant_message() {
        let msg = make_assistant_message("hi there");
        assert_eq!(msg.role, MessageRole::Assistant);
    }

    #[test]
    fn test_make_tool_call_message() {
        let msg = make_tool_call_message("read_file", serde_json::json!({"path": "/tmp"}));
        assert_eq!(msg.role, MessageRole::Assistant);
        assert!(!msg.content.is_empty());
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p alva-test -- fixtures`
Expected: FAIL

**Step 3: Implement fixtures**

```rust
use alva_types::{Message, MessageRole, ContentBlock};
use serde_json::Value;

/// Create a user message with text content.
pub fn make_user_message(text: &str) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::User,
        content: vec![ContentBlock::Text { text: text.into() }],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}

/// Create an assistant message with text content.
pub fn make_assistant_message(text: &str) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::Text { text: text.into() }],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}

/// Create an assistant message with a tool call.
pub fn make_tool_call_message(tool_name: &str, args: Value) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: uuid_str(),
            name: tool_name.into(),
            input: args,
        }],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    }
}

fn uuid_str() -> String {
    uuid::Uuid::new_v4().to_string()
}
```

Add `uuid` and `chrono` to `alva-test/Cargo.toml` dependencies:
```toml
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
```

**Step 4: Run tests**

Run: `cargo test -p alva-test -- fixtures`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/alva-test/
git commit -m "feat(alva-test): add test fixture factory functions"
```

---

### Task 5: Add alva-test as dev-dependency to alva-core, write agent loop tests

**Files:**
- Modify: `crates/alva-core/Cargo.toml` (add `[dev-dependencies]`)
- Modify: `crates/alva-core/src/agent_loop.rs` (replace inline MockModel with alva-test mocks)

The agent loop already has 5 tests at `crates/alva-core/src/agent_loop.rs:417-804`. This task:
1. Adds `alva-test` as dev-dependency
2. Migrates existing tests to use `alva-test` mocks (verifying they still pass)
3. Adds new tests: tool execution flow, multi-turn conversation, error handling

**Step 1: Add dev-dependency**

In `crates/alva-core/Cargo.toml`, add:
```toml
[dev-dependencies]
alva-test = { path = "../alva-test" }
tokio = { version = "1", features = ["rt", "macros"] }
```

**Step 2: Verify existing tests still pass**

Run: `cargo test -p alva-core`
Expected: PASS (existing 5 tests)

**Step 3: Add new agent loop test — tool execution flow**

In the existing `#[cfg(test)] mod tests` in `agent_loop.rs`, add:

```rust
#[tokio::test]
async fn test_tool_execution_flow() {
    use alva_test::mock_provider::MockLanguageModel;
    use alva_test::mock_tool::MockTool;
    use alva_test::fixtures::*;

    // First LLM call returns a tool call, second returns text (no more tools)
    let tool_call_response = make_tool_call_message("read_file", serde_json::json!({"path": "/tmp"}));
    let final_response = make_assistant_message("File contents: hello");

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(tool_call_response)
            .with_response(final_response)
    );

    let tool = Arc::new(
        MockTool::new("read_file")
            .with_result(ToolResult {
                content: "hello".into(),
                is_error: false,
                details: None,
            })
    );

    let hooks = AgentHooks::default();
    let cancel = CancellationToken::new();
    let (event_tx, mut event_rx) = mpsc::unbounded_channel();
    let mut state = AgentState::new();
    state.messages.push(AgentMessage::Standard(make_user_message("read /tmp")));

    // Register tool
    let tools: Vec<Arc<dyn Tool>> = vec![tool.clone()];

    run_agent_loop(&mut state, model.as_ref(), &hooks, &cancel, &event_tx).await;

    // Verify tool was called
    let tool_calls = tool.calls();
    assert_eq!(tool_calls.len(), 1);
    assert_eq!(tool_calls[0]["path"], "/tmp");

    // Verify model was called twice (tool call + final)
    assert_eq!(model.calls().len(), 2);
}
```

Note: This test may need adjustment based on how `run_agent_loop` receives tools. Check the actual function signature and adapt — tools might be in AgentState or passed via AgentHooks. Read the exact code before implementing.

**Step 4: Run tests**

Run: `cargo test -p alva-core`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/alva-core/
git commit -m "test(alva-core): add tool execution flow test using alva-test mocks"
```

---

### Task 6: Add tests for srow-core BaseAgent

**Files:**
- Modify: `crates/srow-core/Cargo.toml` (add alva-test to dev-dependencies)
- Modify: `crates/srow-core/src/base_agent.rs` (add tests)

BaseAgent is defined at `crates/srow-core/src/base_agent.rs:45-99`. It wraps `Agent` with tool registry, skills, MCP, and memory. The existing tests (lines 350-385) test basic builder config.

**Step 1: Add dev-dependency**

In `crates/srow-core/Cargo.toml` `[dev-dependencies]`, add:
```toml
alva-test = { path = "../alva-test" }
```

**Step 2: Write test — BaseAgent prompt produces events**

Add to existing test module in `base_agent.rs`:

```rust
#[tokio::test]
async fn test_base_agent_prompt_produces_events() {
    use alva_test::mock_provider::MockLanguageModel;
    use alva_test::fixtures::make_assistant_message;

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_assistant_message("Hello from mock!"))
    );

    let agent = BaseAgent::builder()
        .system_prompt("You are a test agent.")
        .build(model)
        .await
        .expect("build should succeed");

    let mut rx = agent.prompt_text("hi");

    let mut got_agent_end = false;
    while let Some(event) = rx.recv().await {
        if matches!(event, AgentEvent::AgentEnd { .. }) {
            got_agent_end = true;
            break;
        }
    }
    assert!(got_agent_end, "should receive AgentEnd event");
}
```

**Step 3: Run test to verify**

Run: `cargo test -p srow-core -- test_base_agent_prompt`
Expected: PASS

**Step 4: Write test — BaseAgent with custom tool**

```rust
#[tokio::test]
async fn test_base_agent_registers_custom_tool() {
    use alva_test::mock_provider::MockLanguageModel;
    use alva_test::mock_tool::MockTool;
    use alva_test::fixtures::*;

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_tool_call_message("my_tool", serde_json::json!({"x": 1})))
            .with_response(make_assistant_message("Done."))
    );

    let mock_tool = Arc::new(
        MockTool::new("my_tool")
            .with_result(ToolResult { content: "ok".into(), is_error: false, details: None })
    );

    let agent = BaseAgent::builder()
        .system_prompt("test")
        .tool(mock_tool.clone())
        .build(model)
        .await
        .expect("build should succeed");

    let mut rx = agent.prompt_text("use my_tool");
    while let Some(_event) = rx.recv().await {}

    assert_eq!(mock_tool.calls().len(), 1);
}
```

**Step 5: Run all srow-core tests**

Run: `cargo test -p srow-core`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/srow-core/
git commit -m "test(srow-core): add BaseAgent integration tests using alva-test mocks"
```

---

## Phase B: UI 自动化

### Task 7: Add ActionRegistry to srow-debug

**Files:**
- Create: `crates/srow-debug/src/action_registry.rs`
- Modify: `crates/srow-debug/src/lib.rs` (re-export)

**Step 1: Write test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_and_list_views() {
        let registry = ActionRegistry::new();
        registry.register("test_view", RegisteredView {
            action_fn: Box::new(|_method, _args, _cx| Ok(serde_json::Value::Null)),
            state_fn: Box::new(|_cx| Some(serde_json::json!({"count": 0}))),
            methods: vec!["do_thing".into()],
        });
        let views = registry.list_views();
        assert_eq!(views.len(), 1);
        assert_eq!(views[0].0, "test_view");
        assert_eq!(views[0].1, vec!["do_thing"]);
    }

    #[test]
    fn test_unregister_view() {
        let registry = ActionRegistry::new();
        registry.register("v1", RegisteredView {
            action_fn: Box::new(|_, _, _| Ok(serde_json::Value::Null)),
            state_fn: Box::new(|_| None),
            methods: vec![],
        });
        assert_eq!(registry.list_views().len(), 1);
        registry.unregister("v1");
        assert_eq!(registry.list_views().len(), 0);
    }
}
```

**Step 2: Run to verify fail**

Run: `cargo test -p srow-debug -- action_registry`
Expected: FAIL

**Step 3: Implement ActionRegistry**

```rust
use parking_lot::RwLock;
use std::collections::HashMap;
use serde_json::Value;

/// Type-erased action closure: receives method + args, executes on GPUI thread.
/// The `&mut gpui::App` parameter is only available when GPUI is in scope.
/// For the registry definition itself, we use a generic callback signature.
pub type ActionFn = Box<dyn Fn(&str, Value) -> Result<Value, String> + Send + Sync>;

/// Type-erased state closure: reads component state.
pub type StateFn = Box<dyn Fn() -> Option<Value> + Send + Sync>;

pub struct RegisteredView {
    pub action_fn: ActionFn,
    pub state_fn: StateFn,
    pub methods: Vec<String>,
}

pub struct ActionRegistry {
    views: RwLock<HashMap<String, RegisteredView>>,
}

impl ActionRegistry {
    pub fn new() -> Self {
        Self {
            views: RwLock::new(HashMap::new()),
        }
    }

    pub fn register(&self, id: &str, view: RegisteredView) {
        self.views.write().insert(id.to_string(), view);
    }

    pub fn unregister(&self, id: &str) {
        self.views.write().remove(id);
    }

    /// Returns (view_id, methods) for all registered views.
    pub fn list_views(&self) -> Vec<(String, Vec<String>)> {
        self.views.read().iter().map(|(k, v)| {
            (k.clone(), v.methods.clone())
        }).collect()
    }

    pub fn dispatch(&self, target: &str, method: &str, args: Value) -> Result<Value, String> {
        let views = self.views.read();
        let view = views.get(target)
            .ok_or_else(|| format!("target '{}' not registered or dropped", target))?;
        if !view.methods.contains(&method.to_string()) {
            return Err(format!("method '{}' not found on '{}'", method, target));
        }
        (view.action_fn)(method, args)
    }

    pub fn get_state(&self, target: &str) -> Result<Value, String> {
        let views = self.views.read();
        let view = views.get(target)
            .ok_or_else(|| format!("target '{}' not registered or dropped", target))?;
        (view.state_fn)().ok_or_else(|| format!("entity '{}' has been dropped", target))
    }
}
```

Note: The actual action/state closures will capture GPUI WeakEntity internally and use `gpui::App` when called from the GPUI drain task. The closures shown in tests are simplified. The real integration (Task 9) will wire them through the mpsc channel to the GPUI thread.

**Step 4: Run tests**

Run: `cargo test -p srow-debug -- action_registry`
Expected: PASS

**Step 5: Add to lib.rs re-exports**

Add `pub mod action_registry;` and `pub use action_registry::ActionRegistry;` to `crates/srow-debug/src/lib.rs`.

**Step 6: Commit**

```bash
git add crates/srow-debug/
git commit -m "feat(srow-debug): add ActionRegistry for type-erased view dispatch"
```

---

### Task 8: Add DebugState trait and new HTTP endpoints to srow-debug

**Files:**
- Modify: `crates/srow-debug/src/inspect.rs` (add DebugState trait)
- Modify: `crates/srow-debug/src/router.rs` (add /api/action, /api/inspect/state, /api/inspect/views, /api/screenshot, /api/shutdown endpoints)
- Modify: `crates/srow-debug/src/builder.rs` (accept ActionRegistry)

**Step 1: Add DebugState trait**

In `crates/srow-debug/src/inspect.rs`:

```rust
/// Trait for components to expose runtime state for AI verification.
pub trait DebugState {
    fn debug_state(&self) -> serde_json::Value;
}
```

**Step 2: Update DebugServerBuilder to accept ActionRegistry**

In `builder.rs`, add `action_registry: Option<Arc<ActionRegistry>>` field and builder method `with_action_registry()`. Pass it through to `DebugServer` and then to `Router`.

**Step 3: Add new routes to Router**

In `router.rs`, extend the `handle()` match:

```rust
("POST", "/api/action") => self.handle_action(request),
("GET", "/api/inspect/state") => self.handle_inspect_state(url),
("GET", "/api/inspect/views") => self.handle_inspect_views(),
("POST", "/api/screenshot") => self.handle_screenshot(),
("POST", "/api/shutdown") => self.handle_shutdown(),
```

Each handler:
- `/api/action`: Parse JSON body `{target, method, args}`, call `registry.dispatch()`, return result or error JSON
- `/api/inspect/state`: Parse `?view=X` query param, call `registry.get_state()`, return state JSON
- `/api/inspect/views`: Call `registry.list_views()`, return JSON array of `{id, methods}`
- `/api/screenshot`: Get window ID via `core-graphics`, run `screencapture`, return `{path}`
- `/api/shutdown`: Send shutdown signal via stored channel

**Step 4: Write integration test**

In `crates/srow-debug/tests/integration.rs`, add:

```rust
#[test]
fn test_action_endpoint_returns_result() {
    // Build server with ActionRegistry containing a test view
    // POST /api/action with valid target/method
    // Assert 200 + ok: true
}

#[test]
fn test_action_endpoint_unknown_target() {
    // POST /api/action with unknown target
    // Assert error response with target_not_found
}

#[test]
fn test_inspect_state_endpoint() {
    // GET /api/inspect/state?view=test_view
    // Assert returns state JSON
}

#[test]
fn test_inspect_views_endpoint() {
    // GET /api/inspect/views
    // Assert returns list of registered views with methods
}
```

**Step 5: Run tests**

Run: `cargo test -p srow-debug`
Expected: PASS

**Step 6: Commit**

```bash
git add crates/srow-debug/
git commit -m "feat(srow-debug): add action dispatch, state dump, views, screenshot, shutdown endpoints"
```

---

### Task 9: Wire ActionRegistry into srow-app with GPUI drain task

**Files:**
- Modify: `crates/srow-app/src/main.rs` (create ActionRegistry, spawn drain task)
- Modify: `crates/srow-app/src/chat/gpui_chat.rs` (register in ActionRegistry, implement DebugState)
- Modify: `crates/srow-app/src/lib.rs` (export DebugActionRegistry global)

This is the hardest task — wiring the ActionRegistry through a persistent mpsc channel to the GPUI main thread.

**Step 1: Define the command channel types**

In `crates/srow-app/src/lib.rs`:

```rust
#[cfg(debug_assertions)]
pub struct DebugActionRegistry(pub std::sync::Arc<srow_debug::ActionRegistry>);
#[cfg(debug_assertions)]
impl gpui::Global for DebugActionRegistry {}
```

**Step 2: In main.rs, create ActionRegistry and spawn GPUI drain task**

After `cx.set_global(SharedRuntime(...))`:

```rust
#[cfg(debug_assertions)]
{
    let action_registry = srow_debug::ActionRegistry::new();
    let registry_arc = std::sync::Arc::new(action_registry);
    cx.set_global(srow_app::DebugActionRegistry(registry_arc.clone()));

    // Pass registry to debug server builder
    // server = srow_debug::DebugServer::builder()
    //     ...
    //     .with_action_registry(registry_arc)
    //     ...
}
```

**Step 3: Register GpuiChat in ActionRegistry**

In `GpuiChat::new()`, after entity creation:

```rust
#[cfg(debug_assertions)]
{
    if let Some(registry) = cx.try_global::<crate::DebugActionRegistry>() {
        let weak = cx.entity().downgrade();
        let weak2 = weak.clone();
        registry.0.register("chat_panel", srow_debug::action_registry::RegisteredView {
            action_fn: Box::new(move |method, args| {
                // This will be called from the GPUI drain task context
                // For now: simplified direct dispatch
                match method {
                    "send_message" => Ok(serde_json::Value::Null),
                    _ => Err(format!("unknown method: {method}")),
                }
            }),
            state_fn: Box::new(move || {
                // Simplified — real version needs GPUI context
                Some(serde_json::json!({"registered": true}))
            }),
            methods: vec!["send_message".into()],
        });
    }
}
```

Note: The full GPUI-threaded dispatch (with mpsc channel + oneshot response) should be built incrementally. Start with a simplified version that registers the view but dispatches synchronously. Then iterate to add the async channel-based dispatch.

**Step 4: Verify app still compiles and debug server starts**

Run: `cargo build -p srow-app`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/srow-app/ crates/srow-debug/
git commit -m "feat(srow-app): wire ActionRegistry into GPUI with component registration"
```

---

### Task 10: Implement srow-devtools-mcp

**Files:**
- Create: `crates/srow-devtools-mcp/Cargo.toml`
- Create: `crates/srow-devtools-mcp/src/lib.rs`
- Create: `crates/srow-devtools-mcp/src/main.rs`
- Create: `crates/srow-devtools-mcp/src/tools.rs`
- Modify: `Cargo.toml` (workspace members)

**Step 1: Create crate scaffold**

`Cargo.toml`:
```toml
[package]
name = "srow-devtools-mcp"
version = "0.1.0"
edition = "2021"
description = "MCP server that wraps srow-debug HTTP API for AI-driven development"

[[bin]]
name = "srow-devtools-mcp"
path = "src/main.rs"

[dependencies]
rmcp = { version = "0.1", features = ["server", "transport-io"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["full"] }
reqwest = { version = "0.12", features = ["json"] }
```

Note: Check actual `rmcp` crate availability and version on crates.io. If unavailable, implement MCP protocol manually over stdin/stdout JSON-RPC.

**Step 2: Implement MCP tools**

Each tool maps to a srow-debug HTTP endpoint. The MCP server reads from stdin, parses JSON-RPC, dispatches to the right HTTP call, and writes the response to stdout.

Tools:
- `srow_views` → `GET http://127.0.0.1:9229/api/inspect/views`
- `srow_inspect` (args: `{view: string}`) → `GET http://127.0.0.1:9229/api/inspect/state?view=X`
- `srow_action` (args: `{target, method, args}`) → `POST http://127.0.0.1:9229/api/action`
- `srow_screenshot` → `POST http://127.0.0.1:9229/api/screenshot`
- `srow_shutdown` → `POST http://127.0.0.1:9229/api/shutdown`

**Step 3: Add to workspace**

Add `"crates/srow-devtools-mcp"` to root `Cargo.toml`.

**Step 4: Verify build**

Run: `cargo build -p srow-devtools-mcp`
Expected: PASS

**Step 5: Commit**

```bash
git add crates/srow-devtools-mcp/ Cargo.toml
git commit -m "feat(srow-devtools-mcp): MCP server wrapping srow-debug HTTP API"
```

---

### Task 11: End-to-end verification

**No new files.** This task verifies the complete loop works.

**Step 1: Run all tests**

Run: `cargo test --workspace`
Expected: PASS (except srow-app known issue)

**Step 2: Manual E2E — Phase A**

Run: `cargo test -p alva-core -p srow-core`
Expected: All tests pass, including new alva-test-based tests

**Step 3: Manual E2E — Phase B**

```bash
# Terminal 1: Start app
cargo run -p srow-app &

# Terminal 2: Test endpoints
curl http://127.0.0.1:9229/api/health
curl http://127.0.0.1:9229/api/inspect/views
curl http://127.0.0.1:9229/api/inspect/state?view=chat_panel
curl -X POST http://127.0.0.1:9229/api/screenshot
curl -X POST http://127.0.0.1:9229/api/shutdown
```

Expected: All return valid JSON responses.

**Step 4: Commit final state**

```bash
git commit --allow-empty -m "milestone: AI self-verification loop Phase A+B complete"
```
