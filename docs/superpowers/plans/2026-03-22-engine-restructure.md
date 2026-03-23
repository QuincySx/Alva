# Engine Restructure Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure srow-agent into a three-layer architecture: agent-base (types/traits), alva-core (loop engine), alva-graph (graph execution + orchestration).

**Architecture:** Create 3 new crates additively — no existing code is moved or deleted until the new crates are complete and tested. Existing srow-ai and srow-core continue to function. Migration happens as a final phase.

**Tech Stack:** Rust, async-trait, tokio, serde/serde_json, futures-core (streams), thiserror.

**Spec:** `docs/superpowers/specs/2026-03-22-engine-restructure-design.md`

---

## File Structure

### New crates to create

**agent-base** — Foundation types and traits:

| File | Responsibility |
|------|----------------|
| `crates/agent-base/Cargo.toml` | Dependencies: async-trait, serde, serde_json, thiserror, tokio, futures-core |
| `crates/agent-base/src/lib.rs` | Public re-exports |
| `crates/agent-base/src/message.rs` | `Message`, `MessageRole` |
| `crates/agent-base/src/content.rs` | `ContentBlock` enum (Text, Image, Reasoning, ToolUse, ToolResult) |
| `crates/agent-base/src/tool.rs` | `Tool` trait, `ToolCall`, `ToolResult`, `ToolRegistry` |
| `crates/agent-base/src/model.rs` | `LanguageModel` trait, `ModelConfig`, `UsageMetadata` |
| `crates/agent-base/src/stream.rs` | `StreamEvent` enum |
| `crates/agent-base/src/cancel.rs` | `CancellationToken` |
| `crates/agent-base/src/error.rs` | `AgentError` |

**alva-core** — Loop engine (~1200 lines target):

| File | Responsibility |
|------|----------------|
| `crates/alva-core/Cargo.toml` | Depends on agent-base |
| `crates/alva-core/src/lib.rs` | Public re-exports |
| `crates/alva-core/src/types.rs` | `AgentState`, `AgentMessage`, `AgentConfig`, `AgentContext` |
| `crates/alva-core/src/event.rs` | `AgentEvent` enum |
| `crates/alva-core/src/agent.rs` | `Agent` class: state + events + steering/followUp queues |
| `crates/alva-core/src/agent_loop.rs` | Core double-loop: LLM call → tool execution → repeat |
| `crates/alva-core/src/tool_executor.rs` | Parallel/sequential tool execution with before/after hooks |

**alva-graph** — Graph execution + orchestration:

| File | Responsibility |
|------|----------------|
| `crates/alva-graph/Cargo.toml` | Depends on alva-core + agent-base |
| `crates/alva-graph/src/lib.rs` | Public re-exports |
| `crates/alva-graph/src/graph.rs` | `StateGraph` builder: add_node/add_edge/compile |
| `crates/alva-graph/src/channel.rs` | `Channel` trait + `LastValue`, `BinaryOperatorAggregate`, `EphemeralValue` |
| `crates/alva-graph/src/pregel.rs` | Pregel BSP engine: plan → execute → update |
| `crates/alva-graph/src/session.rs` | `AgentSession`: wraps Agent/Graph with retry + compaction |
| `crates/alva-graph/src/retry.rs` | `RetryConfig` + exponential backoff |
| `crates/alva-graph/src/compaction.rs` | `CompactionConfig` + LLM summarization |
| `crates/alva-graph/src/checkpoint.rs` | `CheckpointSaver` trait + `InMemoryCheckpointSaver` |
| `crates/alva-graph/src/sub_agent.rs` | `SubAgentConfig` + `create_task_tool()` |
| `crates/alva-graph/src/context_transform.rs` | `ContextTransform` trait + `TransformPipeline` |

### Files to modify (final migration phase)

| File | Change |
|------|--------|
| `Cargo.toml` (workspace) | Add 3 new crate members |
| `crates/srow-app/Cargo.toml` | Add agent-base, alva-core dependencies |
| `crates/srow-core/Cargo.toml` | Add agent-base dependency |

---

## Task 1: agent-base — Types and Traits

**Files:**
- Create: `crates/agent-base/Cargo.toml`
- Create: `crates/agent-base/src/lib.rs`
- Create: `crates/agent-base/src/message.rs`
- Create: `crates/agent-base/src/content.rs`
- Create: `crates/agent-base/src/tool.rs`
- Create: `crates/agent-base/src/model.rs`
- Create: `crates/agent-base/src/stream.rs`
- Create: `crates/agent-base/src/cancel.rs`
- Create: `crates/agent-base/src/error.rs`
- Modify: `Cargo.toml` (workspace root — add member)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "agent-base"
version = "0.1.0"
edition = "2021"

[dependencies]
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
thiserror = "2"
tokio = { version = "1", features = ["sync"] }
futures-core = "0.3"
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
```

- [ ] **Step 2: Create cancel.rs — CancellationToken**

```rust
use std::sync::Arc;
use tokio::sync::watch;

#[derive(Clone)]
pub struct CancellationToken {
    sender: Arc<watch::Sender<bool>>,
    receiver: watch::Receiver<bool>,
}

impl CancellationToken {
    pub fn new() -> Self {
        let (sender, receiver) = watch::channel(false);
        Self { sender: Arc::new(sender), receiver }
    }

    pub fn cancel(&self) {
        let _ = self.sender.send(true);
    }

    pub fn is_cancelled(&self) -> bool {
        *self.receiver.borrow()
    }

    pub async fn cancelled(&mut self) {
        while !*self.receiver.borrow_and_update() {
            if self.receiver.changed().await.is_err() {
                break;
            }
        }
    }
}

impl Default for CancellationToken {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 3: Create error.rs**

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Tool error: {tool_name}: {message}")]
    ToolError { tool_name: String, message: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("Max iterations reached: {0}")]
    MaxIterations(u32),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("{0}")]
    Other(String),
}
```

- [ ] **Step 4: Create content.rs — multimodal content blocks**

```rust
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum ContentBlock {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "image")]
    Image { data: String, media_type: String },
    #[serde(rename = "reasoning")]
    Reasoning { text: String },
    #[serde(rename = "tool_use")]
    ToolUse {
        id: String,
        name: String,
        input: serde_json::Value,
    },
    #[serde(rename = "tool_result")]
    ToolResult {
        id: String,
        content: String,
        is_error: bool,
    },
}
```

- [ ] **Step 5: Create message.rs**

```rust
use serde::{Deserialize, Serialize};
use crate::content::ContentBlock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsageMetadata {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tool_calls: Vec<ToolCallData>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    pub timestamp: i64,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_calls: vec![],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_calls: vec![],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| match b {
                ContentBlock::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("")
    }
}
```

- [ ] **Step 6: Create stream.rs**

```rust
use serde::{Deserialize, Serialize};
use crate::message::UsageMetadata;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum StreamEvent {
    Start,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallDelta {
        id: String,
        name: Option<String>,
        arguments_delta: String,
    },
    Usage(UsageMetadata),
    Done,
    Error(String),
}
```

- [ ] **Step 7: Create tool.rs**

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use crate::cancel::CancellationToken;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolResult {
    pub tool_call_id: String,
    pub content: String,
    pub is_error: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub details: Option<serde_json::Value>,
}

#[async_trait]
pub trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> serde_json::Value;
    async fn execute(
        &self,
        input: serde_json::Value,
        cancel: &CancellationToken,
    ) -> Result<ToolResult, crate::error::AgentError>;
}

pub struct ToolRegistry {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: HashMap::new() }
    }

    pub fn register(&mut self, tool: Box<dyn Tool>) {
        let name = tool.name().to_string();
        self.tools.insert(name, tool);
    }

    pub fn get(&self, name: &str) -> Option<&dyn Tool> {
        self.tools.get(name).map(|t| t.as_ref())
    }

    pub fn list(&self) -> Vec<&dyn Tool> {
        self.tools.values().map(|t| t.as_ref()).collect()
    }

    pub fn remove(&mut self, name: &str) -> Option<Box<dyn Tool>> {
        self.tools.remove(name)
    }
}

impl Default for ToolRegistry {
    fn default() -> Self { Self::new() }
}
```

- [ ] **Step 8: Create model.rs**

```rust
use async_trait::async_trait;
use std::pin::Pin;
use futures_core::Stream;
use crate::message::Message;
use crate::stream::StreamEvent;
use crate::tool::Tool;
use crate::error::AgentError;

#[derive(Debug, Clone)]
pub struct ModelConfig {
    pub temperature: Option<f32>,
    pub max_tokens: Option<u32>,
    pub stop_sequences: Vec<String>,
    pub top_p: Option<f32>,
}

impl Default for ModelConfig {
    fn default() -> Self {
        Self {
            temperature: None,
            max_tokens: None,
            stop_sequences: vec![],
            top_p: None,
        }
    }
}

#[async_trait]
pub trait LanguageModel: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message, AgentError>;

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>>;

    fn model_id(&self) -> &str;
}
```

- [ ] **Step 9: Create lib.rs**

```rust
pub mod message;
pub mod content;
pub mod tool;
pub mod model;
pub mod stream;
pub mod cancel;
pub mod error;

pub use message::{Message, MessageRole, UsageMetadata, ToolCallData};
pub use content::ContentBlock;
pub use tool::{Tool, ToolCall, ToolResult, ToolRegistry};
pub use model::{LanguageModel, ModelConfig};
pub use stream::StreamEvent;
pub use cancel::CancellationToken;
pub use error::AgentError;
```

- [ ] **Step 10: Add to workspace and verify**

Add `"crates/agent-base"` to workspace `Cargo.toml` members.

Run: `cargo check -p agent-base && cargo test -p agent-base`
Expected: Compiles, 0 tests (types only).

- [ ] **Step 11: Commit**

```bash
git add crates/agent-base/ Cargo.toml
git commit -m "feat(agent-base): create foundation crate with types, traits, and abstractions"
```

---

## Task 2: alva-core — Loop Engine

**Files:**
- Create: `crates/alva-core/Cargo.toml`
- Create: `crates/alva-core/src/lib.rs`
- Create: `crates/alva-core/src/types.rs`
- Create: `crates/alva-core/src/event.rs`
- Create: `crates/alva-core/src/tool_executor.rs`
- Create: `crates/alva-core/src/agent_loop.rs`
- Create: `crates/alva-core/src/agent.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-core"
version = "0.1.0"
edition = "2021"

[dependencies]
agent-base = { path = "../agent-base" }
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt", "macros", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
futures-core = "0.3"
parking_lot = "0.12"
```

- [ ] **Step 2: Create types.rs — AgentState, AgentConfig, AgentMessage, AgentContext**

```rust
use agent_base::*;
use std::sync::Arc;

/// Extensible message type — Standard LLM messages + custom application messages
#[derive(Debug, Clone)]
pub enum AgentMessage {
    Standard(Message),
    Custom { type_name: String, data: serde_json::Value },
}

impl AgentMessage {
    pub fn from_message(msg: Message) -> Self {
        Self::Standard(msg)
    }

    pub fn custom(type_name: impl Into<String>, data: serde_json::Value) -> Self {
        Self::Custom { type_name: type_name.into(), data }
    }
}

/// The context passed to hooks
pub struct AgentContext {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
}

/// Decision from before_tool_call hook
pub enum ToolCallDecision {
    Allow,
    Block { reason: String },
}

/// Tool execution mode
#[derive(Debug, Clone, Copy, Default)]
pub enum ToolExecutionMode {
    #[default]
    Parallel,
    Sequential,
}

/// The 6 hooks that control the agent loop
pub struct AgentConfig {
    /// Required: convert AgentMessage[] to LLM-compatible Message[]
    pub convert_to_llm: Box<dyn Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync>,

    /// Optional: transform context before convert_to_llm
    pub transform_context: Option<Box<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage> + Send + Sync>>,

    /// Optional: intercept before tool execution, can block
    pub before_tool_call: Option<Box<dyn Fn(&ToolCall, &AgentContext) -> ToolCallDecision + Send + Sync>>,

    /// Optional: intercept after tool execution, can modify result
    pub after_tool_call: Option<Box<dyn Fn(&ToolCall, &mut ToolResult) + Send + Sync>>,

    /// Optional: poll for steering messages after each tool round
    pub get_steering_messages: Option<Box<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// Optional: poll for follow-up messages when agent would stop
    pub get_follow_up_messages: Option<Box<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// Tool execution mode
    pub tool_execution: ToolExecutionMode,
}

impl AgentConfig {
    /// Create with only the required convert_to_llm hook
    pub fn new(convert_to_llm: impl Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync + 'static) -> Self {
        Self {
            convert_to_llm: Box::new(convert_to_llm),
            transform_context: None,
            before_tool_call: None,
            after_tool_call: None,
            get_steering_messages: None,
            get_follow_up_messages: None,
            tool_execution: ToolExecutionMode::default(),
        }
    }
}

/// Runtime agent state
pub struct AgentState {
    pub system_prompt: String,
    pub messages: Vec<AgentMessage>,
    pub tools: Vec<Arc<dyn Tool>>,
    pub is_streaming: bool,
    pub current_stream_message: Option<AgentMessage>,
}

impl AgentState {
    pub fn new(system_prompt: String) -> Self {
        Self {
            system_prompt,
            messages: Vec::new(),
            tools: Vec::new(),
            is_streaming: false,
            current_stream_message: None,
        }
    }
}
```

- [ ] **Step 3: Create event.rs — AgentEvent**

```rust
use agent_base::*;
use crate::types::AgentMessage;

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
    ToolExecutionUpdate { tool_call_id: String, update: serde_json::Value },
    ToolExecutionEnd { tool_call: ToolCall, result: ToolResult },
}
```

- [ ] **Step 4: Create tool_executor.rs**

Executes tool calls from an LLM response, respecting before/after hooks and parallel/sequential mode.

```rust
use agent_base::*;
use crate::types::*;
use crate::event::AgentEvent;
use std::sync::Arc;
use tokio::sync::mpsc;

pub(crate) async fn execute_tools(
    tool_calls: &[ToolCall],
    tools: &[Arc<dyn Tool>],
    config: &AgentConfig,
    context: &AgentContext,
    cancel: &CancellationToken,
    event_tx: &mpsc::UnboundedSender<AgentEvent>,
) -> Vec<ToolResult> {
    match config.tool_execution {
        ToolExecutionMode::Parallel => execute_parallel(tool_calls, tools, config, context, cancel, event_tx).await,
        ToolExecutionMode::Sequential => execute_sequential(tool_calls, tools, config, context, cancel, event_tx).await,
    }
}

async fn execute_parallel(/* params */) -> Vec<ToolResult> {
    // 1. Pre-flight all tool calls (before_tool_call check)
    // 2. Spawn all allowed tools concurrently with tokio::JoinSet
    // 3. Collect results, apply after_tool_call
    // 4. Emit ToolExecutionStart/End events
    // 5. Return results in original tool_call order
    todo!("Implement parallel tool execution")
}

async fn execute_sequential(/* params */) -> Vec<ToolResult> {
    // Same as parallel but one at a time
    todo!("Implement sequential tool execution")
}
```

The implementer should fill in the actual logic following pi-alva-core's tool execution pattern. Key points:
- Look up each tool by name from the tools Vec
- Check `before_tool_call` → if `Block`, create error ToolResult with reason
- Call `tool.execute(input, cancel)`
- Call `after_tool_call` to let hooks modify the result
- Emit events via `event_tx`

- [ ] **Step 5: Create agent_loop.rs — double-loop execution**

```rust
use agent_base::*;
use crate::types::*;
use crate::event::AgentEvent;
use crate::tool_executor::execute_tools;
use std::sync::Arc;
use tokio::sync::mpsc;

/// Run the agent loop. Emits AgentEvents through the channel.
pub(crate) async fn run_loop(
    prompt_messages: Vec<AgentMessage>,
    state: &mut AgentState,
    model: &dyn LanguageModel,
    config: &AgentConfig,
    cancel: &CancellationToken,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
) {
    let _ = event_tx.send(AgentEvent::AgentStart);

    // Add prompt messages to state
    state.messages.extend(prompt_messages);

    // OUTER LOOP: follow-up messages
    loop {
        // INNER LOOP: tool calls + steering
        loop {
            if cancel.is_cancelled() {
                let _ = event_tx.send(AgentEvent::AgentEnd { error: Some("Cancelled".into()) });
                return;
            }

            let _ = event_tx.send(AgentEvent::TurnStart);

            // 1. Apply transform_context → convert_to_llm → get LLM messages
            let agent_messages = if let Some(ref transform) = config.transform_context {
                transform(&state.messages)
            } else {
                state.messages.clone()
            };
            let llm_messages = (config.convert_to_llm)(&agent_messages);

            // 2. Call LLM (stream)
            let tools_refs: Vec<&dyn Tool> = state.tools.iter().map(|t| t.as_ref()).collect();
            let model_config = ModelConfig::default();

            // Use complete() for simplicity; streaming via stream() can be added
            let result = model.complete(&llm_messages, &tools_refs, &model_config).await;

            let response = match result {
                Ok(msg) => msg,
                Err(e) => {
                    let _ = event_tx.send(AgentEvent::AgentEnd { error: Some(e.to_string()) });
                    return;
                }
            };

            // 3. Emit message events
            let agent_msg = AgentMessage::Standard(response.clone());
            let _ = event_tx.send(AgentEvent::MessageStart { message: agent_msg.clone() });
            let _ = event_tx.send(AgentEvent::MessageEnd { message: agent_msg.clone() });
            state.messages.push(agent_msg);

            // 4. Check for tool calls
            if response.tool_calls.is_empty() {
                let _ = event_tx.send(AgentEvent::TurnEnd);
                break; // No tool calls → exit inner loop
            }

            // 5. Execute tools
            let tool_calls: Vec<ToolCall> = response.tool_calls.iter().map(|tc| {
                ToolCall { id: tc.id.clone(), name: tc.name.clone(), arguments: tc.arguments.clone() }
            }).collect();

            let context = AgentContext {
                system_prompt: state.system_prompt.clone(),
                messages: state.messages.clone(),
                tools: state.tools.clone(),
            };

            let results = execute_tools(
                &tool_calls, &state.tools, config, &context, cancel, &event_tx,
            ).await;

            // 6. Push tool results into messages
            for result in results {
                let tool_msg = Message {
                    id: uuid::Uuid::new_v4().to_string(),
                    role: MessageRole::Tool,
                    content: vec![ContentBlock::ToolResult {
                        id: result.tool_call_id.clone(),
                        content: result.content.clone(),
                        is_error: result.is_error,
                    }],
                    tool_calls: vec![],
                    tool_call_id: Some(result.tool_call_id.clone()),
                    usage: None,
                    timestamp: chrono::Utc::now().timestamp_millis(),
                };
                state.messages.push(AgentMessage::Standard(tool_msg));
            }

            let _ = event_tx.send(AgentEvent::TurnEnd);

            // 7. Check steering messages
            if let Some(ref get_steering) = config.get_steering_messages {
                let steering = get_steering();
                if !steering.is_empty() {
                    state.messages.extend(steering);
                    continue; // Continue inner loop
                }
            }

            // No steering → continue inner loop (tool calls need another LLM call)
        }

        // 8. Check follow-up messages
        if let Some(ref get_follow_up) = config.get_follow_up_messages {
            let follow_ups = get_follow_up();
            if !follow_ups.is_empty() {
                state.messages.extend(follow_ups);
                continue; // Continue outer loop
            }
        }

        break; // No follow-ups → exit outer loop
    }

    let _ = event_tx.send(AgentEvent::AgentEnd { error: None });
}
```

- [ ] **Step 6: Create agent.rs — Agent class**

```rust
use agent_base::*;
use crate::types::*;
use crate::event::AgentEvent;
use crate::agent_loop;
use std::sync::Arc;
use tokio::sync::mpsc;
use parking_lot::Mutex;

pub struct Agent {
    state: Arc<Mutex<AgentState>>,
    config: Arc<AgentConfig>,
    model: Arc<dyn LanguageModel>,
    cancel_token: CancellationToken,
    steering_tx: mpsc::UnboundedSender<Vec<AgentMessage>>,
    steering_rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<AgentMessage>>>>,
    follow_up_tx: mpsc::UnboundedSender<Vec<AgentMessage>>,
    follow_up_rx: Arc<Mutex<mpsc::UnboundedReceiver<Vec<AgentMessage>>>>,
}

impl Agent {
    pub fn new(
        model: Arc<dyn LanguageModel>,
        system_prompt: String,
        config: AgentConfig,
    ) -> Self {
        let (steering_tx, steering_rx) = mpsc::unbounded_channel();
        let (follow_up_tx, follow_up_rx) = mpsc::unbounded_channel();

        Self {
            state: Arc::new(Mutex::new(AgentState::new(system_prompt))),
            config: Arc::new(config),
            model,
            cancel_token: CancellationToken::new(),
            steering_tx,
            steering_rx: Arc::new(Mutex::new(steering_rx)),
            follow_up_tx,
            follow_up_rx: Arc::new(Mutex::new(follow_up_rx)),
        }
    }

    /// Start a prompt and return event receiver
    pub fn prompt(&self, messages: Vec<AgentMessage>) -> mpsc::UnboundedReceiver<AgentEvent> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let state = Arc::clone(&self.state);
        let model = Arc::clone(&self.model);
        let config = Arc::clone(&self.config);
        let cancel = self.cancel_token.clone();

        tokio::spawn(async move {
            let mut state_guard = state.lock();
            agent_loop::run_loop(
                messages,
                &mut state_guard,
                model.as_ref(),
                &config,
                &cancel,
                event_tx,
            ).await;
        });

        event_rx
    }

    /// Cancel current execution
    pub fn cancel(&self) {
        self.cancel_token.cancel();
    }

    /// Inject steering messages while agent is running
    pub fn steer(&self, messages: Vec<AgentMessage>) {
        let _ = self.steering_tx.send(messages);
    }

    /// Inject follow-up messages
    pub fn follow_up(&self, messages: Vec<AgentMessage>) {
        let _ = self.follow_up_tx.send(messages);
    }

    /// Access current messages
    pub fn messages(&self) -> Vec<AgentMessage> {
        self.state.lock().messages.clone()
    }

    /// Update tools
    pub fn set_tools(&self, tools: Vec<Arc<dyn Tool>>) {
        self.state.lock().tools = tools;
    }

    /// Update model
    pub fn set_model(&mut self, model: Arc<dyn LanguageModel>) {
        self.model = model;
    }
}
```

> **Note**: The Agent class above is a starting point. The implementer should wire steering_rx/follow_up_rx into the agent_loop's hook system (via closures that drain the channels). The exact wiring depends on how `parking_lot::Mutex` interacts with async — may need `tokio::sync::Mutex` instead. Adapt as needed.

- [ ] **Step 7: Create lib.rs**

```rust
pub mod types;
pub mod event;
pub mod agent;
mod agent_loop;
mod tool_executor;

pub use types::{AgentMessage, AgentConfig, AgentState, AgentContext, ToolCallDecision, ToolExecutionMode};
pub use event::AgentEvent;
pub use agent::Agent;
```

- [ ] **Step 8: Add to workspace and verify**

Add `"crates/alva-core"` to workspace members.

Run: `cargo check -p alva-core`
Expected: Compiles.

- [ ] **Step 9: Write basic tests**

In `crates/alva-core/src/agent_loop.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use agent_base::*;

    // Mock LanguageModel for testing
    struct MockModel {
        response: Message,
    }

    #[async_trait::async_trait]
    impl LanguageModel for MockModel {
        async fn complete(&self, _messages: &[Message], _tools: &[&dyn Tool], _config: &ModelConfig) -> Result<Message, AgentError> {
            Ok(self.response.clone())
        }
        fn stream(&self, _messages: &[Message], _tools: &[&dyn Tool], _config: &ModelConfig) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures_core::stream::empty())
        }
        fn model_id(&self) -> &str { "mock" }
    }

    #[tokio::test]
    async fn basic_loop_completes() {
        let model = MockModel {
            response: Message {
                id: "1".into(),
                role: MessageRole::Assistant,
                content: vec![ContentBlock::Text { text: "Hello!".into() }],
                tool_calls: vec![],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
        };

        let config = AgentConfig::new(|messages| {
            messages.iter().filter_map(|m| match m {
                AgentMessage::Standard(msg) => Some(msg.clone()),
                _ => None,
            }).collect()
        });

        let mut state = AgentState::new("You are a test assistant".into());
        let cancel = CancellationToken::new();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        run_loop(
            vec![AgentMessage::from_message(Message::user("Hi"))],
            &mut state,
            &model,
            &config,
            &cancel,
            tx,
        ).await;

        // Collect events
        let mut events = vec![];
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        // Should have: AgentStart, TurnStart, MessageStart, MessageEnd, TurnEnd, AgentEnd
        assert!(events.len() >= 4);
        assert!(matches!(events.first(), Some(AgentEvent::AgentStart)));
        assert!(matches!(events.last(), Some(AgentEvent::AgentEnd { error: None })));
    }
}
```

Run: `cargo test -p alva-core`

- [ ] **Step 10: Commit**

```bash
git add crates/alva-core/ Cargo.toml
git commit -m "feat(alva-core): create loop engine with Agent, hooks, and events"
```

---

## Task 3: alva-graph — Graph Execution

**Files:**
- Create: `crates/alva-graph/Cargo.toml`
- Create: `crates/alva-graph/src/lib.rs`
- Create: `crates/alva-graph/src/channel.rs`
- Create: `crates/alva-graph/src/graph.rs`
- Create: `crates/alva-graph/src/pregel.rs`
- Modify: `Cargo.toml` (workspace root)

- [ ] **Step 1: Create Cargo.toml**

```toml
[package]
name = "alva-graph"
version = "0.1.0"
edition = "2021"

[dependencies]
agent-base = { path = "../agent-base" }
alva-core = { path = "../alva-core" }
async-trait = "0.1"
tokio = { version = "1", features = ["sync", "rt", "macros", "time"] }
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tracing = "0.1"
thiserror = "2"
parking_lot = "0.12"
```

- [ ] **Step 2: Create channel.rs**

```rust
use std::fmt::Debug;

pub trait Channel: Send + Sync {
    type Value: Clone + Send + Sync;
    type Update: Clone + Send + Sync;

    fn get(&self) -> Option<&Self::Value>;
    fn update(&mut self, values: Vec<Self::Update>) -> bool;
    fn reset(&mut self);
}

/// Stores exactly one value. Errors if multiple updates in same step.
pub struct LastValue<T: Clone + Send + Sync> {
    value: Option<T>,
}

/// Applies a reducer function to merge multiple updates.
pub struct BinaryOperatorAggregate<T: Clone + Send + Sync> {
    value: Option<T>,
    operator: Box<dyn Fn(T, T) -> T + Send + Sync>,
}

/// Cleared after each step.
pub struct EphemeralValue<T: Clone + Send + Sync> {
    value: Option<T>,
}
```

Implement `Channel` trait for each type. Add tests.

- [ ] **Step 3: Create graph.rs — StateGraph builder**

```rust
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use crate::pregel::Pregel;

pub const START: &str = "__start__";
pub const END: &str = "__end__";

pub type NodeFn<S> = Box<dyn Fn(S) -> Pin<Box<dyn Future<Output = S> + Send>> + Send + Sync>;
pub type RouterFn<S> = Box<dyn Fn(&S) -> String + Send + Sync>;

enum Edge {
    Direct { from: String, to: String },
    Conditional { from: String, router: RouterFn<serde_json::Value> },
}

pub struct StateGraph {
    nodes: HashMap<String, NodeFn<serde_json::Value>>,
    edges: Vec<Edge>,
    entry_point: Option<String>,
}

impl StateGraph {
    pub fn new() -> Self { /* ... */ }
    pub fn add_node(&mut self, name: &str, node: NodeFn<serde_json::Value>) { /* ... */ }
    pub fn add_edge(&mut self, from: &str, to: &str) { /* ... */ }
    pub fn add_conditional_edge(&mut self, from: &str, router: RouterFn<serde_json::Value>) { /* ... */ }
    pub fn set_entry_point(&mut self, name: &str) { /* ... */ }
    pub fn compile(self) -> Pregel { /* ... */ }
}
```

- [ ] **Step 4: Create pregel.rs — BSP execution engine**

```rust
use std::collections::HashMap;
use crate::graph::{NodeFn, Edge};

pub struct Pregel {
    nodes: HashMap<String, NodeFn<serde_json::Value>>,
    edges: Vec<Edge>,
    entry_point: String,
}

impl Pregel {
    pub async fn invoke(&self, input: serde_json::Value) -> Result<serde_json::Value, agent_base::AgentError> {
        // BSP loop:
        // 1. Start at entry_point
        // 2. Execute current node
        // 3. Follow edges (direct or conditional) to next node
        // 4. If next is END, return state
        // 5. Repeat
        todo!("Implement Pregel execution")
    }
}
```

- [ ] **Step 5: Create lib.rs with graph modules**

```rust
pub mod channel;
pub mod graph;
pub mod pregel;

pub use graph::{StateGraph, START, END};
pub use channel::{Channel, LastValue, BinaryOperatorAggregate, EphemeralValue};
pub use pregel::Pregel;
```

- [ ] **Step 6: Verify and commit**

Run: `cargo check -p alva-graph`

```bash
git add crates/alva-graph/ Cargo.toml
git commit -m "feat(alva-graph): create graph execution engine with StateGraph, channels, and Pregel"
```

---

## Task 4: alva-graph — Orchestration Layer

**Files:**
- Create: `crates/alva-graph/src/session.rs`
- Create: `crates/alva-graph/src/retry.rs`
- Create: `crates/alva-graph/src/compaction.rs`
- Create: `crates/alva-graph/src/checkpoint.rs`
- Create: `crates/alva-graph/src/sub_agent.rs`
- Create: `crates/alva-graph/src/context_transform.rs`
- Modify: `crates/alva-graph/src/lib.rs`

- [ ] **Step 1: Create checkpoint.rs**

```rust
use async_trait::async_trait;
use alva_core::AgentState;

#[async_trait]
pub trait CheckpointSaver: Send + Sync {
    async fn save(&self, id: &str, state: &AgentState) -> Result<(), agent_base::AgentError>;
    async fn load(&self, id: &str) -> Result<Option<AgentState>, agent_base::AgentError>;
    async fn list(&self) -> Result<Vec<String>, agent_base::AgentError>;
    async fn delete(&self, id: &str) -> Result<(), agent_base::AgentError>;
}

pub struct InMemoryCheckpointSaver { /* HashMap<String, AgentState> behind Mutex */ }
```

- [ ] **Step 2: Create retry.rs**

```rust
pub struct RetryConfig {
    pub max_retries: u32,
    pub initial_delay_ms: u64,
    pub max_delay_ms: u64,
    pub retryable: Box<dyn Fn(&agent_base::AgentError) -> bool + Send + Sync>,
}

impl Default for RetryConfig { /* max_retries: 3, initial: 1000, max: 30000 */ }

pub(crate) async fn with_retry<F, T>(config: &RetryConfig, f: F) -> Result<T, agent_base::AgentError>
where F: Fn() -> futures_core::future::BoxFuture<'static, Result<T, agent_base::AgentError>>
{ /* exponential backoff loop */ }
```

- [ ] **Step 3: Create compaction.rs**

```rust
pub struct CompactionConfig {
    pub max_tokens: usize,
    pub keep_recent: usize,
    pub model: std::sync::Arc<dyn agent_base::LanguageModel>,
}

pub async fn compact_messages(
    messages: &[alva_core::AgentMessage],
    config: &CompactionConfig,
) -> Result<Vec<alva_core::AgentMessage>, agent_base::AgentError> {
    // 1. Estimate token count
    // 2. If under threshold, return unchanged
    // 3. Split: old messages (to summarize) + recent (to keep)
    // 4. LLM-generate summary of old messages
    // 5. Return: [Custom("compaction_summary", summary)] + recent
    todo!()
}
```

- [ ] **Step 4: Create sub_agent.rs**

```rust
use agent_base::*;
use alva_core::*;
use std::sync::Arc;
use std::time::Duration;

pub struct SubAgentConfig {
    pub name: String,
    pub description: String,
    pub system_prompt: String,
    pub model: SubAgentModel,
    pub tools: SubAgentTools,
    pub disallowed_tools: Vec<String>,
    pub max_turns: u32,
    pub timeout: Duration,
}

pub enum SubAgentModel {
    Inherit,
    Specific(Arc<dyn LanguageModel>),
}

pub enum SubAgentTools {
    Inherit,
    Whitelist(Vec<String>),
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            name: "general-purpose".into(),
            description: "General-purpose sub-agent".into(),
            system_prompt: String::new(),
            model: SubAgentModel::Inherit,
            tools: SubAgentTools::Inherit,
            disallowed_tools: vec!["task".into()],
            max_turns: 50,
            timeout: Duration::from_secs(900),
        }
    }
}

/// Creates a "task" tool that spawns sub-agents
pub fn create_task_tool(
    configs: Vec<SubAgentConfig>,
    parent_model: Arc<dyn LanguageModel>,
    parent_tools: Vec<Arc<dyn Tool>>,
) -> Box<dyn Tool> {
    // Returns a tool that:
    // 1. Accepts {subagent_type, prompt} as arguments
    // 2. Looks up SubAgentConfig by name
    // 3. Creates a new Agent with isolated messages
    // 4. Removes disallowed_tools
    // 5. Runs the sub-agent, returns result as ToolResult
    todo!()
}
```

- [ ] **Step 5: Create context_transform.rs**

```rust
use alva_core::AgentMessage;

pub trait ContextTransform: Send + Sync {
    fn transform(&self, messages: &[AgentMessage]) -> Vec<AgentMessage>;
}

pub struct TransformPipeline {
    transforms: Vec<Box<dyn ContextTransform>>,
}

impl TransformPipeline {
    pub fn new() -> Self { Self { transforms: vec![] } }
    pub fn push(&mut self, transform: Box<dyn ContextTransform>) { self.transforms.push(transform); }
    pub fn apply(&self, messages: &[AgentMessage]) -> Vec<AgentMessage> {
        let mut result = messages.to_vec();
        for transform in &self.transforms {
            result = transform.transform(&result);
        }
        result
    }
}
```

- [ ] **Step 6: Create session.rs — AgentSession**

```rust
use alva_core::*;
use agent_base::*;
use crate::retry::*;
use crate::compaction::*;
use crate::checkpoint::*;
use std::sync::Arc;

enum SessionKind {
    Linear(Agent),
    Graph(crate::Pregel),
}

pub struct AgentSession {
    kind: SessionKind,
    retry_config: Option<RetryConfig>,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl AgentSession {
    pub fn from_agent(agent: Agent) -> Self {
        Self {
            kind: SessionKind::Linear(agent),
            retry_config: None,
            compaction_config: None,
            checkpoint_saver: None,
        }
    }

    pub fn with_retry(mut self, config: RetryConfig) -> Self {
        self.retry_config = Some(config);
        self
    }

    pub fn with_compaction(mut self, config: CompactionConfig) -> Self {
        self.compaction_config = Some(config);
        self
    }

    pub fn with_checkpoint(mut self, saver: Box<dyn CheckpointSaver>) -> Self {
        self.checkpoint_saver = Some(saver);
        self
    }
}
```

- [ ] **Step 7: Update lib.rs**

```rust
pub mod channel;
pub mod graph;
pub mod pregel;
pub mod session;
pub mod retry;
pub mod compaction;
pub mod checkpoint;
pub mod sub_agent;
pub mod context_transform;

pub use graph::{StateGraph, START, END};
pub use channel::{Channel, LastValue, BinaryOperatorAggregate, EphemeralValue};
pub use pregel::Pregel;
pub use session::AgentSession;
pub use retry::RetryConfig;
pub use compaction::CompactionConfig;
pub use checkpoint::{CheckpointSaver, InMemoryCheckpointSaver};
pub use sub_agent::{SubAgentConfig, SubAgentModel, SubAgentTools, create_task_tool};
pub use context_transform::{ContextTransform, TransformPipeline};
```

- [ ] **Step 8: Verify and commit**

Run: `cargo check -p alva-graph`

```bash
git add crates/alva-graph/
git commit -m "feat(alva-graph): add orchestration layer — session, retry, compaction, checkpoint, sub-agent"
```

---

## Task 5: Integration Verification

- [ ] **Step 1: Run full workspace check**

```bash
cargo check --workspace
```

All existing crates (srow-core, srow-ai, srow-app, srow-debug) should still compile unchanged. The new crates (agent-base, alva-core, alva-graph) compile independently.

- [ ] **Step 2: Run all tests**

```bash
cargo test --workspace
```

- [ ] **Step 3: Commit final state**

```bash
git add -A
git commit -m "feat: complete three-layer engine architecture — agent-base, alva-core, alva-graph"
```

---

## Future Work: Migration (not in this plan)

Once the 3 new crates are stable and tested:

1. **srow-core migration**: Update `srow-core` to depend on `agent-base` for types. Gradually replace `ports/provider/language_model.rs` imports with `agent_base::LanguageModel`. Replace `ports/tool.rs` with `agent_base::Tool`. Remove duplicated types.

2. **srow-ai migration**: Rename to `agent-base` or merge into it. Remove the `srow-core` dependency from `srow-ai` by using `agent-base` types instead.

3. **Engine migration**: Replace `srow-core::agent::runtime::engine` with `alva-core::Agent`. Wire existing tools to the new `Tool` trait via wrapper.

4. **Orchestrator migration**: Replace `srow-core::agent::orchestrator` with `alva-graph::AgentSession` + sub-agent tools.

5. **srow-app migration**: Update imports from `srow_core` to `agent_base`/`alva_core` where appropriate.

Each migration step is a separate plan/spec cycle.
