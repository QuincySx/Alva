# Agent Engine Restructure: Three-Layer Architecture

## Overview

Restructure srow-agent's engine from a monolithic `alva-app-core` into a three-layer architecture inspired by pi-alva-core (minimal loop engine) and LangGraph (graph execution + orchestration). Rename `srow-ai` to `agent-base`.

The goal: separate stable infrastructure (types, traits) from the loop engine, from the graph/orchestration layer. Each layer is independently usable. Agent paradigms change fast — this structure lets us swap the top layers without touching the foundation.

## Reference Projects

| Layer | pi-mono equivalent | LangGraph equivalent |
|-------|-------------------|---------------------|
| agent-base | pi-ai | langchain-core (messages, tools, models) |
| alva-core | pi-alva-core (1,220 lines, 5 files) | langchain-core (Runnable) |
| alva-graph | coding-agent's AgentSession | langgraph (StateGraph, Pregel, Checkpoint) |

## Crate Structure

```
crates/
├── agent-base/       ← Foundation: types, traits, providers (stable, rarely changes)
├── alva-core/       ← Loop engine: Agent + hooks + events (small, focused)
├── alva-graph/      ← Graph execution + orchestration (checkpoint, sub-agent, retry, compaction)
├── alva-app-core/        ← Slimmed: MCP, environment, security, domain logic (app-specific)
├── alva-app/         ← UI application (unchanged)
└── alva-app-debug/       ← Debug server (unchanged)
```

### Dependency Direction (strictly one-way)

```
agent-base          ← zero engine dependency
    ↑
alva-core          ← depends only on agent-base
    ↑
alva-graph         ← depends on alva-core + agent-base
    ↑
alva-app-core           ← depends on alva-graph (or alva-core directly)
    ↑
alva-app            ← depends on alva-app-core
```

### Usage Patterns

```
Simple agent (linear loop):     agent-base + alva-core
Complex agent (graph/orchestration): agent-base + alva-core + alva-graph
Full application:               all crates
```

---

## agent-base (renamed from srow-ai)

Foundation types and traits. Everything else depends on this. No engine logic.

### File Structure

```
crates/agent-base/src/
├── lib.rs
├── message.rs          ← Message types
├── content.rs          ← Multimodal content blocks
├── tool.rs             ← Tool trait + ToolCall + ToolResult
├── model.rs            ← LanguageModel trait
├── provider/           ← LLM Provider implementations
│   ├── mod.rs
│   ├── openai.rs
│   └── anthropic.rs
├── stream.rs           ← Stream event types
├── transport.rs        ← Transport trait (HTTP/SSE)
├── config.rs           ← Shared config types
└── error.rs            ← Error types
```

### Core Types

**Message:**

```rust
enum MessageRole { User, Assistant, System, Tool }

struct Message {
    id: String,
    role: MessageRole,
    content: Vec<ContentBlock>,
    tool_calls: Vec<ToolCall>,       // Assistant only
    tool_call_id: Option<String>,    // Tool only
    usage: Option<UsageMetadata>,    // Assistant only
    timestamp: i64,
}
```

**ContentBlock (multimodal):**

```rust
enum ContentBlock {
    Text { text: String },
    Image { data: String, media_type: String },
    Reasoning { text: String },
    ToolUse { id: String, name: String, input: Value },
    ToolResult { id: String, content: String, is_error: bool },
}
```

**Tool trait:**

```rust
trait Tool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn parameters_schema(&self) -> Value;  // JSON Schema
    async fn execute(&self, input: Value, cancel: &CancellationToken) -> Result<ToolResult>;
}

struct ToolCall {
    id: String,
    name: String,
    arguments: Value,
}

struct ToolResult {
    tool_call_id: String,
    content: String,
    is_error: bool,
    details: Option<Value>,  // Structured data for UI, not sent to LLM
}
```

**LanguageModel trait:**

```rust
trait LanguageModel: Send + Sync {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Result<Message>;

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn Tool],
        config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent>>>;
}
```

**StreamEvent:**

```rust
enum StreamEvent {
    Start,
    TextDelta { text: String },
    ReasoningDelta { text: String },
    ToolCallDelta { id: String, name: Option<String>, arguments_delta: String },
    Usage(UsageMetadata),
    Done,
    Error(String),
}
```

### Source Mapping

| agent-base | Current source |
|------------|---------------|
| Message/ContentBlock | alva-app-core `domain::message` + srow-ai `types` |
| Tool trait | alva-app-core `ports::tool` |
| LanguageModel trait | alva-app-core `ports::provider::language_model` |
| Provider impls | alva-app-core `adapters::llm::openai` |
| StreamEvent | srow-ai `transport` |
| Transport | srow-ai `transport::traits` |

---

## alva-core (new crate)

Minimal loop engine. Target: ~1,200 lines, 5-6 files. Modeled on pi-alva-core.

### File Structure

```
crates/alva-core/src/
├── lib.rs
├── types.rs            ← AgentState, AgentConfig, AgentEvent, CancellationToken
├── agent.rs            ← Agent class: state + event subscription + steering/followUp queues
├── agent_loop.rs       ← Core loop: LLM call → tool execution → repeat
├── tool_executor.rs    ← Tool executor: parallel/sequential + hooks
└── stream.rs           ← EventStream push-based async iterator
```

### AgentConfig — 6 Hooks

```rust
struct AgentConfig {
    /// Required: convert AgentMessage[] to LLM-compatible Message[]
    convert_to_llm: Box<dyn Fn(&[AgentMessage]) -> Vec<Message> + Send + Sync>,

    /// Optional: transform context before convert_to_llm
    transform_context: Option<Box<dyn Fn(&[AgentMessage]) -> Vec<AgentMessage> + Send + Sync>>,

    /// Optional: intercept before tool execution, can block
    before_tool_call: Option<Box<dyn Fn(&ToolCall, &AgentContext) -> ToolCallDecision + Send + Sync>>,

    /// Optional: intercept after tool execution, can modify result
    after_tool_call: Option<Box<dyn Fn(&ToolCall, &mut ToolResult) + Send + Sync>>,

    /// Optional: poll for steering messages after each tool round
    get_steering_messages: Option<Box<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// Optional: poll for follow-up messages when agent would stop
    get_follow_up_messages: Option<Box<dyn Fn() -> Vec<AgentMessage> + Send + Sync>>,

    /// Tool execution mode
    tool_execution: ToolExecutionMode,
}

enum ToolCallDecision {
    Allow,
    Block { reason: String },
}

enum ToolExecutionMode {
    Parallel,
    Sequential,
}
```

### AgentEvent

```rust
enum AgentEvent {
    AgentStart,
    AgentEnd { error: Option<String> },

    TurnStart,
    TurnEnd,

    MessageStart { message: AgentMessage },
    MessageUpdate { message: AgentMessage, delta: StreamEvent },
    MessageEnd { message: AgentMessage },

    ToolExecutionStart { tool_call: ToolCall },
    ToolExecutionUpdate { tool_call_id: String, update: Value },
    ToolExecutionEnd { tool_call: ToolCall, result: ToolResult },
}
```

### Double-Loop Execution (from pi-alva-core)

```
agent_loop(prompt, context, config):
  emit AgentStart

  OUTER LOOP (follow-up):
    INNER LOOP (tool calls + steering):
      1. Inject pending steering/follow-up messages
      2. transform_context() → convert_to_llm() → LLM stream
      3. emit MessageStart/Update/End
      4. If tool_calls:
         a. before_tool_call() → Allow/Block
         b. Execute tools (parallel or sequential)
         c. after_tool_call()
         d. emit ToolExecutionStart/End
         e. Push results to context
      5. emit TurnEnd
      6. Check cancellation
      7. Poll get_steering_messages() → if any, continue INNER
    END INNER
    8. Poll get_follow_up_messages() → if any, continue OUTER
  END OUTER

  emit AgentEnd
```

### Agent Class

```rust
struct Agent {
    state: AgentState,
    config: AgentConfig,
    model: Box<dyn LanguageModel>,
    cancel_token: CancellationToken,
}

impl Agent {
    fn new(model: Box<dyn LanguageModel>, config: AgentConfig) -> Self;

    /// Start conversation, returns event stream
    fn prompt(&self, messages: Vec<AgentMessage>) -> EventStream<AgentEvent>;

    /// Continue from existing context
    fn continue_loop(&self) -> EventStream<AgentEvent>;

    /// Cancel current execution
    fn cancel(&self);

    /// Inject steering messages (call while Agent is running)
    fn steer(&self, messages: Vec<AgentMessage>);

    /// Inject follow-up messages
    fn follow_up(&self, messages: Vec<AgentMessage>);

    // State access
    fn messages(&self) -> &[AgentMessage];
    fn set_model(&mut self, model: Box<dyn LanguageModel>);
    fn set_tools(&mut self, tools: Vec<Box<dyn Tool>>);
}
```

### AgentMessage (extensible)

```rust
enum AgentMessage {
    /// Standard LLM message
    Standard(Message),
    /// Custom message type (upper layer defines, engine doesn't care)
    Custom { type_name: String, data: Value },
}
```

Upper layer decides how to convert `Custom` to LLM `Message` via the `convert_to_llm` hook.

---

## alva-graph (new crate)

Graph execution + orchestration. The LangGraph equivalent, built on top of alva-core.

### File Structure

```
crates/alva-graph/src/
├── lib.rs
│
│  ── Graph Execution (from LangGraph) ──
├── graph.rs              ← StateGraph builder: add_node/add_edge/compile
├── channel.rs            ← Channel trait + LastValue/BinaryOp/Ephemeral
├── pregel.rs             ← Pregel execution engine: plan → execute → update
│
│  ── Orchestration ──
├── session.rs            ← AgentSession: wraps Agent/Graph with retry/compaction
├── retry.rs              ← Auto retry + exponential backoff
├── compaction.rs         ← Context compression (LLM summarization)
├── checkpoint.rs         ← Checkpoint trait + InMemory implementation
├── sub_agent.rs          ← Sub-Agent scheduling (task tool pattern)
└── context_transform.rs  ← Composable context transform pipeline
```

### Graph Execution

**StateGraph builder (from LangGraph):**

```rust
struct StateGraph<S> {
    nodes: HashMap<String, Box<dyn NodeFn<S>>>,
    edges: Vec<Edge>,
    entry_point: Option<String>,
}

impl<S> StateGraph<S> {
    fn new() -> Self;
    fn add_node(&mut self, name: &str, node: impl NodeFn<S>);
    fn add_edge(&mut self, from: &str, to: &str);
    fn add_conditional_edge(&mut self, from: &str, router: impl Fn(&S) -> &str);
    fn set_entry_point(&mut self, name: &str);
    fn compile(self, checkpoint: Option<Box<dyn CheckpointSaver>>) -> CompiledGraph<S>;
}

impl<S> CompiledGraph<S> {
    async fn invoke(&self, input: S) -> Result<S>;
    fn stream(&self, input: S) -> impl Stream<Item = GraphEvent<S>>;
}
```

**Channel system (from LangGraph):**

```rust
trait Channel: Send + Sync {
    type Value;
    type Update;

    fn get(&self) -> Result<Self::Value>;
    fn update(&mut self, values: &[Self::Update]) -> bool;  // returns whether changed
    fn checkpoint(&self) -> Value;
    fn from_checkpoint(data: Value) -> Self;
}

/// Stores exactly one value. Error if multiple nodes write in same step.
struct LastValue<T> { ... }

/// Applies reducer: (T, T) -> T to merge multiple writes.
struct BinaryOperatorAggregate<T> { ... }

/// Cleared after each step.
struct EphemeralValue<T> { ... }
```

**Pregel execution engine (from LangGraph):**

```rust
struct Pregel<S> {
    nodes: Vec<PregelNode>,
    channels: HashMap<String, Box<dyn Channel>>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl<S> Pregel<S> {
    /// BSP execution loop
    async fn run(&self, input: S) -> Result<S> {
        loop {
            let tasks = self.plan();       // Which nodes to run?
            if tasks.is_empty() { break; }
            self.execute(tasks).await;      // Run them in parallel
            self.update();                  // Apply channel writes atomically
        }
    }
}
```

### AgentSession

Wraps `Agent` (from alva-core) or `CompiledGraph` (from graph execution) with orchestration:

```rust
struct AgentSession {
    agent: Agent,
    retry_config: RetryConfig,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}

impl AgentSession {
    fn new(agent: Agent) -> Self;
    fn with_retry(self, config: RetryConfig) -> Self;
    fn with_compaction(self, config: CompactionConfig) -> Self;
    fn with_checkpoint(self, saver: Box<dyn CheckpointSaver>) -> Self;

    async fn prompt(&self, messages: Vec<AgentMessage>) -> EventStream<AgentEvent>;
    async fn save_checkpoint(&self) -> Result<String>;
    async fn restore_checkpoint(&self, id: &str) -> Result<()>;
}
```

### Retry

```rust
struct RetryConfig {
    max_retries: u32,                        // default 3
    initial_delay_ms: u64,                   // default 1000
    max_delay_ms: u64,                       // default 30000
    retryable: Box<dyn Fn(&Error) -> bool>,  // is this error retryable?
}
```

### Compaction

```rust
struct CompactionConfig {
    max_tokens: usize,                      // trigger threshold
    keep_recent: usize,                     // keep last N messages
    model: Box<dyn LanguageModel>,          // summarization model (can be cheap)
}
```

After each `AgentEnd`, check token usage. If exceeded:
1. Extract old messages
2. LLM-generate summary
3. Replace old messages with `AgentMessage::Custom { type_name: "compaction_summary" }`

### Checkpoint

```rust
trait CheckpointSaver: Send + Sync {
    async fn save(&self, id: &str, state: &AgentState) -> Result<()>;
    async fn load(&self, id: &str) -> Result<Option<AgentState>>;
    async fn list(&self) -> Result<Vec<String>>;
    async fn delete(&self, id: &str) -> Result<()>;
}

struct InMemoryCheckpointSaver { ... }
```

SQLite/Postgres implementations live in separate crates (same pattern as LangGraph).

### Sub-Agent Scheduling

Sub-agents are tools — registered via alva-core's Tool trait. DeerFlow's `task_tool` pattern:

```rust
struct SubAgentConfig {
    name: String,
    description: String,
    system_prompt: String,
    model: SubAgentModel,               // Inherit | Specific
    tools: SubAgentTools,               // Inherit | Whitelist
    disallowed_tools: Vec<String>,      // default ["task"] — prevent recursion
    max_turns: u32,                     // default 50
    timeout: Duration,                  // default 15 min
}

enum SubAgentModel {
    Inherit,
    Specific(Box<dyn LanguageModel>),
}

enum SubAgentTools {
    Inherit,
    Whitelist(Vec<String>),
}
```

`create_task_tool()` creates a Tool that:
1. Looks up `SubAgentConfig` by `subagent_type` parameter
2. Spawns a new `alva-core::Agent` (isolated messages, shared sandbox)
3. Removes `disallowed_tools` (prevents recursive nesting)
4. Runs the sub-agent loop, collects result
5. Returns result as `ToolResult`

Double protection against recursion:
- Config: `disallowed_tools: ["task"]`
- Code: sub-agent created with `subagent_enabled: false`

### Context Transform Pipeline

```rust
trait ContextTransform: Send + Sync {
    fn transform(&self, messages: &[AgentMessage]) -> Vec<AgentMessage>;
}

struct TransformPipeline {
    transforms: Vec<Box<dyn ContextTransform>>,
}

impl TransformPipeline {
    fn push(&mut self, transform: Box<dyn ContextTransform>);
    fn apply(&self, messages: &[AgentMessage]) -> Vec<AgentMessage>;
}
```

Plugs into alva-core's `transform_context` hook.

---

## alva-app-core (slimmed down)

After extraction, alva-app-core keeps app-specific infrastructure:

```
crates/alva-app-core/src/
├── mcp/                    ← MCP protocol client (unchanged)
├── skills/                 ← Skill system (unchanged)
├── environment/            ← Runtime management: Bun/Python/Chromium (unchanged)
├── security/               ← Sandbox, HITL, permission guard (unchanged)
├── domain/                 ← App-specific domain types (Session, Workspace, etc.)
├── agent/
│   ├── agent_client/       ← ACP protocol (app-specific, stays)
│   ├── memory/             ← Embedding + SQLite memory (stays)
│   ├── persistence/        ← Session persistence + migrations (stays)
│   └── session/            ← Session management (stays)
├── types/                  ← App-specific shared types (stays)
├── bin/cli.rs              ← CLI binary (stays, updated imports)
└── error.rs
```

### What moves OUT of alva-app-core

| Current location | Destination | Notes |
|-----------------|-------------|-------|
| `agent/runtime/engine/` | alva-core | Loop, context manager |
| `agent/orchestrator/` | alva-graph | Orchestration logic |
| `domain/message.rs` | agent-base | `LLMMessage` types |
| `ports/provider/language_model.rs` | agent-base | `LanguageModel` trait |
| `ports/provider/types.rs` | agent-base | Provider types |
| `ports/provider/tool_types.rs` | agent-base | `LanguageModelTool`, `FunctionTool` |
| `ports/provider/prompt.rs` | agent-base | `LanguageModelMessage` (absorbed into `Message`) |
| `ports/provider/content.rs` | agent-base | `LanguageModelContent` (absorbed into `ContentBlock`) |
| `ports/provider/errors.rs` | agent-base | `ProviderError` |
| `ports/tool.rs` | agent-base | `Tool` trait + `ToolRegistry` |
| `adapters/llm/` | agent-base | OpenAI/Anthropic impls |
| `ui_message/` | agent-base | `UIMessage`, `UIMessagePart`, `UIMessageChunk` |
| `ui_message_stream/` | agent-base | Stream types |

### What stays in alva-app-core

**General rule**: anything not explicitly listed in "moves OUT" stays in alva-app-core.

| Module | Reason |
|--------|--------|
| `ports/provider/` (non-LLM: embedding, image, speech, etc.) | Move to agent-base later if needed |
| `ports/provider/provider_registry.rs` | `Provider` trait references both LLM and non-LLM — stays until non-LLM traits move |
| `ports/provider/middleware.rs` | Provider middleware stays with Provider trait |
| `ports/storage.rs` | `SessionStorage` trait — app-specific persistence |
| `agent/agent_client/` | ACP protocol (app-specific) |
| `agent/memory/` | Embedding memory (app-specific) |
| `agent/persistence/` | Session persistence + migrations |
| `agent/session/` | Session management |
| `agent/runtime/security/` | Permission guard (plugs into alva-core via `before_tool_call` hook) |
| `adapters/storage/` | In-memory/SQLite storage implementations |
| `domain/` (except `message.rs`) | Session, Agent config, Workspace types |
| `gateway/`, `base/`, `system/` | App infrastructure (stays or removed if unused) |
| `bin/cli.rs` | CLI binary, updated imports |

### Design decisions

**Orchestrator replacement**: The existing `agent/orchestrator/` (brain/reviewer/explorer architecture) is **replaced** by alva-graph's sub-agent model (DeerFlow's task tool pattern). The old orchestrator code is not adapted — it is removed when alva-graph is ready.

**SessionStorage decoupling**: The current engine loop calls `self.storage.append_message()` directly. In the new design, alva-core's `Agent` has no storage concept. Persistence is handled by subscribing to `AgentEvent::MessageEnd` in the upper layer (alva-app-core or alva-app), similar to pi-mono's coding-agent pattern.

**Provider trait split** (future): When non-LLM model traits eventually move to agent-base, the `Provider` trait will be split into `LanguageModelProvider` (agent-base) and `MultiModalProvider` (alva-app-core or agent-base). For now it stays in alva-app-core intact.

### Security integration pattern

alva-app-core's permission guard (`security/`) integrates with alva-core through the `before_tool_call` hook:

```rust
// In alva-app-core, when creating an Agent:
let permission_guard = PermissionGuard::new(config);
let agent_config = AgentConfig {
    before_tool_call: Some(Box::new(move |call, ctx| {
        if permission_guard.is_allowed(&call.name, &call.arguments) {
            ToolCallDecision::Allow
        } else {
            ToolCallDecision::Block { reason: "Permission denied".into() }
        }
    })),
    // ...
};
```

---

## Type Migration Details

### UIMessage / streaming types

The existing `UIMessage`, `UIMessagePart`, `UIMessageChunk`, `ChatStatus`, `ChatError` types currently live in alva-app-core and are used by srow-ai. These move to agent-base as part of the foundational message/stream system:

| Type | Current | Destination in agent-base |
|------|---------|--------------------------|
| `UIMessage` | `alva-app-core::ui_message` | `agent_base::message` (absorbed into `Message`) |
| `UIMessagePart` | `alva-app-core::ui_message::parts` | `agent_base::content` (absorbed into `ContentBlock`) |
| `UIMessageChunk` | `alva-app-core::ui_message_stream` | `agent_base::stream` (absorbed into `StreamEvent`) |
| `ChatStatus` | `srow-ai::chat` | `agent_base::types` or stays in alva-app (UI-specific) |
| `ChatError` | `srow-ai::chat` | `agent_base::error` |

The existing V4 provider interface (`LanguageModelCallOptions`, `LanguageModelMessage`) is superseded by the simpler `LanguageModel::complete(messages, tools, config)`. Existing provider implementations (OpenAI, Anthropic) are adapted to the new trait. This is a breaking change within the workspace — acceptable since all consumers are internal.

### ToolContext → hook injection

The existing `Tool::execute(input, ctx: &ToolContext)` where `ToolContext` carries `session_id`, `workspace`, `allow_dangerous` is simplified in agent-base to `Tool::execute(input, cancel)`. App-specific context is injected via closures when creating tools:

```rust
// In alva-app-core, wrapping a tool with app context:
struct AppToolWrapper {
    inner: Box<dyn Tool>,
    workspace: PathBuf,
    session_id: String,
}

impl Tool for AppToolWrapper {
    async fn execute(&self, input: Value, cancel: &CancellationToken) -> Result<ToolResult> {
        // Inject context, delegate to inner
    }
}
```

### ToolRegistry

`ToolRegistry` moves to agent-base alongside the `Tool` trait. It's a simple `HashMap<String, Box<dyn Tool>>` with name-based lookup — foundational infrastructure, not engine logic.

---

## alva-graph AgentSession — dual mode

`AgentSession` wraps either a linear `Agent` or a `CompiledGraph`:

```rust
enum SessionKind {
    Linear(Agent),
    Graph(CompiledGraph<AgentState>),
}

struct AgentSession {
    kind: SessionKind,
    retry_config: RetryConfig,
    compaction_config: Option<CompactionConfig>,
    checkpoint_saver: Option<Box<dyn CheckpointSaver>>,
}
```

Both modes share the same retry/compaction/checkpoint orchestration. The `prompt()` method dispatches to the appropriate execution path.

---

## Migration Strategy

Phase 1: Create agent-base (rename srow-ai, extract types/traits from alva-app-core, resolve circular dep)
Phase 2: Create alva-core (extract engine loop from alva-app-core)
Phase 3: Create alva-graph (new code + extract orchestrator from alva-app-core)
Phase 4: Slim alva-app-core (remove extracted code, update imports)
Phase 5: Update alva-app imports

Each phase produces a compiling workspace. No big-bang rewrite.

**Phase 1 critical path**: srow-ai currently depends on alva-app-core. To break the cycle:
1. Extract shared types (Message, Tool, LanguageModel, UIMessage, StreamEvent) into agent-base first
2. Both srow-ai (temporarily) and alva-app-core depend on agent-base
3. Then absorb remaining srow-ai code into agent-base and delete srow-ai

## Out of Scope

- Actual tool implementations (bash, read, edit, etc.) — stay in alva-app-core or alva-app
- Extension/plugin system — future work, alva-app layer
- UI changes — none needed for this restructure
- Non-LLM model traits (embedding, image, etc.) — stay in alva-app-core, move to agent-base later if needed
