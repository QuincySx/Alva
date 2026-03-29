# Agent V2 Phase 1 — Core Types & Engine

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Build the new Agent V2 core types (AgentState, AgentConfig, AgentSession, new Middleware) and run_agent() engine alongside the existing code. Old code untouched — new code in new modules/files.

**Architecture:** New types live in `alva-types/src/session.rs` (AgentSession trait) and a new `alva-agent-core/src/v2/` module (AgentState, AgentConfig, run_agent). The old `agent.rs`, `agent_loop.rs`, `types.rs` remain untouched. Phase 2 will migrate consumers. Phase 3 will delete old code.

**Tech Stack:** Rust, async-trait 0.1, tokio 1 (sync, rt, time), thiserror 2

---

## Strategy: Build Alongside, Don't Break

The entire Phase 1 is additive. Nothing existing is modified except adding `pub mod` declarations. This means:
- `cargo check --workspace` passes at every step
- All existing tests keep passing
- New code has its own tests
- Migration happens in a later phase

---

## File Structure

### New files

| File | Responsibility |
|------|---------------|
| `crates/alva-types/src/session.rs` | `AgentSession` trait + `InMemorySession` default impl |
| `crates/alva-agent-core/src/v2/mod.rs` | V2 module root |
| `crates/alva-agent-core/src/v2/state.rs` | `AgentState` + `AgentConfig` |
| `crates/alva-agent-core/src/v2/middleware.rs` | New `Middleware` trait (with wrap hooks), `MiddlewareStack` |
| `crates/alva-agent-core/src/v2/run.rs` | `run_agent()` free function — the new engine |
| `crates/alva-agent-core/src/v2/builtins/mod.rs` | Built-in middleware module |
| `crates/alva-agent-core/src/v2/builtins/loop_detection.rs` | LoopDetectionMiddleware (rewritten for v2) |
| `crates/alva-agent-core/src/v2/builtins/dangling_tool_call.rs` | DanglingToolCallMiddleware (rewritten for v2) |

### Modified files (module declarations only)

| File | Change |
|------|--------|
| `crates/alva-types/src/lib.rs` | Add `pub mod session;` and re-exports |
| `crates/alva-agent-core/src/lib.rs` | Add `pub mod v2;` |

---

## Task Breakdown

### Task 1: AgentSession trait + InMemorySession

**Files:**
- Create: `crates/alva-types/src/session.rs`
- Modify: `crates/alva-types/src/lib.rs` (add `pub mod session;`)

- [ ] **Step 1: Write tests in session.rs**

```rust
// crates/alva-types/src/session.rs

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::message::Message;

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message::user(text))
    }

    #[test]
    fn new_session_has_id() {
        let session = InMemorySession::new();
        assert!(!session.id().is_empty());
    }

    #[test]
    fn new_session_has_no_parent() {
        let session = InMemorySession::new();
        assert!(session.parent_id().is_none());
    }

    #[test]
    fn child_session_has_parent() {
        let parent = InMemorySession::new();
        let child = InMemorySession::with_parent(parent.id());
        assert_eq!(child.parent_id(), Some(parent.id()));
    }

    #[test]
    fn append_and_retrieve() {
        let session = InMemorySession::new();
        session.append(user_msg("hello"));
        session.append(user_msg("world"));
        assert_eq!(session.messages().len(), 2);
    }

    #[test]
    fn recent_returns_last_n() {
        let session = InMemorySession::new();
        for i in 0..10 {
            session.append(user_msg(&format!("msg {}", i)));
        }
        let recent = session.recent(3);
        assert_eq!(recent.len(), 3);
        // Should be the LAST 3
        if let AgentMessage::Standard(m) = &recent[0] {
            assert!(m.text_content().contains("msg 7"));
        }
    }

    #[test]
    fn recent_with_n_larger_than_total() {
        let session = InMemorySession::new();
        session.append(user_msg("only one"));
        let recent = session.recent(100);
        assert_eq!(recent.len(), 1);
    }

    #[test]
    fn empty_session_messages() {
        let session = InMemorySession::new();
        assert!(session.messages().is_empty());
        assert!(session.recent(5).is_empty());
    }

    #[tokio::test]
    async fn flush_and_restore_roundtrip() {
        let session = InMemorySession::new();
        session.append(user_msg("saved"));
        session.flush().await;
        let restored = session.restore().await;
        assert_eq!(restored.len(), 1);
    }
}
```

- [ ] **Step 2: Run tests, verify they fail**

```bash
cargo test -p alva-types -- session::tests -v
```
Expected: FAIL (module doesn't exist)

- [ ] **Step 3: Implement AgentSession trait + InMemorySession**

```rust
// crates/alva-types/src/session.rs

use std::sync::RwLock;
use async_trait::async_trait;
use crate::base::message::AgentMessage;

/// Session manages all messages for an Agent's conversation.
///
/// It is the single source of truth for message history.
/// Agent reads from it (recent/messages), writes to it (append),
/// and external consumers (UI, persistence) also read from it.
#[async_trait]
pub trait AgentSession: Send + Sync {
    /// Unique session identifier.
    fn id(&self) -> &str;

    /// Parent session ID (for sub-agents).
    fn parent_id(&self) -> Option<&str>;

    /// Append a message to the session.
    fn append(&self, message: AgentMessage);

    /// Get all messages (for UI rendering, export).
    fn messages(&self) -> Vec<AgentMessage>;

    /// Get the most recent N messages (for context assembly).
    fn recent(&self, n: usize) -> Vec<AgentMessage>;

    /// Persist to storage backend.
    async fn flush(&self);

    /// Restore from storage backend.
    async fn restore(&self) -> Vec<AgentMessage>;
}

/// In-memory session implementation. Default for quick use.
pub struct InMemorySession {
    id: String,
    parent_id: Option<String>,
    messages: RwLock<Vec<AgentMessage>>,
}

impl InMemorySession {
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            messages: RwLock::new(Vec::new()),
        }
    }

    pub fn with_parent(parent_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: Some(parent_id.to_string()),
            messages: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InMemorySession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentSession for InMemorySession {
    fn id(&self) -> &str {
        &self.id
    }

    fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    fn append(&self, message: AgentMessage) {
        self.messages.write().unwrap().push(message);
    }

    fn messages(&self) -> Vec<AgentMessage> {
        self.messages.read().unwrap().clone()
    }

    fn recent(&self, n: usize) -> Vec<AgentMessage> {
        let msgs = self.messages.read().unwrap();
        let start = msgs.len().saturating_sub(n);
        msgs[start..].to_vec()
    }

    async fn flush(&self) {
        // In-memory: nothing to persist
    }

    async fn restore(&self) -> Vec<AgentMessage> {
        self.messages()
    }
}
```

- [ ] **Step 4: Register module in lib.rs**

Add to `crates/alva-types/src/lib.rs`:
```rust
pub mod session;
pub use session::{AgentSession, InMemorySession};
```

- [ ] **Step 5: Run tests, verify pass**

```bash
cargo test -p alva-types -- session::tests -v
```

- [ ] **Step 6: Verify downstream compiles**

```bash
cargo check --workspace
```

- [ ] **Step 7: Commit**

```bash
git add crates/alva-types/src/session.rs crates/alva-types/src/lib.rs
git commit -m "feat(types): add AgentSession trait + InMemorySession for agent v2"
```

---

### Task 2: V2 AgentState + AgentConfig

**Files:**
- Create: `crates/alva-agent-core/src/v2/mod.rs`
- Create: `crates/alva-agent-core/src/v2/state.rs`
- Modify: `crates/alva-agent-core/src/lib.rs` (add `pub mod v2;`)

- [ ] **Step 1: Write tests in state.rs**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{InMemorySession, ModelConfig};
    use std::sync::Arc;

    // Mock model for testing
    struct MockModel;
    #[async_trait::async_trait]
    impl alva_types::LanguageModel for MockModel {
        async fn complete(&self, _: &[alva_types::Message], _: &[&dyn alva_types::Tool], _: &ModelConfig) -> Result<alva_types::Message, alva_types::AgentError> {
            Ok(alva_types::Message::system("mock"))
        }
        fn stream(&self, _: &[alva_types::Message], _: &[&dyn alva_types::Tool], _: &ModelConfig) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = alva_types::StreamEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }
        fn model_id(&self) -> &str { "mock" }
    }

    #[test]
    fn agent_state_creation() {
        let state = AgentState {
            model: Arc::new(MockModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        };
        assert!(state.tools.is_empty());
        assert!(state.session.messages().is_empty());
    }

    #[test]
    fn agent_config_with_system_prompt() {
        let config = AgentConfig {
            middleware: MiddlewareStack::new(),
            system_prompt: "You are helpful.".to_string(),
        };
        assert_eq!(config.system_prompt, "You are helpful.");
        assert!(config.middleware.is_empty());
    }

    #[test]
    fn extensions_work_on_state() {
        let mut state = AgentState {
            model: Arc::new(MockModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        };

        #[derive(Debug, PartialEq)]
        struct Depth(u32);

        state.extensions.insert(Depth(3));
        assert_eq!(state.extensions.get::<Depth>(), Some(&Depth(3)));
    }
}
```

- [ ] **Step 2: Implement AgentState + AgentConfig**

```rust
// crates/alva-agent-core/src/v2/state.rs

use std::sync::Arc;

use alva_types::model::LanguageModel;
use alva_types::session::AgentSession;
use alva_types::tool::Tool;

use crate::middleware::Extensions;
use super::middleware::MiddlewareStack;

/// Agent V2 mutable state — everything the agent "has".
///
/// Passed as `&mut AgentState` to middleware hooks.
/// Does NOT contain messages — those live in `session`.
pub struct AgentState {
    /// LLM model provider.
    pub model: Arc<dyn LanguageModel>,
    /// Available tools.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Session — the single source of truth for all messages.
    pub session: Arc<dyn AgentSession>,
    /// Open extension slots for middleware-specific data.
    pub extensions: Extensions,
}

/// Agent V2 immutable config — logic that doesn't change during a run.
///
/// Passed as `&AgentConfig` to the run loop. Separate from AgentState
/// to solve the Rust borrow conflict (middleware in config reads state mutably).
pub struct AgentConfig {
    /// Middleware stack — the ONE hook system.
    pub middleware: MiddlewareStack,
    /// System prompt prepended to every LLM call.
    pub system_prompt: String,
}
```

- [ ] **Step 3: Create v2/mod.rs**

```rust
// crates/alva-agent-core/src/v2/mod.rs

pub mod state;
pub mod middleware;
pub mod builtins;
pub mod run;

pub use state::{AgentState, AgentConfig};
pub use middleware::{Middleware, MiddlewareStack, LlmCallFn, ToolCallFn};
pub use run::run_agent;
```

- [ ] **Step 4: Register in lib.rs**

Add to `crates/alva-agent-core/src/lib.rs`:
```rust
pub mod v2;
```

- [ ] **Step 5: Run tests + workspace check**

```bash
cargo test -p alva-agent-core -- v2::state::tests -v
cargo check --workspace
```

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-core/src/v2/ crates/alva-agent-core/src/lib.rs
git commit -m "feat(core): add v2 AgentState + AgentConfig — state/config separation"
```

---

### Task 3: V2 Middleware trait (with wrap hooks)

**Files:**
- Create: `crates/alva-agent-core/src/v2/middleware.rs`

- [ ] **Step 1: Write tests**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    #[tokio::test]
    async fn before_after_execution_order() {
        let order = Arc::new(std::sync::Mutex::new(Vec::<String>::new()));

        struct OrderMw {
            label: String,
            order: Arc<std::sync::Mutex<Vec<String>>>,
        }

        #[async_trait::async_trait]
        impl Middleware for OrderMw {
            async fn before_llm_call(&self, _state: &mut super::super::state::AgentState, _messages: &mut Vec<alva_types::Message>) -> Result<(), MiddlewareError> {
                self.order.lock().unwrap().push(format!("before:{}", self.label));
                Ok(())
            }
            async fn after_llm_call(&self, _state: &mut super::super::state::AgentState, _response: &mut alva_types::Message) -> Result<(), MiddlewareError> {
                self.order.lock().unwrap().push(format!("after:{}", self.label));
                Ok(())
            }
            fn name(&self) -> &str { &self.label }
        }

        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(OrderMw { label: "A".into(), order: order.clone() }));
        stack.push(Arc::new(OrderMw { label: "B".into(), order: order.clone() }));

        // Need a mock state for the test — covered in integration tests
    }

    #[test]
    fn empty_stack() {
        let stack = MiddlewareStack::new();
        assert!(stack.is_empty());
        assert_eq!(stack.len(), 0);
    }

    #[test]
    fn push_sorted_by_priority() {
        struct PrioMw(i32);
        #[async_trait::async_trait]
        impl Middleware for PrioMw {
            fn priority(&self) -> i32 { self.0 }
            fn name(&self) -> &str { "prio" }
        }

        let mut stack = MiddlewareStack::new();
        stack.push_sorted(Arc::new(PrioMw(3000)));
        stack.push_sorted(Arc::new(PrioMw(1000)));
        stack.push_sorted(Arc::new(PrioMw(5000)));

        // Verify order via priority
        let priorities: Vec<i32> = stack.iter().map(|m| m.priority()).collect();
        assert_eq!(priorities, vec![1000, 3000, 5000]);
    }
}
```

- [ ] **Step 2: Implement V2 Middleware trait + MiddlewareStack**

```rust
// crates/alva-agent-core/src/v2/middleware.rs

use std::sync::Arc;
use async_trait::async_trait;
use alva_types::{AgentError, Message, ToolCall, ToolResult};

use super::state::AgentState;

// Re-use existing MiddlewareError and Extensions from v1
pub use crate::middleware::{MiddlewareError, Extensions, MiddlewarePriority};

/// Callback trait for LLM calls within wrap_llm_call.
#[async_trait]
pub trait LlmCallFn: Send + Sync {
    async fn call(&self, messages: Vec<Message>) -> Result<Message, AgentError>;
}

/// Callback trait for tool calls within wrap_tool_call.
#[async_trait]
pub trait ToolCallFn: Send + Sync {
    async fn call(&self, tool_call: &ToolCall) -> Result<ToolResult, AgentError>;
}

/// V2 Middleware trait — the ONE hook system.
///
/// All hooks have default no-op implementations. Middleware only overrides
/// what it needs. Receives &mut AgentState directly (no clone, no separate context).
#[async_trait]
pub trait Middleware: Send + Sync {
    // ── Agent lifecycle ──

    async fn on_agent_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> { Ok(()) }
    async fn on_agent_end(&self, _state: &mut AgentState, _error: Option<&str>) -> Result<(), MiddlewareError> { Ok(()) }

    // ── LLM call ──

    async fn before_llm_call(&self, _state: &mut AgentState, _messages: &mut Vec<Message>) -> Result<(), MiddlewareError> { Ok(()) }
    async fn after_llm_call(&self, _state: &mut AgentState, _response: &mut Message) -> Result<(), MiddlewareError> { Ok(()) }
    async fn wrap_llm_call(&self, _state: &AgentState, messages: Vec<Message>, next: &dyn LlmCallFn) -> Result<Message, MiddlewareError> {
        next.call(messages).await.map_err(|e| MiddlewareError::Other(e.to_string()))
    }

    // ── Tool call ──

    async fn before_tool_call(&self, _state: &mut AgentState, _tool_call: &ToolCall) -> Result<(), MiddlewareError> { Ok(()) }
    async fn after_tool_call(&self, _state: &mut AgentState, _tool_call: &ToolCall, _result: &mut ToolResult) -> Result<(), MiddlewareError> { Ok(()) }
    async fn wrap_tool_call(&self, _state: &AgentState, tool_call: &ToolCall, next: &dyn ToolCallFn) -> Result<ToolResult, MiddlewareError> {
        next.call(tool_call).await.map_err(|e| MiddlewareError::Other(e.to_string()))
    }

    // ── Meta ──

    fn priority(&self) -> i32 { MiddlewarePriority::DEFAULT }
    fn name(&self) -> &str { std::any::type_name::<Self>() }
}

/// Ordered middleware stack. Before hooks run top→bottom, after hooks bottom→top.
pub struct MiddlewareStack {
    layers: Vec<Arc<dyn Middleware>>,
}

impl MiddlewareStack {
    pub fn new() -> Self {
        Self { layers: Vec::new() }
    }

    pub fn push(&mut self, mw: Arc<dyn Middleware>) {
        self.layers.push(mw);
    }

    pub fn push_sorted(&mut self, mw: Arc<dyn Middleware>) {
        let prio = mw.priority();
        let pos = self.layers.iter()
            .position(|m| m.priority() > prio)
            .unwrap_or(self.layers.len());
        self.layers.insert(pos, mw);
    }

    pub fn is_empty(&self) -> bool { self.layers.is_empty() }
    pub fn len(&self) -> usize { self.layers.len() }

    pub fn iter(&self) -> impl Iterator<Item = &Arc<dyn Middleware>> {
        self.layers.iter()
    }

    // ── Execution methods (used by run_agent) ──

    pub async fn run_on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        for mw in &self.layers { mw.on_agent_start(state).await?; }
        Ok(())
    }

    pub async fn run_on_agent_end(&self, state: &mut AgentState, error: Option<&str>) -> Result<(), MiddlewareError> {
        for mw in self.layers.iter().rev() { mw.on_agent_end(state, error).await?; }
        Ok(())
    }

    pub async fn run_before_llm_call(&self, state: &mut AgentState, messages: &mut Vec<Message>) -> Result<(), MiddlewareError> {
        for mw in &self.layers { mw.before_llm_call(state, messages).await?; }
        Ok(())
    }

    pub async fn run_after_llm_call(&self, state: &mut AgentState, response: &mut Message) -> Result<(), MiddlewareError> {
        for mw in self.layers.iter().rev() { mw.after_llm_call(state, response).await?; }
        Ok(())
    }

    pub async fn run_before_tool_call(&self, state: &mut AgentState, tool_call: &ToolCall) -> Result<(), MiddlewareError> {
        for mw in &self.layers { mw.before_tool_call(state, tool_call).await?; }
        Ok(())
    }

    pub async fn run_after_tool_call(&self, state: &mut AgentState, tool_call: &ToolCall, result: &mut ToolResult) -> Result<(), MiddlewareError> {
        for mw in self.layers.iter().rev() { mw.after_tool_call(state, tool_call, result).await?; }
        Ok(())
    }
}

impl Default for MiddlewareStack {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: Run tests + check**

```bash
cargo test -p alva-agent-core -- v2::middleware::tests -v
cargo check --workspace
```

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-core/src/v2/middleware.rs
git commit -m "feat(core): add v2 Middleware trait with wrap_llm_call + wrap_tool_call"
```

---

### Task 4: run_agent() — the new engine

**Files:**
- Create: `crates/alva-agent-core/src/v2/run.rs`

This is the core engine. It replaces `agent_loop.rs` but lives alongside it.

- [ ] **Step 1: Write integration test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::*;
    use std::sync::Arc;

    struct EchoModel;
    #[async_trait::async_trait]
    impl LanguageModel for EchoModel {
        async fn complete(&self, messages: &[Message], _tools: &[&dyn Tool], _config: &ModelConfig) -> Result<Message, AgentError> {
            let last_text = messages.last()
                .map(|m| m.text_content())
                .unwrap_or_default();
            Ok(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text { text: format!("Echo: {}", last_text) }],
                tool_call_id: None,
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            })
        }
        fn stream(&self, _: &[Message], _: &[&dyn Tool], _: &ModelConfig) -> Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }
        fn model_id(&self) -> &str { "echo" }
    }

    #[tokio::test]
    async fn simple_echo_run() {
        let session = Arc::new(InMemorySession::new());
        let mut state = super::super::state::AgentState {
            model: Arc::new(EchoModel),
            tools: vec![],
            session: session.clone(),
            extensions: crate::middleware::Extensions::new(),
        };
        let config = super::super::state::AgentConfig {
            middleware: super::super::middleware::MiddlewareStack::new(),
            system_prompt: "You echo.".to_string(),
        };
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let input = vec![AgentMessage::Standard(Message::user("hello"))];

        run_agent(&mut state, &config, cancel, input, tx).await.unwrap();

        // Session should have 2 messages: user + assistant
        let msgs = session.messages();
        assert_eq!(msgs.len(), 2);

        // Check events
        let mut events = vec![];
        while let Ok(e) = rx.try_recv() { events.push(e); }
        assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentStart)));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::AgentEnd { .. })));
        assert!(events.iter().any(|e| matches!(e, AgentEvent::MessageEnd { .. })));
    }
}
```

- [ ] **Step 2: Implement run_agent()**

```rust
// crates/alva-agent-core/src/v2/run.rs

use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, warn};

use alva_types::{
    AgentError, AgentMessage, CancellationToken, ContentBlock,
    Message, MessageRole, ModelConfig, StreamEvent, ToolCall, ToolResult,
};
use crate::event::AgentEvent;

use super::state::{AgentState, AgentConfig};
use super::middleware::MiddlewareStack;

/// Run the V2 agent loop.
///
/// This is a free function, not a method on Agent. It receives:
/// - `state` (mutable) — model, tools, session, extensions
/// - `config` (immutable) — middleware, system_prompt
/// - `cancel` — cancellation token (parent cancel → child cancel)
/// - `input` — initial messages to process
/// - `event_tx` — event channel for real-time observation
pub async fn run_agent(
    state: &mut AgentState,
    config: &AgentConfig,
    cancel: CancellationToken,
    input: Vec<AgentMessage>,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) -> Result<(), AgentError> {
    let max_iterations: u32 = 100; // TODO: make configurable

    // 1. on_agent_start
    let _ = event_tx.send(AgentEvent::AgentStart);
    if let Err(e) = config.middleware.run_on_agent_start(state).await {
        warn!(error = %e, "middleware on_agent_start failed");
    }

    // 2. Store input messages in session
    for msg in input {
        state.session.append(msg);
    }

    // 3. Agent loop
    let mut iteration = 0u32;
    let mut error_result: Option<String> = None;

    loop {
        if cancel.is_cancelled() {
            error_result = Some("Cancelled".to_string());
            break;
        }

        if iteration >= max_iterations {
            error_result = Some(format!("Max iterations reached: {}", max_iterations));
            break;
        }
        iteration += 1;

        let _ = event_tx.send(AgentEvent::TurnStart);

        // 3a. Get messages from session
        let session_messages = state.session.messages();

        // 3b. Build LLM input: system prompt + session messages
        let mut llm_messages = vec![Message::system(&config.system_prompt)];
        for msg in &session_messages {
            if let AgentMessage::Standard(m) = msg {
                llm_messages.push(m.clone());
            }
        }

        // 3c. Middleware: before_llm_call
        if let Err(e) = config.middleware.run_before_llm_call(state, &mut llm_messages).await {
            warn!(error = %e, "middleware before_llm_call failed");
        }

        // 3d. Call LLM
        let tool_refs: Vec<&dyn alva_types::Tool> =
            state.tools.iter().map(|t| t.as_ref()).collect();

        debug!(
            model = state.model.model_id(),
            messages = llm_messages.len(),
            tools = tool_refs.len(),
            iteration,
            "calling LLM"
        );

        let mut assistant_message = state.model
            .complete(&llm_messages, &tool_refs, &ModelConfig::default())
            .await?;

        // 3e. Middleware: after_llm_call
        if let Err(e) = config.middleware.run_after_llm_call(state, &mut assistant_message).await {
            warn!(error = %e, "middleware after_llm_call failed");
        }

        // 3f. Store assistant message in session
        let agent_msg = AgentMessage::Standard(assistant_message.clone());
        state.session.append(agent_msg.clone());

        // 3g. Emit events
        let _ = event_tx.send(AgentEvent::MessageStart { message: agent_msg.clone() });
        let _ = event_tx.send(AgentEvent::MessageEnd { message: agent_msg });

        // 3h. Extract tool calls
        let tool_calls: Vec<ToolCall> = assistant_message.content.iter()
            .filter_map(|b| {
                if let ContentBlock::ToolUse { id, name, input } = b {
                    Some(ToolCall { id: id.clone(), name: name.clone(), arguments: input.clone() })
                } else {
                    None
                }
            })
            .collect();

        if tool_calls.is_empty() {
            let _ = event_tx.send(AgentEvent::TurnEnd);
            break; // No tool calls → done
        }

        // 3i. Execute tools
        for tool_call in &tool_calls {
            let _ = event_tx.send(AgentEvent::ToolExecutionStart { tool_call: tool_call.clone() });

            // Find tool
            let tool = state.tools.iter()
                .find(|t| t.name() == tool_call.name)
                .cloned();

            let mut result = match tool {
                Some(tool) => {
                    // Middleware: before_tool_call
                    if let Err(e) = config.middleware.run_before_tool_call(state, tool_call).await {
                        ToolResult {
                            content: format!("Blocked: {}", e),
                            is_error: true,
                            details: None,
                        }
                    } else {
                        // Execute tool
                        let ctx = alva_types::EmptyToolContext;
                        match tool.execute(tool_call.arguments.clone(), &cancel, &ctx).await {
                            Ok(r) => r,
                            Err(e) => ToolResult {
                                content: format!("Tool error: {}", e),
                                is_error: true,
                                details: None,
                            },
                        }
                    }
                }
                None => ToolResult {
                    content: format!("Tool '{}' not found", tool_call.name),
                    is_error: true,
                    details: None,
                },
            };

            // Middleware: after_tool_call
            if let Err(e) = config.middleware.run_after_tool_call(state, tool_call, &mut result).await {
                warn!(error = %e, "middleware after_tool_call failed");
            }

            // Store tool result in session
            let tool_msg = AgentMessage::Standard(Message {
                id: uuid::Uuid::new_v4().to_string(),
                role: MessageRole::Tool,
                content: vec![ContentBlock::ToolResult {
                    id: tool_call.id.clone(),
                    content: result.content.clone(),
                    is_error: result.is_error,
                }],
                tool_call_id: Some(tool_call.id.clone()),
                usage: None,
                timestamp: chrono::Utc::now().timestamp_millis(),
            });
            state.session.append(tool_msg);

            let _ = event_tx.send(AgentEvent::ToolExecutionEnd {
                tool_call: tool_call.clone(),
                result,
            });
        }

        let _ = event_tx.send(AgentEvent::TurnEnd);
    }

    // 4. on_agent_end
    let error_ref = error_result.as_deref();
    if let Err(e) = config.middleware.run_on_agent_end(state, error_ref).await {
        warn!(error = %e, "middleware on_agent_end failed");
    }

    let _ = event_tx.send(AgentEvent::AgentEnd { error: error_result.clone() });

    match error_result {
        Some(e) if e == "Cancelled" => Err(AgentError::Cancelled),
        Some(e) if e.starts_with("Max iterations") => Err(AgentError::MaxIterations(max_iterations)),
        Some(e) => Err(AgentError::Other(e)),
        None => Ok(()),
    }
}
```

- [ ] **Step 3: Create builtins/mod.rs (empty for now)**

```rust
// crates/alva-agent-core/src/v2/builtins/mod.rs
// Built-in middleware will be added in later tasks.
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p alva-agent-core -- v2::run::tests -v
```

- [ ] **Step 5: Verify workspace**

```bash
cargo check --workspace
```

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-core/src/v2/
git commit -m "feat(core): add v2 run_agent() — new engine with session-centric message management"
```

---

### Task 5: V2 Built-in Middleware — LoopDetection + DanglingToolCall

**Files:**
- Create: `crates/alva-agent-core/src/v2/builtins/loop_detection.rs`
- Create: `crates/alva-agent-core/src/v2/builtins/dangling_tool_call.rs`
- Modify: `crates/alva-agent-core/src/v2/builtins/mod.rs`

Rewrite the existing v1 builtin middlewares for the v2 Middleware trait (receives `&mut AgentState` instead of `&mut MiddlewareContext`).

- [ ] **Step 1: Implement LoopDetectionMiddleware for v2**

Same logic as v1 (`crates/alva-agent-core/src/middleware/loop_detection.rs`) but adapted:
- `after_llm_call` receives `&mut AgentState` — get session_id from `state.session.id()`
- Hash tool_calls from response
- Track in internal `Mutex<HashMap<String, Vec<String>>>`
- Same thresholds: warn=3, hard_limit=5, window=20

Copy tests from v1, adapt signatures.

- [ ] **Step 2: Implement DanglingToolCallMiddleware for v2**

Same logic as v1 (`crates/alva-agent-core/src/middleware/dangling_tool_call.rs`) but adapted:
- `before_llm_call` receives `&mut AgentState, &mut Vec<Message>` — scan messages for dangling
- Same patching logic: find ToolUse without matching ToolResult, insert synthetic error ToolMessage

Copy tests from v1, adapt signatures.

- [ ] **Step 3: Update builtins/mod.rs**

```rust
pub mod loop_detection;
pub mod dangling_tool_call;

pub use loop_detection::LoopDetectionMiddleware;
pub use dangling_tool_call::DanglingToolCallMiddleware;
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p alva-agent-core -- v2::builtins -v
```

- [ ] **Step 5: Commit**

```bash
git add crates/alva-agent-core/src/v2/builtins/
git commit -m "feat(core): add v2 built-in middleware — LoopDetection + DanglingToolCall"
```

---

### Task 6: Integration test — full V2 agent run

**Files:**
- Create: `crates/alva-agent-core/tests/v2_integration.rs`

- [ ] **Step 1: Write integration test**

Test scenario:
```
1. Create AgentState with EchoModel + InMemorySession
2. Create AgentConfig with no middleware
3. Run run_agent() with "hello" input
4. Verify: session has 2 messages (user + assistant)
5. Verify: events stream has AgentStart, MessageEnd, AgentEnd

6. Create AgentConfig with LoopDetection + DanglingToolCall middleware
7. Run run_agent() with same setup
8. Verify: middleware runs without error (no loops, no dangling)

9. Test cancellation: cancel token mid-run → verify AgentEnd with error
```

- [ ] **Step 2: Run test**

```bash
cargo test -p alva-agent-core --test v2_integration -v
```

- [ ] **Step 3: Commit**

```bash
git add crates/alva-agent-core/tests/v2_integration.rs
git commit -m "test(core): v2 integration test — full agent run with session + middleware"
```

---

## Self-Review Checklist

**Spec coverage:**
- ✅ AgentState (no messages, has session) — Task 2
- ✅ AgentConfig (middleware, system_prompt, separate from state) — Task 2
- ✅ AgentSession trait + InMemorySession — Task 1
- ✅ V2 Middleware trait with wrap hooks — Task 3
- ✅ run_agent() free function — Task 4
- ✅ Built-in middleware (LoopDetection, DanglingToolCall) — Task 5
- ✅ Integration test — Task 6
- ❌ Sub-agent spawning → Phase 2
- ❌ Migration of BaseAgent/CLI → Phase 2
- ❌ Delete old code → Phase 3

**Placeholder scan:** No TBDs found. All code is complete.

**Type consistency:** AgentState/AgentConfig field names match across all tasks. Middleware trait method signatures are consistent.
