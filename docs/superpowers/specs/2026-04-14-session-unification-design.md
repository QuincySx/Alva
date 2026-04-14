# Session Unification Design

**Date:** 2026-04-14
**Status:** Draft — awaiting review
**Scope:** Consolidate six parallel "session/history" subsystems into a single `AgentSession` abstraction.

---

## 1. Problem

The repository currently has **six** parallel subsystems that all record "what an agent did," and only one of them is actually wired into the runtime. This has produced:

- **Duplication.** The same events (turn boundaries, tool calls, LLM invocations) are either recorded in multiple shapes or not recorded at all, depending on which subsystem a consumer happens to look at.
- **Naming collisions.** Two different traits are both called `AgentSession`/`SessionAccess` and two different types are both called `InMemorySession`, in different modules.
- **Dead design.** `SessionAccess` was designed to be the unified event log but was never wired into the runtime write path, so every consumer that needed richer data built its own parallel system instead.
- **Tight coupling to specific apps.** `alva-app-eval` defines its own `RecorderMiddleware` + `RunStore` (SQLite) as private types; no other crate can read its data and eval cannot read anyone else's.

### Inventory of what exists today

| # | Component | Location | What it records | Wired into runtime? | Persistence |
|---|---|---|---|---|---|
| 1 | `AgentSession` trait + `InMemorySession` | `alva-kernel-abi/src/session.rs` | Linear `AgentMessage` history, `parent_id` for sub-agents | **Yes** — `kernel-core/src/run.rs` has 5 `state.session.append(...)` call sites and 2 reads | In-memory only; `flush`/`restore` are no-ops |
| 2 | `SessionAccess` trait + `SessionEvent` + `InMemorySession` (different type, same name) | `alva-kernel-abi/src/scope/context/{traits.rs, types.rs}` + `alva-agent-context/src/session.rs` | Append-only event log: user/assistant/tool_result/progress/system events, `parent_uuid` causal links, query/rollback/snapshot | **No** — zero production write sites; trait exists but is dead code outside its own unit tests | In-memory only |
| 3 | `SessionTracker` | `alva-agent-context/src/scope/session_tracker.rs` | In-memory sub-agent spawn tree (root → child agent relationships) | Marginal | None |
| 4 | `SessionStore` | `alva-app-cli/src/session_store.rs` | File-based JSON sessions (`.alva/sessions/{id}.json`), stores `AgentMessage` lists | CLI-private | JSON files |
| 5 | `RecorderMiddleware` + `RunRecord` | `alva-app-eval/src/recorder.rs` | Structured turn/llm_call/tool_call/hook/sub_run record, driven by middleware hooks | Eval-private | None (held in memory) |
| 6 | `RunStore` | `alva-app-eval/src/store.rs` | SQLite table `runs(run_id, model_id, turns, total_tokens, duration_ms, record_json, logs_json, created_at)` | Eval-private | SQLite (`alva-eval-runs.db`) |

Of these, only **#1** is actually wired into `run.rs`. **#2** is the one designed for the role we actually want ("record every interaction, every decision"), but it never got a production write point or a SQLite backend, so #3/#4/#5/#6 each reinvented a subset of it in isolation.

### What each of #3-#6 reinvented

- **#3** wants sub-agent spawn trees → `SessionEvent` already has `parent_uuid` and #2 already supports child sessions via the `SessionAccess` interface.
- **#4** wants "list history sessions + restore a conversation" → `SessionAccess.query(role=user|assistant)` plus a persistent backend covers it.
- **#5** and **#6** want full run audit + inspection + compare → every `RunRecord` field maps to a `SessionEvent` (`ConfigSnapshot` → `system` event, `LlmCallRecord` → `assistant_message` + `progress(llm_call_start/end)`, `ToolCallRecord` → `tool_use` + `tool_result`, `HookRecord` → `progress(hook)`, `sub_run` → child session with `parent_session_id`).

### Additional defect: the two `AgentSession`s are not the same thing

`AgentSession` (#1) is actually a **"live working message buffer"** — its only job is to answer "what messages do I put in the next LLM call?" It is not about audit, not about debug, not about history recovery. Its contract is essentially `append/messages/recent(n)/parent_id`. It was named `Session` historically, which collides with the true session concept (#2, `SessionAccess`).

Verification of #1's actual usage in production:

| Method | Production call sites | Status |
|---|---|---|
| `append(msg)` | `run.rs` × 5 | Alive — must migrate |
| `messages()` | `run.rs:466` | Alive — must migrate |
| `recent(n)` | `run.rs:464` | Alive — must migrate |
| `len()` / `is_empty()` | 0 | Dead |
| `parent_id()` | 0 production (only the trait's own tests) | Dead — `Scope::parent_id` is a different thing |
| `flush()` / `restore()` | 0 production (only tests) | Dead as implemented — but the **lifecycle signal** is needed (see §5) |
| `truncate(n)` / `retain(f)` / `clear()` | 0 | Dead |
| `impl AgentSession for ...` | Only `InMemorySession` in the same file | No other backends |

The real migration surface is small: 7 call sites in `run.rs`, one file to delete.

---

## 2. Goals and non-goals

### Goals

1. **One `AgentSession` trait** that serves as the single source of truth for everything that happens during an agent's lifetime: user/assistant/tool messages, runtime skeleton events, and component-emitted semantic events.
2. **Wired from day one** — the runtime writes to it inline, not via an optional middleware. Skeleton events (run/iteration/llm_call/tool boundaries) are recorded by kernel-core itself and are never missing.
3. **Third-party extension points write with zero ceremony** — tools, middleware, and extensions inherit a scoped session handle through their existing context types, so `ctx.session.append(SessionEvent::progress(...))` Just Works without any registration.
4. **Strict ordering** — events within a session are strictly ordered by a monotonically increasing `seq`, not by wall clock.
5. **Identifiable emitters** — every event carries the identity of who wrote it (`EventEmitter { kind, id, instance }`), so downstream parsing (eval, debug, replay) can classify and join events reliably.
6. **Pluggable backends** — in-memory default, SQLite for desktop persistence, easy path to file/remote backends later. Backends decide cache strategy; the trait defines only the contract.
7. **Delete, don't layer.** #1 and #3/#4/#5/#6 all go away. The trait exposed to runtime is the new unified `AgentSession`, full stop.

### Responsibility split — what core owns, what consumers own

This spec's central principle is **"recording is core, storage is a consumer choice."**

- **Core owns the recording.** The event model (`SessionEvent`, `EventEmitter`, `EmitterKind`), the runtime skeleton-event writes, the parent/child linkage, the monotonic `seq` ordering, the lifecycle contract — these are defined in the kernel and applied identically to every agent. A third-party consumer never writes its own recording code; it inherits recording for free.
- **Consumers choose their storage.** Core ships `InMemoryAgentSession` (default, tests) and `SqliteAgentSession` (persistent, ready to use) as backends. Any other backend — JSON files, a custom binary format, a remote service, an in-process event bus — is the consumer's own implementation, not shipped by core.
- **Core provides two extension points** so consumers don't reinvent recording when they just want custom storage or custom observation: (a) implementing the full `AgentSession` trait, for consumers that want their own authoritative storage; (b) implementing `SessionEventSink`, for consumers that just want to observe the event stream without owning storage. See §4.8.

### Why the eval vs CLI distinction matters

- **eval's current design is wrong in both dimensions.** It invented its own recording (`RecorderMiddleware` + `RunRecord`) AND its own storage (`RunStore` SQLite). The recording was wrong because it duplicated core's job. The storage was wrong because core already ships `SqliteAgentSession`. After migration, eval has zero persistence code and zero recording code — it is a pure projection frontend over `AgentSession`.
- **CLI's current design is half wrong.** The recording part (inventing its own `AgentMessage`-only session model) duplicated core's job and is wrong. But the storage choice (JSON files under `.alva/sessions/`) is a legitimate product decision for CLI. After migration, CLI's recording comes from core (via the unified event stream), and CLI continues to use JSON by implementing its own `JsonFileAgentSession` backend — which is a small extension over core's `AgentSession` trait, not a reinvention of anything.

### Non-goals

- **Not a logging replacement.** Tracing logs (structured text for humans) are a separate concern handled by `tracing` subscribers; they are not going through `AgentSession`. eval's `log_capture` module stays as-is.
- **Not a context manager.** `ContextStore` and the four-layer context model are untouched. Context *decisions* are recorded as session events (`context_compacted`, `context_externalized`), but the live working context itself remains in `ContextStore`.
- **Not a checkpoint system.** `alva-agent-graph`'s checkpoints are state snapshots for a state-machine engine, a separate concern.
- **Not a KV store for arbitrary app state.** If eval wants to store UI preferences they go somewhere else.
- **No automatic session consolidation across crashes.** A session that was open when the process died is whatever the backend durably persisted before the crash. The backend contract (`flush()`) defines the durability boundary.
- **No schema migrations in this spec.** SQLite backend will have a schema version column and an upgrade path, but the actual migration strategy is an implementation detail for the backend PR.

---

## 3. Architecture

```
                ┌──── AgentSession (single event stream, append-only)────┐
                │                                                         │
  write side:   │  runtime (kernel-core) — skeleton events                │
                │  tools (via ScopedSession in ToolExecutionContext)      │
                │  middleware (via ScopedSession in MiddlewareContext)    │
                │  extensions (via ScopedSession in their lifecycle ctx)  │
                │                                                         │
                └──┬──────────────┬──────────────┬────────────────────────┘
                   │              │              │
         ┌─────────▼──┐   ┌───────▼────┐   ┌─────▼────────────────┐
         │ message    │   │ context    │   │ eval / cli / debug   │
         │ view       │   │ view       │   │ view                 │
         │            │   │            │   │                      │
         │ append_msg │   │ live       │   │ projection of        │
         │ /messages/ │   │ ContextStr │   │ events into turns /  │
         │ recent_msg │   │ replaying  │   │ runs / history       │
         │            │   │ context    │   │                      │
         │ (runtime   │   │ mutation   │   │ (ad-hoc projection,  │
         │  hot path) │   │ events)    │   │  no stored duplicate)│
         └────────────┘   └────────────┘   └──────────────────────┘
```

### Invariants

1. **Single writer target.** Every piece of information worth recording is written to exactly one `AgentSession` instance for the session. Views are projections, never independent stores.
2. **Monotonic order.** Every event has a `seq: u64` assigned atomically at `append` time. `seq` is strictly increasing within a session; there are no duplicates and no gaps (except where `rollback_after` deliberately drops events).
3. **Emitter identity is structurally guaranteed.** Third-party code never fills in the `emitter` field manually — scoped-session wrappers inject it at construction time, so `emitter.kind` necessarily matches the actual call path.
4. **Runtime owns the skeleton.** Runtime always writes `run_start`, iteration boundaries, LLM-call boundaries, tool-use/tool-result pairs, and `run_end` events — regardless of which middleware or extensions are loaded.

---

## 4. Types

All of these live in the new file `alva-kernel-abi/src/agent_session.rs`. The existing `alva-kernel-abi/src/session.rs` is deleted. The existing `SessionAccess` trait and `SessionEvent`/`SessionMessage`/`EventQuery`/`EventMatch` types are moved from `alva-kernel-abi/src/scope/context/{traits.rs,types.rs}` into the new top-level file.

### 4.1 `SessionEvent`

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Strictly monotonic within a session. Assigned by the backend
    /// atomically at `append` time. Used for ordering; `timestamp` is
    /// only for human display.
    pub seq: u64,

    /// Unique id of this event.
    pub uuid: String,

    /// Causal parent (e.g. tool_result.parent_uuid = tool_use.uuid).
    pub parent_uuid: Option<String>,

    /// Wall-clock timestamp in epoch millis. Display only.
    pub timestamp: i64,

    /// Event type discriminator. Known values (see §4.4):
    /// "user" | "assistant" | "tool_use" | "tool_result" |
    /// "run_start" | "run_end" | "iteration_start" | "iteration_end" |
    /// "llm_call_start" | "llm_call_end" | "progress" | "system" |
    /// "context_compacted" | "context_externalized" | "component_registry"
    /// | other (forward-compat)
    #[serde(rename = "type")]
    pub event_type: String,

    /// Who wrote this event. Filled by the scoped session wrapper at
    /// the appropriate extension point; never by the third-party caller.
    pub emitter: EventEmitter,

    /// Present for message-bearing events (user/assistant/tool_result).
    pub message: Option<SessionMessage>,

    /// Present for non-message events. Arbitrary JSON payload.
    pub data: Option<serde_json::Value>,
}
```

### 4.2 `EventEmitter` and `EmitterKind`

```rust
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEmitter {
    pub kind: EmitterKind,

    /// Stable id within `kind`. Matches `ComponentDescriptor.id` in the
    /// session's registry event.
    pub id: String,

    /// Optional per-instance discriminator (e.g. multiple MCP servers
    /// with the same component id, or multiple concurrent instances).
    pub instance: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterKind {
    /// kernel-core runtime itself — skeleton events only.
    Runtime,
    /// A `Tool` during `Tool::execute`.
    Tool,
    /// A `Middleware` during one of its hook methods.
    Middleware,
    /// An `Extension` during its lifecycle or a service it provides.
    Extension,
    /// Escape hatch for future extension points not currently modeled.
    Other(String),
}
```

### 4.3 `ComponentDescriptor`

Registered once per session at `run_start` via a `component_registry` event. Subsequent events only carry the lightweight `EventEmitter`; readers join on `emitter.id` when they need the full descriptor.

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    pub kind: EmitterKind,
    pub id: String,      // matches EventEmitter.id
    pub name: String,    // human-readable display name
}
```

Deliberately minimal. Version, source, capabilities, etc. are not included — they are added as new fields when a concrete use case requires them. The Rust compiler will flag all construction sites, which is the review mechanism.

### 4.4 Event type vocabulary and `parent_uuid` contract

Runtime-emitted events form a strict tree:

| event_type | `parent_uuid` | Description |
|---|---|---|
| `run_start` | `None` | A new `prompt_text`/run begins. `data` carries config snapshot. |
| `component_registry` | `run_start.uuid` | Full list of `ComponentDescriptor` for this run. |
| `user` | `run_start.uuid` (or a previous assistant/tool_result) | User input message. |
| `iteration_start` | `run_start.uuid` (or previous `iteration_end`) | One loop iteration begins. |
| `llm_call_start` | `iteration_start.uuid` | Before the LLM request. `data` = input token count, message count. |
| `assistant` | `llm_call_start.uuid` | LLM response. `message` = full response. `data` = tokens, duration, stop_reason. |
| `llm_call_end` | `llm_call_start.uuid` | LLM call completed (success or error). |
| `tool_use` | `assistant.uuid` (or `llm_call_end.uuid`) | A tool call extracted from the assistant response. `data` = tool name, arguments. |
| `tool_result` | `tool_use.uuid` | Tool finished. `message` = result, `data` = duration, is_error. |
| `iteration_end` | `iteration_start.uuid` | One loop iteration completed. |
| `run_end` | `run_start.uuid` | The run is finished. `data` = total duration, error if any. |

Component-emitted events (from tools/middleware/extensions) set their own `parent_uuid` — typically the current `tool_use` or `llm_call_start`, or `None` for standalone events. The runtime does not police this; projections that care about the tree just follow parents wherever they point.

### 4.5 `AgentSession` trait

```rust
#[async_trait]
pub trait AgentSession: Send + Sync {
    // --- Identity ---

    fn session_id(&self) -> &str;

    /// Parent session id for sub-agents. `None` for root sessions.
    fn parent_session_id(&self) -> Option<&str>;

    // --- Write ---

    /// Append a raw event. Used by runtime and by any caller that needs
    /// to record something that is not a plain AgentMessage (progress,
    /// hook timing, custom component events, etc.).
    ///
    /// The backend:
    /// - assigns `event.seq` atomically,
    /// - persists the event according to its durability model,
    /// - updates any internal projections (e.g. message cache).
    async fn append(&self, event: SessionEvent);

    /// Append an `AgentMessage` as the appropriate event type
    /// (user/assistant/tool_result). The backend translates the message
    /// into a SessionEvent with `emitter.kind = Runtime` and appends it.
    ///
    /// This is the hot-path used by kernel-core to record conversation
    /// messages without constructing SessionEvents by hand.
    async fn append_message(&self, msg: AgentMessage);

    // --- Read: event-level ---

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    async fn count(&self, filter: &EventQuery) -> usize;

    // --- Read: message-level (hot path for LLM input assembly) ---

    /// All messages in append order, projected from events.
    /// Backends are expected to serve this from an internal cache.
    async fn messages(&self) -> Vec<AgentMessage>;

    /// Last N messages, projected from events.
    /// Backends are expected to serve this from an internal cache.
    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage>;

    // --- Write correction ---

    /// Drop all events with `seq` greater than the event identified by
    /// `uuid`. Used for undo, error recovery, and user-initiated rewind.
    /// Returns the number of events dropped.
    async fn rollback_after(&self, uuid: &str) -> usize;

    /// Store an opaque snapshot blob. Used by `ContextStore` to persist
    /// its four-layer state for restore across process restarts.
    async fn save_snapshot(&self, data: &[u8]);

    async fn load_snapshot(&self) -> Option<Vec<u8>>;

    // --- Lifecycle (timing signals from runtime to backend) ---

    /// Called once after this session object is constructed, before any
    /// other call. Idempotent. The backend should warm any internal
    /// projections (e.g. load the most recent events into the message
    /// cache) from durable storage. For a fresh session, this is a no-op.
    async fn restore(&self) -> Result<(), SessionError>;

    /// Called at strategic points by the runtime:
    /// 1. on `on_agent_end` (run completion),
    /// 2. periodically during long runs (every N iterations or M seconds,
    ///    exact policy determined by kernel-core),
    /// 3. before process shutdown.
    ///
    /// The backend MUST ensure all pending writes are durably persisted
    /// before the returned future resolves. For in-memory backends this
    /// is a no-op; for SQLite it must commit any open transaction and
    /// fsync as appropriate.
    async fn flush(&self) -> Result<(), SessionError>;

    /// Called when the session is no longer going to be used. Implies a
    /// `flush()` followed by release of any held resources (file handles,
    /// network subscriptions, cache memory). After `close()`, the session
    /// object is not safe to use.
    async fn close(&self) -> Result<(), SessionError>;

    /// Drop all events and snapshots for this session. Used for explicit
    /// "reset this conversation" and in tests. Not for routine cleanup.
    async fn clear(&self) -> Result<(), SessionError>;
}
```

### 4.6 `ScopedSession`

A wrapper that makes a session handle look like an `AgentSession` but automatically stamps every appended event with a fixed `EventEmitter`. Third-party code only ever sees `ScopedSession`, never the raw trait object, so it has no way to set `emitter` incorrectly.

```rust
pub struct ScopedSession {
    inner: Arc<dyn AgentSession>,
    emitter: EventEmitter,
}

impl ScopedSession {
    pub fn new(inner: Arc<dyn AgentSession>, emitter: EventEmitter) -> Self {
        Self { inner, emitter }
    }

    pub async fn append(&self, mut event: SessionEvent) {
        event.emitter = self.emitter.clone();
        self.inner.append(event).await;
    }

    // Read-side methods delegate without modification.
    pub async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.inner.query(filter).await
    }

    pub async fn count(&self, filter: &EventQuery) -> usize {
        self.inner.count(filter).await
    }

    // Intentionally does NOT expose `append_message` — message writes are
    // the runtime's job. Tools/middleware/extensions write SessionEvents,
    // not AgentMessages.
}
```

### 4.8 `SessionEventSink` — lightweight observation extension point

For consumers who want to observe the event stream without owning storage (analytics exporters, live UIs subscribing to events, third-party loggers), the full `AgentSession` trait is overkill. They do not need to implement `query` / `rollback_after` / `save_snapshot` / the full lifecycle — they just want "events as they happen."

```rust
#[async_trait]
pub trait SessionEventSink: Send + Sync {
    /// Called for every event appended to the wrapped session.
    /// The sink MUST NOT block; long-running work should be dispatched
    /// to a background task.
    async fn on_event(&self, session_id: &str, event: &SessionEvent);

    /// Called when the wrapped session's `flush()` is called. The sink
    /// should treat this as a hint to persist any buffered data it holds.
    async fn on_flush(&self, session_id: &str) {}

    /// Called when the wrapped session is closed.
    async fn on_close(&self, session_id: &str) {}
}
```

Core provides a wrapper that tees appends to any number of registered sinks:

```rust
pub struct TeeAgentSession {
    inner: Arc<dyn AgentSession>,
    sinks: Vec<Arc<dyn SessionEventSink>>,
}

impl TeeAgentSession {
    pub fn new(inner: Arc<dyn AgentSession>, sinks: Vec<Arc<dyn SessionEventSink>>) -> Self { ... }
}

#[async_trait]
impl AgentSession for TeeAgentSession {
    async fn append(&self, event: SessionEvent) {
        self.inner.append(event.clone()).await;
        for sink in &self.sinks {
            sink.on_event(self.inner.session_id(), &event).await;
        }
    }

    async fn append_message(&self, msg: AgentMessage) {
        // Delegates to `append` via the inner session's translation;
        // sinks see the resulting SessionEvent.
        self.inner.append_message(msg).await;
        // Note: this one's slightly trickier — inner translated without
        // notifying sinks. The concrete impl will either re-read the
        // just-appended event by seq and notify sinks, or route through
        // append() explicitly. Detail for the implementation PR.
    }

    async fn flush(&self) -> Result<(), SessionError> {
        self.inner.flush().await?;
        for sink in &self.sinks { sink.on_flush(self.inner.session_id()).await; }
        Ok(())
    }

    // ... other methods delegate to inner and notify sinks where appropriate ...
}
```

When to use which extension point:

| Need | Use |
|---|---|
| "I just want to see the events and do X with each" | Implement `SessionEventSink`. Wrap the agent's session with `TeeAgentSession::new(base, vec![my_sink])`. |
| "I own the storage — events live in MY database/files/service" | Implement `AgentSession` directly. Use `InMemoryAgentSession` / `SqliteAgentSession` as reference templates. |
| "I want persistence with zero code" | Use `SqliteAgentSession` as-is. Ships with core. |
| "I want no persistence" | Use `InMemoryAgentSession` as-is. Ships with core. |

### 4.7 Other types (moved from existing `scope/context` unchanged)

`SessionMessage`, `EventQuery`, `EventMatch`, and the `SessionEvent` constructors (`user_message`, `assistant_message`, `tool_result`, `progress`, `system`) move from `alva-kernel-abi/src/scope/context/types.rs` to the new `alva-kernel-abi/src/agent_session.rs`. Their signatures are unchanged except that the constructors now take an `EventEmitter` parameter (to match §4.1) — in practice, callers will use the `ScopedSession.append(event)` helper, which fills emitter automatically, so most call sites that currently build events by hand will go away.

`SessionError` is a new error type (not previously defined) for the lifecycle methods. A small enum covering `IoError(io::Error)`, `SerializationError(serde_json::Error)`, `NotFound`, `Other(String)`.

---

## 5. Lifecycle contract

The lifecycle methods on `AgentSession` (`restore` / `flush` / `close` / `clear`) exist specifically because persistence and caching decisions depend on timing signals that only the runtime knows. This section is the authoritative contract.

### 5.1 Runtime obligations

| Method | Runtime calls it when | Backend MUST |
|---|---|---|
| `restore()` | Exactly once, after the session object is constructed, before any other call. | Warm internal projections (message cache, snapshot cache) from durable storage. Idempotent for fresh sessions. Return `Err` if storage is corrupted. |
| `flush()` | **Three times, deterministically:** (a) at `on_agent_end` for every run, (b) every `flush_interval` during long-running runs (default: every 10 iterations OR 30 seconds, whichever first; configurable on `AgentConfig`), (c) once during graceful process shutdown. | Ensure all previously-appended events and snapshots are durably persisted before the future resolves. After `flush()` returns, a process crash MUST NOT lose any event that was appended before `flush()` started. |
| `close()` | When the session is no longer going to be used (user ends the conversation, agent owner drops the session). | Perform a `flush()`, then release all resources (file handles, network subscriptions, cache memory). After `close()`, the session object MUST return an error from any subsequent method. |
| `clear()` | Only on explicit user-initiated "reset this conversation" or in tests. **Never** called by the runtime during normal operation. | Drop all events and snapshots for this session. If durable, delete from storage. |

### 5.2 Backend obligations

Each backend implementation MUST document its concrete durability semantics:

- **`InMemoryAgentSession`**: all methods are no-ops except that `clear()` actually empties the vector. No persistence; `flush` and `close` are no-ops; `restore` is a no-op.
- **`SqliteAgentSession`**: `append` writes into a WAL-backed SQLite file in its own transaction. `flush` runs `PRAGMA wal_checkpoint(FULL)`. `restore` opens the connection, loads the most recent K events into the message cache (K = `cache_warm_size`, default 256), and verifies the schema version. `close` calls `flush` then closes the connection. `clear` deletes all rows for `session_id` and the associated snapshot.

### 5.3 Why these hooks are required

Without `restore`, the backend has no signal that "now is the right time to load cached data" — it would have to either load eagerly on construction (wasteful for never-used sessions) or lazily on first read (first-read latency spike). Runtime knows when it is about to start using a session.

Without `flush`, the backend has to either fsync on every append (correct but slow) or never fsync (fast but loses data on crash). Runtime knows the three correct checkpoint moments (agent end, long-run intervals, process shutdown) and signals them explicitly.

Without `close`, the backend has to rely on `Drop` for resource release, which in Rust cannot perform async work reliably. An explicit async `close()` is the only way to do a graceful flush-then-release sequence.

---

## 6. Write-side integration: 8A+

The runtime writes skeleton events inline in `kernel-core/src/run.rs`. Third-party extension points write their own semantic events via `ScopedSession` handles that are injected into their existing context types.

### 6.1 Runtime-written skeleton events

In `alva-kernel-core/src/run.rs`, kernel-core writes the following events with `emitter = EventEmitter { kind: EmitterKind::Runtime, id: "kernel_core", instance: None }`:

- At the start of `run_loop`: `run_start` + `component_registry` (carrying the `ComponentDescriptor` list for all tools, middleware, and extensions in this agent's configuration).
- For each loop iteration: `iteration_start` ... `iteration_end`.
- Around each LLM call: `llm_call_start` + `assistant` (on response) + `llm_call_end`.
- For each tool call: `tool_use` + `tool_result` (with `tool_result.parent_uuid = tool_use.uuid`).
- On any user input arriving (initial prompt, steering, context-hook injection): a `user` event.
- At the end of `run_loop`: `run_end`.

These are the only events runtime writes. Runtime does not emit `progress` or `system` events for things it cannot directly observe (e.g., plugin internal decisions).

### 6.2 Context type changes for injection

Three context types get a session handle so that tools/middleware/extensions can write their own events without any registration:

#### `ToolExecutionContext` (trait, in `alva-kernel-abi/src/tool/execution.rs`)

```rust
pub trait ToolExecutionContext: Send + Sync {
    // ... existing methods ...

    /// Scoped session handle for this tool invocation. Every event
    /// appended through this handle is automatically stamped with
    /// `EmitterKind::Tool` and the tool's registered id.
    fn session(&self) -> &ScopedSession;
}
```

The runtime wires this up by creating a fresh `ScopedSession { inner, emitter: EventEmitter { kind: Tool, id: tool.name, instance: None } }` for each tool invocation and passing it through the context implementation.

#### `MiddlewareContext` (struct, in `alva-kernel-core/src/middleware.rs`)

```rust
pub struct MiddlewareContext {
    pub bus: Option<BusHandle>,
    pub workspace: Option<PathBuf>,
    /// NEW: scoped session for this middleware. `None` only if the
    /// agent was built without an AgentSession (tests).
    pub session: Option<ScopedSession>,
}
```

Each middleware's hook is given its own `ScopedSession` stamped with `EmitterKind::Middleware` and the middleware's `name()`.

#### Extension lifecycle

Extensions do not currently have a uniform runtime context. They get one as part of this work: any `Extension` trait method that receives a context struct (`configure` / `finalize` / service-provision callbacks) includes a `ScopedSession` for that extension. An extension that wants to record events during its lifecycle uses it; one that doesn't, ignores it.

### 6.3 What third-party code looks like

```rust
// A tool that wants to record a custom event:
async fn execute(&self, args: &Args, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput> {
    ctx.session().append(SessionEvent::progress(serde_json::json!({
        "sub_operation": "parsing_input",
    }))).await;
    // ... real work ...
}
```

No registration, no middleware stack position, no discovery. The tool doesn't know or care what `EmitterKind` is assigned to its events — that's baked in when the runtime constructed the context.

---

## 7. What gets deleted, what gets rewritten

| # | Current | Action | Detail |
|---|---|---|---|
| 1 | `alva-kernel-abi/src/session.rs` (old `AgentSession` + old `InMemorySession`) | **Delete entire file** | 7 call sites in `run.rs` migrate to the new trait (see §8 step 5). No other impls exist. |
| 2 | `SessionAccess` trait + `SessionEvent` types in `alva-kernel-abi/src/scope/context/` + `InMemorySession` in `alva-agent-context/src/session.rs` | **Move and rename** | Move to `alva-kernel-abi/src/agent_session.rs`. `SessionAccess` → `AgentSession`. `InMemorySession` → `InMemoryAgentSession`. Everything inside the trait gets augmented per §4.5 and §5. |
| 3 | `alva-agent-context/src/scope/session_tracker.rs` (`SessionTracker`) | **Delete file** | Sub-agent linking is covered by `parent_session_id` on `AgentSession`. Confirm no production callers before deletion. |
| 4 | `alva-app-cli/src/session_store.rs` (`SessionStore`) | **Rewrite, not delete** | CLI has two valid paths and chooses one: **(a)** drop the existing JSON format and use the shipped `SqliteAgentSession` — simplest, ~20 lines of CLI code change; **(b)** keep the JSON format by implementing `JsonFileAgentSession: AgentSession` — a ~200-line custom backend modeled on `InMemoryAgentSession`. Either way, CLI's recording logic is gone — it no longer invents events, it consumes the unified event stream. Which path CLI takes is a CLI-product decision, not a spec decision. The spec guarantees both paths work equivalently. |
| 5 | `alva-app-eval/src/recorder.rs` (`RecorderMiddleware` + `RunRecord`) | **Delete file** | The full `RecorderMiddleware` is not needed because runtime writes skeleton events directly. `RunRecord` is replaced by a pure projection function in a new `alva-app-eval/src/projection.rs` that turns `&[SessionEvent]` into whatever shape the eval UI renders. Eval writes zero recording code after migration — recording is entirely core's job. |
| 6 | `alva-app-eval/src/store.rs` (`RunStore`, SQLite table `runs`) | **Delete file** | Replaced by the shipped `SqliteAgentSession` backend in `alva-app-core`. Eval wires its API endpoints to an `AgentSession` handle it obtains from core's session registry — it writes zero storage code. A dedicated `alva-agent-session-sqlite` crate can be extracted later if more than one backend needs to share the same sqlite machinery; YAGNI until then. |

### eval after the change

`alva-app-eval` keeps:

- `main.rs` HTTP/SSE routing, embedded UI, state management
- `log_capture.rs` (tracing log capture — orthogonal to sessions)
- `skills.rs` (skill discovery — orthogonal)
- `child_recording.rs` — **DELETE** if sub-agents use independent child sessions with `parent_session_id` (likely). Keep only if there's a reason child recording can't go through the session event stream.

and gains:

- `projection.rs` — pure functions `events: &[SessionEvent] -> RunView` where `RunView` is whatever the frontend renders.

eval's API endpoints become thin wrappers:

- `POST /api/run` — creates a new session, starts the run, returns session_id
- `GET /api/events/:session_id` — SSE stream of live events (subscribes to the session's event stream)
- `GET /api/records/:session_id` — reads events from `AgentSession`, runs `projection::build_run_view(...)`, returns JSON
- `GET /api/runs` — lists sessions; see §7.1 for the listing mechanism
- `GET /api/compare/:a/:b` — runs two projections, diffs them
- `GET /api/logs/:session_id` — unchanged; reads from `log_capture`

### 7.1 Listing sessions — where it lives

An `AgentSession` instance represents exactly one session. Listing sessions across an entire workspace (for eval's `/api/runs` and CLI's history view) is a separate concern that requires a **`SessionRegistry`** — a small trait that knows how to enumerate and load sessions from a given backend.

```rust
#[async_trait]
pub trait SessionRegistry: Send + Sync {
    /// List sessions known to this registry, newest first.
    async fn list(&self) -> Vec<SessionSummary>;

    /// Open an existing session by id. Returns `None` if the session
    /// does not exist in this registry.
    async fn open(&self, session_id: &str) -> Option<Arc<dyn AgentSession>>;

    /// Create a new session in this registry and return its handle.
    async fn create(&self, parent_session_id: Option<&str>) -> Arc<dyn AgentSession>;

    /// Delete a session permanently.
    async fn delete(&self, session_id: &str) -> Result<(), SessionError>;
}

pub struct SessionSummary {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub event_count: usize,
    pub preview: String,  // first user message, truncated
}
```

`SqliteSessionRegistry` is the concrete implementation that owns a SQLite connection and constructs `SqliteAgentSession` handles on demand, sharing the underlying file. eval and CLI both depend on the `SessionRegistry` trait, not on the concrete SQLite type.

Deliberately minimal. No filtering, tagging, archiving, or search — those are added only when a concrete use case appears.

---

## 8. Migration plan

Each step must leave the workspace in a compiling, test-passing state.

### Step 1 — Create the new file

Create `alva-kernel-abi/src/agent_session.rs`. Copy `SessionAccess` trait + `SessionEvent` + `SessionMessage` + `EventQuery` + `EventMatch` from the existing `scope/context/` files. Rename `SessionAccess` → `AgentSession`. Add §4.1 `seq` field to `SessionEvent`. Add §4.2 `EventEmitter` / `EmitterKind` types. Add §4.3 `ComponentDescriptor`. Add §4.6 `ScopedSession`. Add §5 lifecycle methods (`restore`, `flush`, `close`, `clear`) with `SessionError` type.

Add new message-level methods (`append_message`, `messages`, `recent_messages`) to the trait. No default implementations — each backend writes its own per §7.

Re-export `AgentSession` from `alva-kernel-abi::lib` alongside the existing `SessionAccess` re-export so callers can migrate incrementally.

### Step 2 — In-memory backend

Rename `alva-agent-context/src/session.rs`'s `InMemorySession` to `InMemoryAgentSession`. Extend it to implement the full new trait. Add the internal `VecDeque<AgentMessage>` message cache, the `AtomicU64` seq counter, and the seven new/updated methods.

Write unit tests covering:
- `seq` is strictly monotonic under concurrent append (1000 concurrent appends, verify `0..1000` strictly increasing)
- `append_message` translates to the correct event type and updates the cache
- `recent_messages(n)` serves from cache in O(n)
- `rollback_after` drops events above the target seq
- Lifecycle methods are no-ops but don't error

Move `InMemoryAgentSession` into `alva-kernel-abi/src/agent_session.rs` alongside the trait; delete `alva-agent-context/src/session.rs`.

### Step 3 — Wire `ScopedSession` into context types

Update `ToolExecutionContext` trait in `alva-kernel-abi/src/tool/execution.rs` to require `fn session(&self) -> &ScopedSession`. Update all implementors (search for `impl ToolExecutionContext for`). Provide the scoped session at the point where tools are invoked in `run.rs`.

Update `MiddlewareContext` struct in `alva-kernel-core/src/middleware.rs` to include `session: Option<ScopedSession>`. Update `configure_all` in `agent_builder.rs` and the middleware hook call sites in `run.rs` to construct the scoped session per middleware.

For `Extension`, audit which lifecycle methods take a context struct and add the session there. This step may require defining a small `ExtensionContext` struct if one does not already exist.

### Step 4 — Add skeleton events to runtime

In `alva-kernel-core/src/run.rs`, insert the §6.1 events at the appropriate points. These writes use the raw `AgentSession` (not a scoped wrapper) and explicitly set `emitter = Runtime`.

Before starting a run, collect all tools/middleware/extensions into a `Vec<ComponentDescriptor>` and emit the `component_registry` event.

### Step 5 — Migrate `run.rs` call sites

Change `AgentState.session` type from `Arc<dyn old::AgentSession>` to `Arc<dyn new::AgentSession>`. Change the 7 existing call sites:

- `state.session.append(msg)` × 5 → `state.session.append_message(msg).await`
- `state.session.messages()` × 1 → `state.session.messages().await`
- `state.session.recent(context_window)` × 1 → `state.session.recent_messages(context_window).await`

Update `run_child.rs` similarly.

### Step 6 — Delete old trait

Delete `alva-kernel-abi/src/session.rs`. Remove its `mod session` from `lib.rs`. Fix compile errors (there should be few — test helpers, `AgentState` construction in tests).

### Step 7 — Delete duplicate subsystems

In separate commits:

- Delete `alva-agent-context/src/scope/session_tracker.rs`. Remove references.
- Delete `alva-app-cli/src/session_store.rs`. Rewrite CLI session listing to use `AgentSession` directly.
- Delete `alva-app-eval/src/recorder.rs`. Write `alva-app-eval/src/projection.rs`.
- Delete `alva-app-eval/src/store.rs`. Write `SqliteAgentSession` in a shared location and wire eval to use it.
- Delete `alva-app-eval/src/child_recording.rs` (if redundant per §7).

### Step 8 — SQLite backend

Implement `SqliteAgentSession` per §5.2. Schema:

```sql
CREATE TABLE sessions (
    session_id        TEXT PRIMARY KEY,
    parent_session_id TEXT,
    created_at        INTEGER NOT NULL,
    schema_version    INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE events (
    session_id        TEXT NOT NULL,
    seq               INTEGER NOT NULL,
    uuid              TEXT NOT NULL,
    parent_uuid       TEXT,
    timestamp         INTEGER NOT NULL,
    event_type        TEXT NOT NULL,
    emitter_kind      TEXT NOT NULL,
    emitter_id        TEXT NOT NULL,
    emitter_instance  TEXT,
    message_json      TEXT,
    data_json         TEXT,
    PRIMARY KEY (session_id, seq)
);

CREATE INDEX idx_events_type  ON events(session_id, event_type);
CREATE INDEX idx_events_uuid  ON events(uuid);
CREATE INDEX idx_events_parent ON events(parent_uuid);

CREATE TABLE snapshots (
    session_id TEXT PRIMARY KEY,
    data       BLOB NOT NULL,
    updated_at INTEGER NOT NULL
);
```

Run the same unit-test suite from Step 2 against the SQLite backend to verify trait conformance.

### Step 9 — Default backend wiring

Set `InMemoryAgentSession` as the library default in `BaseAgent::builder()` — this is what tests and minimal embeddings get. Apps that want persistence explicitly pass `SqliteAgentSession` at build time (eval does this directly; CLI chooses between `SqliteAgentSession` and its own `JsonFileAgentSession` depending on which migration path CLI takes in step 7).

Core ships `InMemoryAgentSession` + `SqliteAgentSession` + `SessionEventSink` trait + `TeeAgentSession`. That is the complete set. No JSON backend, no remote backend, no file-per-session backend — those are consumer extensions.

---

## 9. Risks and trade-offs

**Async hot path.** Every call in `run.rs` that previously touched `state.session` becomes `.await`. `run_loop` is already `async fn`, so this is mechanical, but it does mean the `.await` points multiply. SQLite backend must make `append` fast (in-memory buffer with batched writes, or at least transaction-per-append with WAL) to avoid introducing per-message latency.

**Cache invalidation under rollback.** When `rollback_after` drops events, backends that hold a message cache must also roll back their cache. The InMemory backend does this by replaying. SQLite backend does this by reloading from disk after the rollback transaction commits. Either way, rollback is not free — spec'd as a non-hot-path operation.

**Schema migration.** The SQLite schema above is v1. Future changes will require a schema version column (included above) and a migration function. Not spec'd in detail here because v1 does not need it yet, but the column is present to enable the mechanism later.

**`ScopedSession` over-helpful wrapper.** By preventing third-party code from setting `emitter`, we also prevent it from _forwarding_ events from nested sources (e.g., an MCP tool wrapper passing through events from the underlying MCP server with the MCP's own emitter). If this use case materializes, we add an explicit `ScopedSession::forward(event)` method that preserves the provided emitter but validates its `kind` against an allowlist. Not spec'd for the first version — YAGNI until a concrete need arises.

**Eval UI migration.** Eval's current API returns a `RunRecord` shape. After migration, the projection function must produce the same (or close to same) shape so the frontend keeps working. This is a pure data-transform task and is tracked in step 7.

**Middleware hook ordering vs skeleton events.** Runtime writes `llm_call_start` *before* running `before_llm_call` middleware, and `llm_call_end` *after* running `after_llm_call` middleware. This means middleware-emitted events (e.g., loop detection triggering) land between the two skeleton events, which is the correct order for projections that rebuild the turn structure. Document this timing in the trait's doc comments.

**Deleting `SessionTracker` with zero audit.** Spec'd as "confirm no production callers before deletion." If audit finds a caller, that caller's functionality must be covered by the new trait before deletion.

---

## 10. Success criteria

1. `alva-kernel-abi/src/session.rs` no longer exists.
2. `alva-agent-context/src/scope/session_tracker.rs`, `alva-app-cli/src/session_store.rs`, `alva-app-eval/src/recorder.rs`, `alva-app-eval/src/store.rs` no longer exist.
3. `alva-kernel-core/src/run.rs` writes the §6.1 skeleton events; a fresh run without any middleware or extensions still produces a complete event stream (`run_start` → ... → `run_end`) in the session.
4. A new integration test creates an agent with a `SqliteAgentSession`, runs a prompt that invokes two tools, kills the process, reopens the session, and verifies all events (including both tool calls and the assistant response) are recovered in order.
5. eval's `/api/records/:session_id` returns the same shape as before (for frontend compatibility), built by `projection.rs` over events, with no `RecorderMiddleware` or `RunStore` involved.
6. CLI's session listing (`/sessions`) uses `AgentSession.query(...)` via a SQLite backend; deleting `.alva/sessions/` makes zero difference to the CLI's behavior (because that directory no longer exists).
7. A third-party tool in the test suite writes a `progress` event via `ctx.session()` without touching `EventEmitter` directly, and the recorded event has `emitter.kind = Tool`, `emitter.id = <tool_name>`.
