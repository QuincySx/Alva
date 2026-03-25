# alva-engine-adapter-alva Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Create an EngineRuntime adapter that wraps the local alva-agent-core Agent, providing a unified execution interface alongside the existing Claude adapter.

**Architecture:** AlvaAdapter implements EngineRuntime by creating an Agent per session, mapping AgentEvent → RuntimeEvent through a background tokio task and mpsc channel. Permission flow is handled via before_tool_call hooks that pause on a oneshot channel until respond_permission() is called.

**Tech Stack:** Rust, async-trait, tokio (mpsc + oneshot + spawn), tokio-stream, alva-engine-runtime, alva-agent-core, alva-types

---

## File Structure

```
crates/alva-engine-adapter-alva/
├── Cargo.toml           — crate manifest (deps: engine-runtime, agent-core, types)
├── src/
│   ├── lib.rs           — public exports: AlvaAdapter, AlvaAdapterConfig
│   ├── config.rs        — AlvaAdapterConfig struct
│   ├── adapter.rs       — AlvaAdapter impl EngineRuntime (execute, cancel, respond_permission, capabilities)
│   └── mapping.rs       — EventMapper: AgentEvent → Vec<RuntimeEvent> stateful mapping
```

Also modified:
- `Cargo.toml` (workspace) — add member
- `scripts/ci-check-deps.sh` — add Rule 12

---

### Task 1: Scaffold crate and config

**Files:**
- Create: `crates/alva-engine-adapter-alva/Cargo.toml`
- Create: `crates/alva-engine-adapter-alva/src/lib.rs`
- Create: `crates/alva-engine-adapter-alva/src/config.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-engine-adapter-alva"
version = "0.1.0"
edition = "2021"
description = "EngineRuntime adapter for the local alva-agent-core Agent"

[dependencies]
alva-engine-runtime = { path = "../alva-engine-runtime" }
alva-agent-core = { path = "../alva-agent-core" }
alva-types = { path = "../alva-types" }
async-trait = "0.1"
futures-core = "0.3"
tokio = { version = "1", features = ["sync", "rt", "time"] }
tokio-stream = "0.1"
serde_json = "1"
tracing = "0.1"
uuid = { version = "1", features = ["v4"] }

[dev-dependencies]
tokio = { version = "1", features = ["full"] }
alva-test = { path = "../alva-test" }
```

- [ ] **Step 2: Create config.rs**

```rust
use std::sync::Arc;
use alva_types::{LanguageModel, Tool, ToolContext};
use alva_agent_core::{ConvertToLlmFn, ToolExecutionMode};

/// Configuration for the Alva engine adapter.
pub struct AlvaAdapterConfig {
    /// LLM model instance.
    pub model: Arc<dyn LanguageModel>,
    /// convert_to_llm hook (required by AgentHooks).
    pub convert_to_llm: ConvertToLlmFn,
    /// Tool set available to the agent.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Tool context for execution.
    pub tool_context: Arc<dyn ToolContext>,
    /// Tool execution mode (parallel or sequential).
    pub tool_execution: ToolExecutionMode,
    /// Maximum agentic turns (0 = use AgentHooks default of 100).
    pub max_iterations: u32,
    /// Enable streaming deltas.
    pub streaming: bool,
}
```

- [ ] **Step 3: Create lib.rs with placeholder**

```rust
mod adapter;
mod config;
mod mapping;

pub use adapter::AlvaAdapter;
pub use config::AlvaAdapterConfig;
```

- [ ] **Step 4: Add to workspace Cargo.toml**

Add `"crates/alva-engine-adapter-alva"` to `[workspace] members`.

- [ ] **Step 5: Create empty mapping.rs and adapter.rs so it compiles**

Minimal stubs to pass `cargo check`.

- [ ] **Step 6: Verify compilation**

Run: `cargo check -p alva-engine-adapter-alva`
Expected: PASS

- [ ] **Step 7: Commit**

```bash
git add crates/alva-engine-adapter-alva/ Cargo.toml
git commit -m "feat(alva-engine-adapter-alva): scaffold crate with config"
```

---

### Task 2: EventMapper — AgentEvent → RuntimeEvent

**Files:**
- Create: `crates/alva-engine-adapter-alva/src/mapping.rs`

The mapper is a stateful struct that tracks tool names (tool_use_id → name) and turn count, converting each AgentEvent into zero or more RuntimeEvents.

- [ ] **Step 1: Write failing tests for event mapping**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{ContentBlock, Message, MessageRole, StreamEvent, ToolCall, ToolResult};

    fn mapper() -> EventMapper {
        EventMapper::new("test-session".into())
    }

    #[test]
    fn agent_start_maps_to_session_started() {
        let mut m = mapper();
        let events = m.map(AgentEvent::AgentStart);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::SessionStarted { session_id, .. } if session_id == "test-session"));
    }

    #[test]
    fn message_end_splits_text_and_tool_use() {
        let mut m = mapper();
        let msg = AgentMessage::Standard(Message {
            id: "m1".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text { text: "hello".into() },
                ContentBlock::ToolUse { id: "tu1".into(), name: "shell".into(), input: serde_json::json!({}) },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        });
        let events = m.map(AgentEvent::MessageEnd { message: msg });
        // Should produce: Message (text only) + ToolStart
        assert!(events.iter().any(|e| matches!(e, RuntimeEvent::Message { content, .. } if content.len() == 1)));
        assert!(events.iter().any(|e| matches!(e, RuntimeEvent::ToolStart { name, .. } if name == "shell")));
    }

    #[test]
    fn message_update_maps_to_delta() {
        let mut m = mapper();
        let msg = AgentMessage::Standard(Message {
            id: "m1".into(), role: MessageRole::Assistant,
            content: vec![], tool_call_id: None, usage: None, timestamp: 0,
        });
        let delta = StreamEvent::TextDelta { text: "hi".into() };
        let events = m.map(AgentEvent::MessageUpdate { message: msg, delta: delta.clone() });
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::MessageDelta { id, .. } if id == "m1"));
    }

    #[test]
    fn tool_execution_end_maps_to_tool_end() {
        let mut m = mapper();
        // Register tool name via MessageEnd first
        let msg = AgentMessage::Standard(Message {
            id: "m1".into(), role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse { id: "tu1".into(), name: "grep".into(), input: serde_json::json!({}) }],
            tool_call_id: None, usage: None, timestamp: 0,
        });
        m.map(AgentEvent::MessageEnd { message: msg });

        let tc = ToolCall { id: "tu1".into(), name: "grep".into(), input: serde_json::json!({}) };
        let result = ToolResult { content: "found".into(), is_error: false, details: None };
        let events = m.map(AgentEvent::ToolExecutionEnd { tool_call: tc, result: result.clone() });
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::ToolEnd { name, .. } if name == "grep"));
    }

    #[test]
    fn agent_end_success_maps_to_completed() {
        let mut m = mapper();
        let events = m.map(AgentEvent::AgentEnd { error: None });
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::Completed { .. }));
    }

    #[test]
    fn agent_end_error_maps_to_error_then_completed() {
        let mut m = mapper();
        let events = m.map(AgentEvent::AgentEnd { error: Some("boom".into()) });
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Error { message, recoverable } if message == "boom" && !recoverable));
        assert!(matches!(&events[1], RuntimeEvent::Completed { result, .. } if result.is_none()));
    }

    #[test]
    fn turn_events_increment_counter() {
        let mut m = mapper();
        m.map(AgentEvent::TurnStart);
        m.map(AgentEvent::TurnEnd);
        m.map(AgentEvent::TurnStart);
        m.map(AgentEvent::TurnEnd);
        assert_eq!(m.turn_count(), 2);
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p alva-engine-adapter-alva`
Expected: FAIL (EventMapper not defined)

- [ ] **Step 3: Implement EventMapper**

```rust
use std::collections::HashMap;
use std::time::Instant;

use alva_agent_core::AgentEvent;
use alva_engine_runtime::event::{RuntimeEvent, RuntimeUsage};
use alva_types::{AgentMessage, ContentBlock, MessageRole};

/// Stateful mapper converting AgentEvent → Vec<RuntimeEvent>.
pub struct EventMapper {
    session_id: String,
    /// tool_use_id → tool_name (registered when ToolUse blocks appear in messages)
    tool_names: HashMap<String, String>,
    turn_count: u32,
    start_time: Instant,
}

impl EventMapper {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            tool_names: HashMap::new(),
            turn_count: 0,
            start_time: Instant::now(),
        }
    }

    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    pub fn map(&mut self, event: AgentEvent) -> Vec<RuntimeEvent> {
        match event {
            AgentEvent::AgentStart => {
                self.start_time = Instant::now();
                vec![RuntimeEvent::SessionStarted {
                    session_id: self.session_id.clone(),
                    model: None,
                    tools: vec![],
                }]
            }

            AgentEvent::TurnStart => {
                self.turn_count += 1;
                vec![]
            }
            AgentEvent::TurnEnd => vec![],
            AgentEvent::MessageStart { .. } => vec![],

            AgentEvent::MessageUpdate { message, delta } => {
                let id = match &message {
                    AgentMessage::Standard(m) => m.id.clone(),
                    AgentMessage::Custom { type_name, .. } => type_name.clone(),
                };
                vec![RuntimeEvent::MessageDelta { id, delta }]
            }

            AgentEvent::MessageEnd { message } => {
                let mut events = Vec::new();
                if let AgentMessage::Standard(ref msg) = message {
                    // 1) Register tool names and emit ToolStart
                    for block in &msg.content {
                        if let Some((id, name, input)) = block.as_tool_use() {
                            self.tool_names.insert(id.to_string(), name.to_string());
                            events.push(RuntimeEvent::ToolStart {
                                id: id.to_string(),
                                name: name.to_string(),
                                input: input.clone(),
                            });
                        }
                    }

                    // 2) Emit Message with non-tool content
                    let content: Vec<ContentBlock> = msg.content.iter()
                        .filter(|b| !b.is_tool_use())
                        .cloned()
                        .collect();
                    if !content.is_empty() {
                        events.insert(0, RuntimeEvent::Message {
                            id: msg.id.clone(),
                            role: MessageRole::Assistant,
                            content,
                        });
                    }
                }
                events
            }

            AgentEvent::ToolExecutionStart { .. } => vec![],
            AgentEvent::ToolExecutionUpdate { .. } => vec![],

            AgentEvent::ToolExecutionEnd { tool_call, result } => {
                let name = self.tool_names
                    .get(&tool_call.id)
                    .cloned()
                    .unwrap_or_else(|| tool_call.name.clone());
                vec![RuntimeEvent::ToolEnd {
                    id: tool_call.id,
                    name,
                    result,
                    duration_ms: None,
                }]
            }

            AgentEvent::AgentEnd { error } => {
                let duration = self.start_time.elapsed();
                let mut events = Vec::new();
                if let Some(ref err) = error {
                    events.push(RuntimeEvent::Error {
                        message: err.clone(),
                        recoverable: false,
                    });
                }
                events.push(RuntimeEvent::Completed {
                    session_id: self.session_id.clone(),
                    result: error.is_none().then(|| "completed".to_string()),
                    usage: Some(RuntimeUsage {
                        input_tokens: 0,
                        output_tokens: 0,
                        total_cost_usd: None,
                        duration_ms: duration.as_millis() as u64,
                        num_turns: self.turn_count,
                    }),
                });
                events
            }
        }
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p alva-engine-adapter-alva`
Expected: all 7 tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/alva-engine-adapter-alva/src/mapping.rs
git commit -m "feat(alva-engine-adapter-alva): implement EventMapper with tests"
```

---

### Task 3: AlvaAdapter — EngineRuntime implementation

**Files:**
- Create: `crates/alva-engine-adapter-alva/src/adapter.rs`

- [ ] **Step 1: Write integration test for execute() happy path**

Add to `crates/alva-engine-adapter-alva/src/adapter.rs` (or separate test file):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use alva_test::MockLanguageModel;
    use alva_types::{ContentBlock, Message, MessageRole, EmptyToolContext};
    use alva_agent_core::{AgentHooks, AgentMessage, ConvertToLlmFn};
    use tokio_stream::StreamExt;

    fn default_convert() -> ConvertToLlmFn {
        Arc::new(|ctx| {
            ctx.messages.iter().filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.clone()),
                _ => None,
            }).collect()
        })
    }

    fn test_config() -> AlvaAdapterConfig {
        let model = Arc::new(MockLanguageModel::with_text_response("Hello from Alva!"));
        AlvaAdapterConfig {
            model,
            convert_to_llm: default_convert(),
            tools: vec![],
            tool_context: Arc::new(EmptyToolContext),
            tool_execution: alva_agent_core::ToolExecutionMode::Parallel,
            max_iterations: 10,
            streaming: false,
        }
    }

    #[tokio::test]
    async fn test_execute_simple_prompt() {
        let adapter = AlvaAdapter::new(test_config());
        let request = RuntimeRequest::new("Say hello");
        let stream = adapter.execute(request).unwrap();
        let events: Vec<RuntimeEvent> = stream.collect().await;

        assert!(matches!(&events[0], RuntimeEvent::SessionStarted { .. }));
        assert!(events.iter().any(|e| matches!(e, RuntimeEvent::Message { .. })));
        assert!(matches!(events.last().unwrap(), RuntimeEvent::Completed { .. }));
    }

    #[tokio::test]
    async fn test_cancel_stops_execution() {
        let adapter = AlvaAdapter::new(test_config());
        let request = RuntimeRequest::new("Do something");
        let mut stream = adapter.execute(request).unwrap();

        // Consume SessionStarted
        let first = stream.next().await.unwrap();
        let session_id = match first {
            RuntimeEvent::SessionStarted { session_id, .. } => session_id,
            _ => panic!("expected SessionStarted"),
        };

        // Cancel immediately
        adapter.cancel(&session_id).await.unwrap();

        // Drain — should get Completed eventually
        let remaining: Vec<RuntimeEvent> = stream.collect().await;
        assert!(remaining.iter().any(|e| matches!(e, RuntimeEvent::Completed { .. })));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p alva-engine-adapter-alva`
Expected: FAIL (AlvaAdapter not defined)

- [ ] **Step 3: Implement AlvaAdapter**

```rust
use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;

use alva_agent_core::{Agent, AgentHooks, AgentMessage};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use alva_types::{CancellationToken, ContentBlock, Message, MessageRole};

use crate::config::AlvaAdapterConfig;
use crate::mapping::EventMapper;

struct ActiveSession {
    cancel: CancellationToken,
    /// Pending permission responses: request_id → oneshot sender
    permission_tx: Arc<Mutex<HashMap<String, oneshot::Sender<PermissionDecision>>>>,
}

pub struct AlvaAdapter {
    config: AlvaAdapterConfig,
    sessions: Arc<Mutex<HashMap<String, ActiveSession>>>,
}

impl AlvaAdapter {
    pub fn new(config: AlvaAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for AlvaAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        let session_id = uuid::Uuid::new_v4().to_string();
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        // Build agent
        let mut hooks = AgentHooks::new(self.config.convert_to_llm.clone());
        hooks.tool_execution = self.config.tool_execution;
        if self.config.max_iterations > 0 {
            hooks.max_iterations = self.config.max_iterations;
        }

        let agent = Agent::new(
            self.config.model.clone(),
            request.system_prompt.unwrap_or_default(),
            hooks,
        );

        // Set tools and streaming
        let tools = self.config.tools.clone();
        let streaming = request.options.streaming || self.config.streaming;
        let tool_context = self.config.tool_context.clone();
        let cancel_token = CancellationToken::new();

        let active = ActiveSession {
            cancel: cancel_token.clone(),
            permission_tx: Arc::new(Mutex::new(HashMap::new())),
        };

        // Register session synchronously (before spawning)
        let sessions = self.sessions.clone();
        let sid = session_id.clone();

        // Build user message
        let user_msg = AgentMessage::Standard(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: request.prompt }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        });

        // Spawn background task
        tokio::spawn(async move {
            // Configure agent (async operations)
            agent.set_tools(tools).await;
            agent.set_streaming(streaming).await;
            // Set tool context via state
            {
                let mut st = agent.state.lock().await; // Note: need to expose state or use set_tool_context
                // For now, we set it through the config
            }

            // Register session
            sessions.lock().await.insert(sid.clone(), active);

            // Start agent loop
            let mut agent_rx = agent.prompt(vec![user_msg]);
            let mut mapper = EventMapper::new(sid.clone());

            while let Some(agent_event) = agent_rx.recv().await {
                let runtime_events = mapper.map(agent_event);
                for re in runtime_events {
                    let is_completed = matches!(re, RuntimeEvent::Completed { .. });
                    if event_tx.send(re).is_err() {
                        break;
                    }
                    if is_completed {
                        // Clean up session
                        sessions.lock().await.remove(&sid);
                        return;
                    }
                }
            }

            // If loop ends without Completed (shouldn't happen, but safety net)
            let _ = event_tx.send(RuntimeEvent::Completed {
                session_id: sid.clone(),
                result: None,
                usage: None,
            });
            sessions.lock().await.remove(&sid);
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;
        session.cancel.cancel();
        Ok(())
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let session = sessions
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;
        let mut pending = session.permission_tx.lock().await;
        let tx = pending
            .remove(request_id)
            .ok_or_else(|| RuntimeError::PermissionNotFound(request_id.into()))?;
        let _ = tx.send(decision);
        Ok(())
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: true,
            resume: false,
            cancel: true,
        }
    }
}
```

Note: The actual implementation will need to handle the fact that `Agent::state` is not directly accessible from outside. We'll need to use the public API (`set_tools`, `set_streaming`, `set_model_config`) and pass tool_context through `AgentState::with_tool_context`. Read the actual Agent API carefully during implementation and adjust.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test -p alva-engine-adapter-alva`
Expected: all tests PASS

- [ ] **Step 5: Commit**

```bash
git add crates/alva-engine-adapter-alva/src/adapter.rs
git commit -m "feat(alva-engine-adapter-alva): implement EngineRuntime with execute/cancel"
```

---

### Task 4: CI firewall rule + final verification

**Files:**
- Modify: `scripts/ci-check-deps.sh`

- [ ] **Step 1: Add CI rule for alva-engine-adapter-alva**

After the existing adapter-claude rule, add:

```bash
# Rule 12: alva-engine-adapter-alva only depends on alva-types + alva-engine-runtime + alva-agent-core
check_no_workspace_deps "alva-engine-adapter-alva" "alva-types|alva-engine-runtime|alva-agent-core"
```

- [ ] **Step 2: Full workspace check**

Run: `cargo check --workspace`
Expected: PASS (ignoring pre-existing alva-app settings_model error)

- [ ] **Step 3: Full test suite**

Run: `cargo test -p alva-engine-adapter-alva`
Expected: all tests PASS

- [ ] **Step 4: Commit**

```bash
git add scripts/ci-check-deps.sh
git commit -m "chore: add CI firewall rule for alva-engine-adapter-alva"
```

---

## Key Implementation Notes

1. **Agent.state is private** — cannot access `agent.state.lock()` from outside. Use `Agent::new()` which takes `system_prompt` + `AgentHooks`. Tools are set via `agent.set_tools()`, streaming via `agent.set_streaming()`. For tool_context, check if Agent exposes a setter or if AgentState needs a public method added.

2. **CancellationToken** — Agent owns its own CancellationToken internally. Use `agent.cancel()` method rather than a separate token.

3. **Permission flow** — v1 can skip interactive permission (just allow all). Add permission hook support as a follow-up if needed. The `respond_permission` plumbing is in place for future use.

4. **chrono dependency** — Needed for `timestamp_millis()` on Message construction. Add to Cargo.toml if not transitively available.

5. **alva-test MockLanguageModel** — Check the exact API in `crates/alva-test/` for test setup. It may need `with_text_response()` or similar constructor.
