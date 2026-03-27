# Context as Agent Component — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make MessageStore and ContextPlugin first-class Agent components instead of external middleware wrappers.

**Architecture:** Remove the middleware adapter pattern. ContextPlugin hooks fire directly from the agent loop. MessageStore persists turns from the agent loop. ContextPlugin always present (defaults to `RulesContextPlugin`); MessageStore optional.

**Tech Stack:** Rust, alva-agent-core, alva-agent-context, alva-types, async-trait, tokio

---

## Dependency Refactor

Current (circular risk):
```
alva-agent-context → depends on → alva-agent-core (for Middleware trait)
```

After:
```
alva-agent-core → depends on → alva-agent-context (for ContextPlugin + MessageStore traits)
alva-agent-context → depends on → alva-types only (no agent-core)
```

## File Map

| File | Action | Responsibility |
|------|--------|----------------|
| `alva-agent-context/Cargo.toml` | Modify | Remove `alva-agent-core` dep |
| `alva-agent-context/src/lib.rs` | Modify | Remove middleware module |
| `alva-agent-context/src/middleware.rs` | Delete | No longer needed |
| `alva-agent-context/src/sdk_impl.rs` | Modify | Add MessageStore, remove blocking_lock on core types |
| `alva-agent-core/Cargo.toml` | Modify | Add `alva-agent-context` dep |
| `alva-agent-core/src/types.rs` | Modify | Add session_id to AgentState, add ContextPlugin + MessageStore fields to AgentHooks |
| `alva-agent-core/src/agent.rs` | Modify | Accept MessageStore + ContextPlugin in constructor |
| `alva-agent-core/src/agent_loop.rs` | Modify | Track turn boundaries, call plugin hooks, persist turns |
| `alva-agent-core/src/tool_executor.rs` | Modify | Call plugin before/after tool hooks |
| `alva-agent-core/src/lib.rs` | Modify | Re-export context types |

---

### Task 1: Break the dependency cycle

**Files:**
- Modify: `crates/alva-agent-context/Cargo.toml`
- Delete: `crates/alva-agent-context/src/middleware.rs`
- Modify: `crates/alva-agent-context/src/lib.rs`

- [ ] **Step 1: Remove `alva-agent-core` from context crate's Cargo.toml**

```toml
# Remove this line from [dependencies]:
# alva-agent-core = { path = "../alva-agent-core" }
```

- [ ] **Step 2: Delete middleware.rs**

```bash
rm crates/alva-agent-context/src/middleware.rs
```

- [ ] **Step 3: Update lib.rs — remove middleware module and export**

Remove:
```rust
pub mod middleware;
```
and:
```rust
pub use middleware::ContextManagementMiddleware;
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p alva-agent-context`
Expected: PASS (no errors, middleware was the only file depending on agent-core)

- [ ] **Step 5: Commit**

```bash
git add -A crates/alva-agent-context/
git commit -m "refactor(context): remove agent-core dependency, delete middleware adapter"
```

---

### Task 2: Add context dependency to agent-core

**Files:**
- Modify: `crates/alva-agent-core/Cargo.toml`
- Modify: `crates/alva-agent-core/src/types.rs`

- [ ] **Step 1: Add dependency**

In `crates/alva-agent-core/Cargo.toml`, add:
```toml
alva-agent-context = { path = "../alva-agent-context" }
```

- [ ] **Step 2: Add session_id to AgentState**

In `crates/alva-agent-core/src/types.rs`, add field to `AgentState`:
```rust
pub struct AgentState {
    pub session_id: String,              // NEW — identifies this session for MessageStore
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,
    pub model_config: ModelConfig,
    pub tool_context: Arc<dyn ToolContext>,
}
```

Update `AgentState::new()` to accept session_id:
```rust
pub fn new(session_id: impl Into<String>, system_prompt: impl Into<String>, model_config: ModelConfig) -> Self {
    Self {
        session_id: session_id.into(),
        system_prompt: system_prompt.into(),
        // ... rest unchanged
    }
}
```

- [ ] **Step 3: Add ContextPlugin + MessageStore to AgentHooks**

In `crates/alva-agent-core/src/types.rs`, add to AgentHooks:
```rust
use std::sync::Arc;
use tokio::sync::Mutex;
use alva_agent_context::{
    ContextPlugin, MessageStore, ContextManagementSDK,
    RulesContextPlugin, ContextSDKImpl, ContextStore,
};

pub struct AgentHooks {
    // ... existing fields ...
    pub context_plugin: Arc<dyn ContextPlugin>,         // always present
    pub context_sdk: Arc<dyn ContextManagementSDK>,     // always present
    pub message_store: Option<Arc<dyn MessageStore>>,   // optional persistence
}
```

Update `AgentHooks::new()` — default to `RulesContextPlugin`:
```rust
pub fn new(convert_to_llm: ConvertToLlmFn) -> Self {
    // Default context: RulesContextPlugin + ContextStore (200K window, 180K budget)
    let store = Arc::new(Mutex::new(ContextStore::new(200_000, 180_000, "/tmp/alva-ctx".into())));
    let sdk: Arc<dyn ContextManagementSDK> = Arc::new(ContextSDKImpl::new(store));
    let plugin: Arc<dyn ContextPlugin> = Arc::new(RulesContextPlugin::default());

    Self {
        // ... existing defaults ...
        context_plugin: plugin,
        context_sdk: sdk,
        message_store: None,
    }
}
```

- [ ] **Step 4: Fix all compilation errors from session_id change**

AgentState::new() callers need to pass session_id. Search for `AgentState::new(` and add `""` as first arg for existing callers.

Run: `cargo check -p alva-agent-core`

- [ ] **Step 5: Commit**

```bash
git add crates/alva-agent-core/
git commit -m "refactor(core): add ContextPlugin + MessageStore as Agent components"
```

---

### Task 3: Track turn boundaries in agent_loop

**Files:**
- Modify: `crates/alva-agent-core/src/agent_loop.rs`

This is the most critical change. We need to know where each turn starts and ends.

- [ ] **Step 1: Add turn tracking state**

At the top of `run_agent_loop`, after creating MiddlewareContext, add:
```rust
// Track turn boundary for MessageStore
let turn_start_msg_index = state.messages.len();
let turn_start_time = chrono::Utc::now().timestamp_millis();
```

- [ ] **Step 2: Call plugin lifecycle hooks in run_agent_loop (before inner loop)**

After `mw.on_agent_start`, add:
```rust
// Context plugin: bootstrap (once) + maintain (every turn) + on_agent_start
{
    let plugin = &config.context_plugin;
    let sdk = config.context_sdk.as_ref();
    // Bootstrap fires only once (plugin tracks internally)
    if let Err(e) = plugin.bootstrap(sdk.as_ref(), &state.session_id).await {
        tracing::warn!("context plugin bootstrap failed: {}", e);
    }
    plugin.on_agent_start(sdk.as_ref(), &state.session_id).await;
    if let Err(e) = plugin.maintain(sdk.as_ref(), &state.session_id).await {
        tracing::warn!("context plugin maintain failed: {}", e);
    }
}
```

- [ ] **Step 3: Call plugin.on_user_message before inner loop**

After the lifecycle hooks, process the user message:
```rust
{
    let plugin = &config.context_plugin;
    let sdk = config.context_sdk.as_ref();
    if let Some(last_user_msg) = state.messages.last() {
        let injections = plugin.on_user_message(sdk.as_ref(), &state.session_id, last_user_msg).await;
        // Process injections: append memory/skill/runtime to system_prompt or messages
        for injection in injections {
            match injection {
                alva_agent_context::Injection::Memory(facts) => {
                    if !facts.is_empty() {
                        let text = facts.iter().map(|f| format!("- {}", f.text)).collect::<Vec<_>>().join("\n");
                        state.system_prompt = format!("{}\n\n<user_memory>\n{}\n</user_memory>", state.system_prompt, text);
                    }
                }
                alva_agent_context::Injection::Skill { name, content } => {
                    state.system_prompt = format!("{}\n\n<skill name=\"{}\">\n{}\n</skill>", state.system_prompt, name, content);
                }
                alva_agent_context::Injection::RuntimeContext(data) => {
                    state.system_prompt = format!("{}\n\n<runtime>\n{}\n</runtime>", state.system_prompt, data);
                }
                alva_agent_context::Injection::Message(msg) => {
                    state.messages.push(msg);
                }
            }
        }
    }
}
```

- [ ] **Step 4: Delete transform_context, replace with plugin.assemble**

In `types.rs`, remove from AgentHooks:
```rust
// DELETE these:
pub transform_context: Option<TransformContextFn>,
// and the type alias:
pub type TransformContextFn = Arc<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage> + Send + Sync>;
```

Remove from `AgentHooks::new()` as well.

Change the context building section in `agent_loop.rs` (~line 119-128):

```rust
// Build context messages — always goes through ContextPlugin.assemble
let budget = config.context_sdk.budget(&state.session_id);
let context_messages = config.context_plugin.assemble(
    config.context_sdk.as_ref(),
    &state.session_id,
    state.messages.clone(),
    budget.budget_tokens,
).await;
```

- [ ] **Step 5: Call plugin.on_llm_output after LLM response**

After `mw.after_llm_call`, add:
```rust
{
    let plugin = &config.context_plugin;
    let sdk = config.context_sdk.as_ref();
    let agent_msg = AgentMessage::Standard(assistant_message.clone());
    plugin.on_llm_output(sdk.as_ref(), &state.session_id, &agent_msg).await;
}
```

- [ ] **Step 6: Persist turn via MessageStore after inner loop**

After inner loop exits, before outer loop follow-up check:
```rust
if let Some(store) = &config.message_store {
    let turn_messages: Vec<AgentMessage> = state.messages[turn_start_msg_index..].to_vec();
    if let Some((user_msg, agent_msgs)) = turn_messages.split_first() {
        let turn = alva_agent_context::Turn {
            index: store.turn_count(&state.session_id).await,
            user_message: user_msg.clone(),
            agent_messages: agent_msgs.to_vec(),
            started_at: turn_start_time,
            completed_at: Some(chrono::Utc::now().timestamp_millis()),
        };
        store.append_turn(&state.session_id, turn).await;
    }
}
```

- [ ] **Step 7: Call plugin.after_turn + on_agent_end**

In `run_agent_loop`, after inner loop, before mw.on_agent_end:
```rust
{
    let plugin = &config.context_plugin;
    let sdk = config.context_sdk.as_ref();
    plugin.after_turn(sdk.as_ref(), &state.session_id).await;
}
```

And in the agent_end section:
```rust
{
    let plugin = &config.context_plugin;
    let sdk = config.context_sdk.as_ref();
    plugin.on_agent_end(sdk.as_ref(), &state.session_id, error_str).await;
}
```

- [ ] **Step 8: Verify compilation**

Run: `cargo check -p alva-agent-core`

- [ ] **Step 9: Commit**

```bash
git add crates/alva-agent-core/src/agent_loop.rs
git commit -m "feat(core): integrate ContextPlugin + MessageStore into agent loop"
```

---

### Task 4: Wire plugin into tool_executor

**Files:**
- Modify: `crates/alva-agent-core/src/tool_executor.rs`

- [ ] **Step 1: Pass plugin + sdk to execute_tools**

Add parameters to `execute_tools()`:
```rust
pub(crate) async fn execute_tools(
    // ... existing params ...
    context_plugin: &Arc<dyn ContextPlugin>,
    context_sdk: &Arc<dyn ContextManagementSDK>,
) -> Vec<ToolResult>
```

- [ ] **Step 2: Call plugin.before_tool_call before execution**

Before executing each tool:
```rust
{
    let plugin = context_plugin;
    let sdk = context_sdk.as_ref();
    let action = plugin.before_tool_call(
        sdk.as_ref(),
        &session_id,
        &tool_call.name,
        &tool_call.arguments,
    ).await;
    match action {
        alva_agent_context::ToolCallAction::Block { reason } => {
            results.push(ToolResult { content: format!("Blocked: {}", reason), is_error: true });
            continue;
        }
        alva_agent_context::ToolCallAction::AllowWithWarning { warning } => {
            tracing::warn!(tool = tool_call.name, warning, "tool call warning");
        }
        alva_agent_context::ToolCallAction::Allow => {}
    }
}
```

- [ ] **Step 3: Call plugin.after_tool_call after execution**

After tool execution, before returning result:
```rust
{
    let plugin = context_plugin;
    let sdk = context_sdk.as_ref();
    let agent_msg = AgentMessage::Standard(Message::tool_result(&tool_call.id, &result.content, result.is_error));
    let tokens = result.content.len() / 4;
    let action = plugin.after_tool_call(
        sdk.as_ref(),
        &session_id,
        &tool_call.name,
        &agent_msg,
        tokens,
    ).await;
    match action {
        alva_agent_context::ToolResultAction::Truncate { max_lines } => {
            let lines: Vec<&str> = result.content.lines().collect();
            if lines.len() > max_lines {
                result.content = format!("{}\n[...truncated {} → {} lines]",
                    lines[..max_lines].join("\n"), lines.len(), max_lines);
            }
        }
        alva_agent_context::ToolResultAction::Replace { summary } => {
            result.content = summary;
        }
        alva_agent_context::ToolResultAction::Keep => {}
        alva_agent_context::ToolResultAction::Externalize { path } => {
            result.content = format!("[Result externalized to {}]", path);
        }
    }
}
```

- [ ] **Step 4: Update call site in agent_loop.rs**

Pass plugin and sdk to execute_tools:
```rust
let results = execute_tools(
    &tool_calls, &state.tools, config, &ctx, cancel, event_tx, &state.tool_context, mw_ctx,
    &config.context_plugin,             // NEW
    &config.context_sdk,               // NEW
).await;
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check -p alva-agent-core`

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-core/src/tool_executor.rs crates/alva-agent-core/src/agent_loop.rs
git commit -m "feat(core): wire ContextPlugin into tool execution"
```

---

### Task 5: Update Agent constructor

**Files:**
- Modify: `crates/alva-agent-core/src/agent.rs`

- [ ] **Step 1: Add builder methods for context components**

```rust
impl Agent {
    /// Replace the context plugin (default: RulesContextPlugin).
    pub fn set_context_plugin(
        &self,
        plugin: Arc<dyn ContextPlugin>,
        sdk: Arc<dyn ContextManagementSDK>,
    ) {
        let mut config = self.config.blocking_lock();
        config.context_plugin = plugin;
        config.context_sdk = sdk;
    }

    /// Set the message store for turn persistence.
    pub fn set_message_store(&self, store: Arc<dyn MessageStore>) {
        let mut config = self.config.blocking_lock();
        config.message_store = Some(store);
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p alva-agent-core`

- [ ] **Step 3: Commit**

```bash
git add crates/alva-agent-core/src/agent.rs
git commit -m "feat(core): add builder methods for ContextPlugin + MessageStore"
```

---

### Task 6: Update SDK impl to use MessageStore

**Files:**
- Modify: `crates/alva-agent-context/src/sdk_impl.rs`

- [ ] **Step 1: Add MessageStore to ContextSDKImpl**

```rust
pub struct ContextSDKImpl {
    store: Arc<Mutex<ContextStore>>,
    message_store: Option<Arc<dyn MessageStore>>,
}

impl ContextSDKImpl {
    pub fn new(store: Arc<Mutex<ContextStore>>) -> Self {
        Self { store, message_store: None }
    }

    pub fn with_message_store(mut self, ms: Arc<dyn MessageStore>) -> Self {
        self.message_store = Some(ms);
        self
    }
}
```

- [ ] **Step 2: Implement query_memory using MessageStore for context**

The `query_memory` and `inject_memory` methods can now potentially search through turn history via MessageStore. For now, mark as TODO but ensure MessageStore is accessible.

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p alva-agent-context`

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-context/src/sdk_impl.rs
git commit -m "feat(context): add MessageStore to SDK impl"
```

---

### Task 7: Fix downstream compilation

**Files:**
- Modify: any crates that depend on alva-agent-core's AgentState or middleware

- [ ] **Step 1: Find all AgentState::new() callers**

```bash
grep -rn "AgentState::new(" crates/
```

Add session_id parameter to each call site.

- [ ] **Step 2: Find all imports of ContextManagementMiddleware**

```bash
grep -rn "ContextManagementMiddleware" crates/
```

Remove any usage (it's been deleted).

- [ ] **Step 3: Full workspace check**

Run: `cargo check --workspace`

Fix any remaining errors.

- [ ] **Step 4: Commit**

```bash
git add -A
git commit -m "fix: update all callers for context-as-component refactor"
```

---

### Task 8: Integration test

**Files:**
- Create: `crates/alva-agent-core/tests/context_integration.rs`

- [ ] **Step 1: Write test — agent with DefaultContextPlugin + InMemoryMessageStore**

```rust
#[tokio::test]
async fn test_agent_with_context_plugin() {
    use alva_agent_context::*;
    use std::sync::Arc;
    use tokio::sync::Mutex;

    // Setup
    let store = Arc::new(Mutex::new(ContextStore::new(100_000, 80_000, "/tmp/test".into())));
    let message_store = Arc::new(InMemoryMessageStore::new());
    let plugin: Arc<dyn ContextPlugin> = Arc::new(RulesContextPlugin::default());
    let sdk: Arc<dyn ContextManagementSDK> = Arc::new(ContextSDKImpl::new(store));

    // Create agent with context components
    let model = /* test model */;
    let agent = Agent::new(model, "test-session", "You are helpful", Default::default());
    agent.with_context_plugin(plugin, sdk);
    agent.with_message_store(message_store.clone());

    // Send a message
    let rx = agent.prompt(vec![AgentMessage::user("Hello")]);

    // Consume events
    while let Some(event) = rx.recv().await { /* ... */ }

    // Verify: MessageStore should have 1 turn
    let turns = message_store.get_turns("test-session").await;
    assert_eq!(turns.len(), 1);
    assert_eq!(turns[0].user_message, AgentMessage::user("Hello"));
    assert!(!turns[0].agent_messages.is_empty());
}
```

- [ ] **Step 2: Write test — assemble respects sliding window**

```rust
#[tokio::test]
async fn test_context_plugin_sliding_window() {
    // Setup with RulesContextPlugin (max_messages: 5)
    let plugin = RulesContextPlugin { max_messages: 5, ..Default::default() };

    // Send 10 messages
    for i in 0..10 {
        agent.prompt(vec![AgentMessage::user(&format!("Message {}", i))]);
        // wait for completion
    }

    // The context sent to LLM should only have last 5 turns' messages
    // (verified by observing what convert_to_llm receives)
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p alva-agent-core`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-core/tests/
git commit -m "test(core): integration tests for context plugin + message store"
```

---

## Summary of changes

| Component | Before | After |
|-----------|--------|-------|
| MessageStore | Standalone, not connected | Agent component, agent_loop persists turns |
| ContextPlugin | Via middleware adapter | Agent component, agent_loop calls hooks directly |
| ContextManagementMiddleware | Bridges plugin ↔ middleware | Deleted |
| agent-context depends on | agent-core + alva-types | alva-types only |
| agent-core depends on | alva-types | alva-types + agent-context |
| transform_context hook | Existed but unused | Deleted — ContextPlugin.assemble is the only path |
| Turn tracking | None | agent_loop tracks start/end of each turn |
