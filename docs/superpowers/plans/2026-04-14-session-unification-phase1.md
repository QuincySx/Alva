# Session Unification Phase 1 — Core Foundation Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the legacy `AgentSession` (a thin `AgentMessage` buffer) with a new unified `AgentSession` trait that is the single source of truth for everything an agent does during a run. After this plan, the runtime writes skeleton events (`run_start`, `iteration_start/end`, `llm_call_start/end`, `tool_use`, `tool_result`, `run_end`) into the session inline, third-party tools and middleware receive a `ScopedSession` through their existing context types, and the old `alva-kernel-abi/src/session.rs` file is deleted.

**Architecture:** Introduce the new trait + types in a new file `alva-kernel-abi/src/agent_session.rs` as additive work (old trait untouched). Implement `InMemoryAgentSession` with a monotonic `seq` counter, message cache, and full lifecycle. Extend `ToolExecutionContext` and `MiddlewareContext` with a scoped session handle. Then do one atomic switchover commit that changes `AgentState.session` to the new trait, migrates `run.rs`'s 5 append sites + 2 read sites, and deletes the old file. Add skeleton event writes to `run.rs` after the switchover.

**Tech Stack:** Rust, tokio, async-trait, serde, uuid, chrono, existing workspace conventions.

**Spec:** `docs/superpowers/specs/2026-04-14-session-unification-design.md`

**Out of scope for this plan** (follow-up plans will cover these):
- SQLite backend (`SqliteAgentSession`)
- `SessionRegistry` trait + implementations
- `SessionEventSink` trait + `TeeAgentSession`
- Deletion of `SessionTracker`, `alva-app-cli/src/session_store.rs`, `alva-app-eval/src/{recorder.rs,store.rs,child_recording.rs}`
- Eval and CLI migrations

---

## File Structure

### New files

- **`crates/alva-kernel-abi/src/agent_session.rs`** — The new home for everything session-related. Contains the `AgentSession` trait, `SessionEvent` + related types, `EventEmitter` + `EmitterKind`, `ComponentDescriptor`, `ScopedSession`, `SessionError`, `InMemoryAgentSession`, and their unit tests. Target: ~800 lines. Single file because all the pieces are tightly coupled and designed together; splitting would harm readability.

### Modified files

- **`crates/alva-kernel-abi/src/lib.rs`** — Remove `pub mod session;` and `pub use session::{AgentSession, InMemorySession};`. Add `pub mod agent_session;` and `pub use agent_session::{AgentSession, InMemoryAgentSession, SessionError, SessionEvent, SessionMessage, EventEmitter, EmitterKind, ComponentDescriptor, ScopedSession, EventQuery, EventMatch};`.
- **`crates/alva-kernel-abi/src/tool/execution.rs`** — Extend `ToolExecutionContext` trait with `fn session(&self) -> Option<&ScopedSession>` (default: `None`). Update `MinimalExecutionContext` impl explicitly (still returns `None`). Update module-level `// OUTPUT:` comment.
- **`crates/alva-kernel-core/src/middleware.rs`** — Add `pub session: Option<ScopedSession>` field to `MiddlewareContext` struct. Update any in-crate constructors.
- **`crates/alva-kernel-core/src/state.rs`** — Change `pub session: Arc<dyn AgentSession>` to use the new trait. Update the `InMemorySession` import in the `#[cfg(test)]` block to `InMemoryAgentSession`.
- **`crates/alva-kernel-core/src/run.rs`** — Migrate 5 `state.session.append(msg)` sites to `state.session.append_message(msg).await`; migrate `state.session.messages()` and `state.session.recent(n)` to `.messages().await` and `.recent_messages(n).await`; add skeleton event writes (`run_start`, `component_registry`, `iteration_start/end`, `llm_call_start/end`, `tool_use`, `tool_result`, `run_end`). Construct `ScopedSession` instances when producing `ToolExecutionContext` and `MiddlewareContext`.
- **`crates/alva-kernel-core/src/run_child.rs`** — Same call-site migration for sub-agent runs.
- **`crates/alva-kernel-core/src/builtins/test_helpers.rs`** — Update `make_state()` to construct `InMemoryAgentSession` and update imports.
- **`crates/alva-kernel-core/tests/integration.rs`** — Update any `InMemorySession` usage to `InMemoryAgentSession`.
- **`crates/alva-kernel-core/src/state.rs` tests, examples, and other callers of `InMemorySession`** — Same. Scope: any file that currently has `use alva_kernel_abi::session::InMemorySession` or equivalent must switch to the new type. The grep command in Task 8 enumerates them precisely.

### Deleted files

- **`crates/alva-kernel-abi/src/session.rs`** — deleted in Task 9. Contents fully replaced by `agent_session.rs`.

### Files NOT touched in this plan

- `alva-agent-context/src/session.rs` — still exposes the old `SessionAccess` from `scope::context`. Left in place until Phase 2 merges it. This means there will be a temporary period of both `SessionAccess` (in agent-context) and `AgentSession` (new, in kernel-abi) coexisting. They do not conflict because they live in different modules and are not directly interconnected.
- `alva-app-eval/src/*`, `alva-app-cli/src/session_store.rs`, `alva-agent-context/src/scope/session_tracker.rs` — all handled in Phase 3.

---

## Task 1: Create new module scaffold + value types

**Files:**
- Create: `crates/alva-kernel-abi/src/agent_session.rs`
- Modify: `crates/alva-kernel-abi/src/lib.rs`

This task adds the new file with value types only (no trait, no impl yet). It is purely additive — nothing in the existing codebase imports from the new module, so this commit alone does not affect any other crate.

- [ ] **Step 1.1: Create `agent_session.rs` with header and value types**

Create `crates/alva-kernel-abi/src/agent_session.rs` with exactly this content:

```rust
// INPUT:  async_trait, serde, serde_json, uuid, chrono, tokio, std, crate::AgentMessage
// OUTPUT: AgentSession trait, InMemoryAgentSession, SessionEvent, SessionMessage,
//         EventEmitter, EmitterKind, ComponentDescriptor, ScopedSession,
//         SessionError, EventQuery, EventMatch
// POS:    Unified session abstraction — the single source of truth for everything
//         an agent does during a run. Replaces the legacy message-buffer-only
//         AgentSession that used to live in src/session.rs.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::base::message::AgentMessage;

// ===========================================================================
// Errors
// ===========================================================================

/// Errors that can be returned from `AgentSession` lifecycle methods.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session error: {0}")]
    Other(String),
}

// ===========================================================================
// Emitter identity
// ===========================================================================

/// Identifies who wrote a session event.
///
/// The `kind` is set by the extension point (Runtime / Tool / Middleware /
/// Extension); the `id` is the stable name of the concrete component within
/// that kind (e.g. `read_file` for a tool, `loop_detection` for a middleware).
/// Third-party code NEVER sets these fields directly — the scoped session
/// wrapper injects them when events are appended through it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEmitter {
    pub kind: EmitterKind,
    pub id: String,
    pub instance: Option<String>,
}

impl EventEmitter {
    /// Runtime emitter used for kernel-core skeleton events.
    pub fn runtime() -> Self {
        Self {
            kind: EmitterKind::Runtime,
            id: "kernel_core".to_string(),
            instance: None,
        }
    }
}

/// Base categories for event emitters. Use `Other` for future extension points
/// that do not fit into the existing kinds — once a new kind stabilizes it can
/// be promoted to a named variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterKind {
    /// kernel-core runtime itself — used for skeleton events only.
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

/// Descriptor for a runtime component. Registered once per run via a
/// `component_registry` event at `run_start`; subsequent events only carry
/// the lightweight `EventEmitter { kind, id, instance }` and can be joined
/// back to this descriptor for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    pub kind: EmitterKind,
    pub id: String,
    pub name: String,
}

// ===========================================================================
// Events
// ===========================================================================

/// A single event in a session's append-only log.
///
/// `seq` is assigned by the backend at `append` time, not by the caller.
/// Callers constructing `SessionEvent` instances should leave `seq` at 0;
/// the backend overwrites it before persisting. Readers always order by
/// `seq`, never by `timestamp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Strictly monotonic within a session. 0 means "not yet assigned".
    pub seq: u64,

    /// Unique id for this event.
    pub uuid: String,

    /// Causal parent (e.g. `tool_result.parent_uuid == tool_use.uuid`).
    pub parent_uuid: Option<String>,

    /// Wall-clock epoch millis. Display only.
    pub timestamp: i64,

    /// Event type discriminator.
    #[serde(rename = "type")]
    pub event_type: String,

    /// Who wrote the event. Filled by `ScopedSession` at the extension point.
    pub emitter: EventEmitter,

    /// Message payload for user/assistant/tool_result events.
    pub message: Option<SessionMessage>,

    /// Arbitrary JSON payload for non-message events.
    pub data: Option<serde_json::Value>,
}

/// A conversation message embedded inside a `SessionEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// "user" | "assistant" | "tool"
    pub role: String,
    /// Content — a string or a content-block array.
    pub content: serde_json::Value,
}

impl SessionEvent {
    /// Construct a raw event with a fresh uuid and current timestamp.
    /// `seq` is 0 (will be overwritten by the backend).
    /// `emitter` is a placeholder — `ScopedSession::append` overwrites it.
    fn new(event_type: impl Into<String>) -> Self {
        Self {
            seq: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.into(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: None,
        }
    }

    /// Construct a user-message event.
    pub fn user_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("user");
        e.message = Some(SessionMessage { role: "user".into(), content });
        e
    }

    /// Construct an assistant-message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("assistant");
        e.message = Some(SessionMessage { role: "assistant".into(), content });
        e
    }

    /// Construct a tool_result event linked to a parent tool_use uuid.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        let mut e = Self::new("tool_result");
        e.parent_uuid = Some(parent_tool_use_uuid.to_string());
        e.message = Some(SessionMessage { role: "tool".into(), content });
        e
    }

    /// Construct a progress event with arbitrary data.
    pub fn progress(data: serde_json::Value) -> Self {
        let mut e = Self::new("progress");
        e.data = Some(data);
        e
    }

    /// Construct a system event with arbitrary data.
    pub fn system(data: serde_json::Value) -> Self {
        let mut e = Self::new("system");
        e.data = Some(data);
        e
    }
}

// ===========================================================================
// Query
// ===========================================================================

/// Filter for `AgentSession::query`. All fields optional; `None` means "don't
/// filter on this field".
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub event_type: Option<String>,
    pub role: Option<String>,
    pub text_contains: Option<String>,
    pub after_uuid: Option<String>,
    pub last_n: Option<usize>,
    pub limit: usize,
}

/// A query result with a short preview text for display.
#[derive(Debug, Clone)]
pub struct EventMatch {
    pub event: SessionEvent,
    pub preview: String,
}
```

- [ ] **Step 1.2: Register the module in `lib.rs`**

Modify `crates/alva-kernel-abi/src/lib.rs`. Find line 26 (`pub mod session;`). Add the new module declaration directly below it, so the block looks like:

```rust
pub mod scope;
pub mod session;
pub mod agent_session;
// tool_guard is now at tool::guard
```

Do NOT add any `pub use agent_session::...` re-exports yet — that will happen in Task 9 after the switchover. Keeping it un-re-exported in this task keeps the change fully additive.

- [ ] **Step 1.3: Verify the crate compiles**

Run:

```bash
cargo build -p alva-kernel-abi
```

Expected: success. Warnings about unused items in `agent_session` are OK (you will fix them in later tasks). If you see a hard error, re-read the file content and compare to Step 1.1 character-by-character.

- [ ] **Step 1.4: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs crates/alva-kernel-abi/src/lib.rs
git commit -m "feat(kernel-abi): scaffold agent_session module with event value types

Additive: introduces SessionEvent, SessionMessage, EventEmitter,
EmitterKind, ComponentDescriptor, EventQuery, EventMatch, and SessionError.
No trait yet, no impls yet, no re-exports yet. Nothing outside this module
references the new types; the old session.rs module is untouched."
```

---

## Task 2: Define the `AgentSession` trait

**Files:**
- Modify: `crates/alva-kernel-abi/src/agent_session.rs`

- [ ] **Step 2.1: Append the trait definition**

Append the following block to the END of `crates/alva-kernel-abi/src/agent_session.rs` (after the `EventMatch` struct):

```rust
// ===========================================================================
// AgentSession trait
// ===========================================================================

/// Unified session abstraction — the single source of truth for everything
/// that happens during an agent's lifetime.
///
/// ## Invariants
///
/// 1. **Monotonic order.** Every event has a `seq: u64` assigned atomically
///    at `append` time. `seq` is strictly increasing within a session; there
///    are no duplicates and no gaps (except where `rollback_after` deliberately
///    drops events).
/// 2. **Single writer target.** Every piece of information worth recording is
///    written to exactly one `AgentSession` instance for the session. Views
///    are projections, never independent stores.
/// 3. **Emitter identity.** Every event carries `emitter: EventEmitter`; the
///    scoped session wrapper at each extension point injects this automatically
///    so third-party code cannot fill it incorrectly.
///
/// ## Lifecycle contract
///
/// - **`restore()`** — called exactly once after construction, before any other
///   call. Idempotent. The backend warms internal projections (e.g. message
///   cache) from durable storage. For a fresh session, this is a no-op.
///
/// - **`flush()`** — called at three moments by the runtime: (a) `on_agent_end`,
///   (b) periodically during long runs (default every 10 iterations or 30
///   seconds), (c) once during graceful process shutdown. After `flush()`
///   returns, every event appended before `flush()` started MUST be durably
///   persisted.
///
/// - **`close()`** — called when the session is no longer going to be used.
///   Implies `flush()` followed by resource release. After `close()`, calls
///   to other methods MUST return an error.
///
/// - **`clear()`** — called only on explicit user-initiated reset or in tests.
///   Never called by the runtime during normal operation. Drops all events and
///   snapshots for this session.
#[async_trait]
pub trait AgentSession: Send + Sync {
    // --- Identity ---

    /// Unique identifier for this session.
    fn session_id(&self) -> &str;

    /// Parent session id for sub-agents. `None` for root sessions.
    fn parent_session_id(&self) -> Option<&str>;

    // --- Write ---

    /// Append a raw event. The backend assigns `event.seq` atomically and
    /// updates any internal projections (e.g. message cache).
    async fn append(&self, event: SessionEvent);

    /// Append an `AgentMessage` as a user / assistant / tool_result event.
    /// The backend translates the message into a `SessionEvent` with
    /// `emitter = EventEmitter::runtime()` and appends it.
    async fn append_message(&self, msg: AgentMessage);

    // --- Read: event-level ---

    /// Query events matching the filter, ordered by `seq` ascending.
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    /// Count events matching the filter.
    async fn count(&self, filter: &EventQuery) -> usize;

    // --- Read: message-level (hot path for LLM input assembly) ---

    /// All messages in append order, projected from events.
    /// Backends are expected to serve this from an internal cache.
    async fn messages(&self) -> Vec<AgentMessage>;

    /// The last N messages, projected from events.
    /// Backends are expected to serve this from an internal cache.
    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage>;

    // --- Write correction ---

    /// Drop all events with `seq` greater than the event identified by `uuid`.
    /// Returns the number of events dropped.
    async fn rollback_after(&self, uuid: &str) -> usize;

    /// Store an opaque snapshot blob (used by `ContextStore` for L0..L3 state).
    async fn save_snapshot(&self, data: &[u8]);

    /// Load the most recent snapshot, if any.
    async fn load_snapshot(&self) -> Option<Vec<u8>>;

    // --- Lifecycle ---

    async fn restore(&self) -> Result<(), SessionError>;

    async fn flush(&self) -> Result<(), SessionError>;

    async fn close(&self) -> Result<(), SessionError>;

    async fn clear(&self) -> Result<(), SessionError>;
}
```

- [ ] **Step 2.2: Verify compile**

```bash
cargo build -p alva-kernel-abi
```

Expected: success, with warnings about unused items. No errors.

- [ ] **Step 2.3: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs
git commit -m "feat(kernel-abi): define AgentSession trait with lifecycle contract

Adds the unified trait with event-level append/query/count/rollback,
message-level append_message/messages/recent_messages hot-path, snapshot
load/save, and lifecycle restore/flush/close/clear. No implementations yet."
```

---

## Task 3: Add `ScopedSession` wrapper

**Files:**
- Modify: `crates/alva-kernel-abi/src/agent_session.rs`

- [ ] **Step 3.1: Append `ScopedSession`**

Append to the end of `crates/alva-kernel-abi/src/agent_session.rs`:

```rust
// ===========================================================================
// ScopedSession
// ===========================================================================

/// A wrapper around an `AgentSession` handle that stamps every appended event
/// with a fixed `EventEmitter`. Third-party tools, middleware, and extensions
/// receive a `ScopedSession` from their execution context — they can append
/// events without ever touching the `emitter` field, because the wrapper
/// fills it at construction time.
///
/// This is the structural guarantee that `emitter.kind` always matches the
/// actual call path: runtime constructs the `ScopedSession` with the correct
/// kind and id for each extension point, and the wrapper prevents the caller
/// from overriding it.
#[derive(Clone)]
pub struct ScopedSession {
    inner: Arc<dyn AgentSession>,
    emitter: EventEmitter,
}

impl ScopedSession {
    /// Create a new scoped wrapper. The emitter is baked in — it cannot be
    /// changed after construction.
    pub fn new(inner: Arc<dyn AgentSession>, emitter: EventEmitter) -> Self {
        Self { inner, emitter }
    }

    /// The session id of the underlying session.
    pub fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    /// The emitter that will be stamped on every event appended through this
    /// wrapper.
    pub fn emitter(&self) -> &EventEmitter {
        &self.emitter
    }

    /// Append an event. The `emitter` field of the event is overwritten with
    /// this wrapper's emitter; any value set by the caller is discarded.
    pub async fn append(&self, mut event: SessionEvent) {
        event.emitter = self.emitter.clone();
        self.inner.append(event).await;
    }

    /// Delegate query to the inner session.
    pub async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.inner.query(filter).await
    }

    /// Delegate count to the inner session.
    pub async fn count(&self, filter: &EventQuery) -> usize {
        self.inner.count(filter).await
    }
}
```

- [ ] **Step 3.2: Verify compile**

```bash
cargo build -p alva-kernel-abi
```

Expected: success.

- [ ] **Step 3.3: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs
git commit -m "feat(kernel-abi): add ScopedSession wrapper for scoped event writes

ScopedSession wraps Arc<dyn AgentSession> with a baked-in EventEmitter.
append() overwrites the caller's emitter field, guaranteeing structurally
that emitter.kind matches the extension point that produced the event."
```

---

## Task 4: Implement `InMemoryAgentSession` — skeleton + seq counter

**Files:**
- Modify: `crates/alva-kernel-abi/src/agent_session.rs`

This task creates the struct and implements half of the trait (append, query, count, messages, recent_messages, and basic lifecycle no-ops). Rollback and snapshot come in Task 5.

- [ ] **Step 4.1: Append the struct + constructors**

Append to `crates/alva-kernel-abi/src/agent_session.rs`:

```rust
// ===========================================================================
// InMemoryAgentSession
// ===========================================================================

/// In-memory backend for `AgentSession`. Used by default in tests and by
/// agents that do not need persistence. All data lives in a single struct
/// protected by async RwLocks.
///
/// Implementation notes:
///
/// - `seq` counter is a single `AtomicU64`, `fetch_add(1, SeqCst)` at the
///   start of each `append`. This guarantees strict monotonic ordering
///   even under concurrent appends.
/// - `events` is the authoritative event log.
/// - `messages` is a projection cache: every `append` of a message-bearing
///   event (`user` / `assistant` / `tool_result` via `append_message` or via
///   raw `append`) pushes to this cache, so `recent_messages` is O(n).
/// - `snapshot` is a single opaque blob for `ContextStore`.
/// - Lifecycle methods are no-ops except `clear`, which actually resets
///   state. `flush`/`restore`/`close` are no-ops because there is nothing
///   to persist.
pub struct InMemoryAgentSession {
    session_id: String,
    parent_session_id: Option<String>,
    seq_counter: AtomicU64,
    events: RwLock<Vec<SessionEvent>>,
    messages: RwLock<VecDeque<AgentMessage>>,
    snapshot: RwLock<Option<Vec<u8>>>,
}

impl InMemoryAgentSession {
    /// Create a fresh root session with a random UUID v4.
    pub fn new() -> Self {
        Self::with_id(uuid::Uuid::new_v4().to_string())
    }

    /// Create a fresh root session with the given id.
    pub fn with_id(session_id: String) -> Self {
        Self {
            session_id,
            parent_session_id: None,
            seq_counter: AtomicU64::new(1),
            events: RwLock::new(Vec::new()),
            messages: RwLock::new(VecDeque::new()),
            snapshot: RwLock::new(None),
        }
    }

    /// Create a child session linked to a parent.
    pub fn with_parent(parent_session_id: impl Into<String>) -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            parent_session_id: Some(parent_session_id.into()),
            seq_counter: AtomicU64::new(1),
            events: RwLock::new(Vec::new()),
            messages: RwLock::new(VecDeque::new()),
            snapshot: RwLock::new(None),
        }
    }

    /// Translate an `AgentMessage` into a `SessionEvent`. Used by
    /// `append_message`. The resulting event has `emitter = Runtime`.
    fn message_to_event(msg: &AgentMessage) -> SessionEvent {
        use crate::base::message::MessageRole;

        let (role, content) = match msg {
            AgentMessage::Standard(m) => {
                let role_str = match m.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::System => "system",
                };
                let content = serde_json::to_value(&m.content).unwrap_or_else(|_| serde_json::json!([]));
                (role_str, content)
            }
            AgentMessage::ToolResult(tr) => {
                let content = serde_json::to_value(&tr.content).unwrap_or_else(|_| serde_json::json!([]));
                ("tool", content)
            }
            AgentMessage::Marker(_) => {
                // Markers carry no role; emit as a system event with empty content.
                ("system", serde_json::json!(null))
            }
        };

        let event_type = match role {
            "user" => "user",
            "assistant" => "assistant",
            "tool" => "tool_result",
            _ => "system",
        };

        let mut ev = SessionEvent::new(event_type);
        ev.message = Some(SessionMessage {
            role: role.to_string(),
            content,
        });
        ev
    }

    /// Whether a given event carries a message payload we should cache.
    fn event_carries_message(event: &SessionEvent) -> bool {
        matches!(
            event.event_type.as_str(),
            "user" | "assistant" | "tool_result"
        )
    }
}

impl Default for InMemoryAgentSession {
    fn default() -> Self {
        Self::new()
    }
}
```

- [ ] **Step 4.2: Verify compile**

```bash
cargo build -p alva-kernel-abi
```

Expected: success, with warnings about unused methods. If compile fails, check that `AgentMessage` is in scope and that `MessageRole` variants match the ones used in the project (read `crates/alva-kernel-abi/src/base/message.rs` to confirm).

- [ ] **Step 4.3: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs
git commit -m "feat(kernel-abi): add InMemoryAgentSession struct with constructors

Skeleton for the in-memory backend: atomic seq counter, events log,
message cache, snapshot slot. message_to_event translates AgentMessage
into the appropriate SessionEvent. No trait impl yet."
```

---

## Task 5: Implement `AgentSession` for `InMemoryAgentSession`

**Files:**
- Modify: `crates/alva-kernel-abi/src/agent_session.rs`

- [ ] **Step 5.1: Append the trait impl**

Append to `crates/alva-kernel-abi/src/agent_session.rs`:

```rust
#[async_trait]
impl AgentSession for InMemoryAgentSession {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn parent_session_id(&self) -> Option<&str> {
        self.parent_session_id.as_deref()
    }

    async fn append(&self, mut event: SessionEvent) {
        // Assign seq atomically. This is the only place seq is assigned.
        event.seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);

        // If the event carries a message, update the cache. We do this
        // outside the events lock to avoid holding two locks.
        let cached_msg: Option<AgentMessage> = if Self::event_carries_message(&event) {
            event_to_message(&event)
        } else {
            None
        };

        // Write the event.
        self.events.write().await.push(event);

        // Update the message cache if we extracted one.
        if let Some(m) = cached_msg {
            self.messages.write().await.push_back(m);
        }
    }

    async fn append_message(&self, msg: AgentMessage) {
        // Translate then delegate to append so that seq assignment,
        // event log write, and cache update all share the same code path.
        let event = Self::message_to_event(&msg);
        self.append(event).await;
    }

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        let events = self.events.read().await;

        // Find start position if after_uuid cursor is set.
        let start = if let Some(ref after) = filter.after_uuid {
            events
                .iter()
                .position(|e| e.uuid == *after)
                .map(|i| i + 1)
                .unwrap_or(0)
        } else {
            0
        };

        let mut matches: Vec<EventMatch> = events[start..]
            .iter()
            .filter(|e| event_matches(e, filter))
            .map(|e| EventMatch {
                preview: make_preview(e),
                event: e.clone(),
            })
            .collect();

        if let Some(n) = filter.last_n {
            let skip = matches.len().saturating_sub(n);
            matches = matches.into_iter().skip(skip).collect();
        }

        if filter.limit > 0 {
            matches.truncate(filter.limit);
        }

        matches
    }

    async fn count(&self, filter: &EventQuery) -> usize {
        let events = self.events.read().await;
        events.iter().filter(|e| event_matches(e, filter)).count()
    }

    async fn messages(&self) -> Vec<AgentMessage> {
        let msgs = self.messages.read().await;
        msgs.iter().cloned().collect()
    }

    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage> {
        let msgs = self.messages.read().await;
        let len = msgs.len();
        if n >= len {
            msgs.iter().cloned().collect()
        } else {
            msgs.iter().skip(len - n).cloned().collect()
        }
    }

    async fn rollback_after(&self, uuid: &str) -> usize {
        let mut events = self.events.write().await;
        if let Some(pos) = events.iter().position(|e| e.uuid == *uuid) {
            let removed = events.len() - pos - 1;
            events.truncate(pos + 1);

            // Rebuild the message cache from the surviving events.
            // We drop the events lock before rebuilding the cache to avoid
            // holding two write locks; we briefly reacquire a read guard
            // on events by cloning the surviving slice.
            let surviving: Vec<SessionEvent> = events.iter().cloned().collect();
            drop(events);

            let mut msgs = self.messages.write().await;
            msgs.clear();
            for ev in &surviving {
                if Self::event_carries_message(ev) {
                    if let Some(m) = event_to_message(ev) {
                        msgs.push_back(m);
                    }
                }
            }

            removed
        } else {
            0
        }
    }

    async fn save_snapshot(&self, data: &[u8]) {
        *self.snapshot.write().await = Some(data.to_vec());
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot.read().await.clone()
    }

    async fn restore(&self) -> Result<(), SessionError> {
        // In-memory: nothing persisted, nothing to restore.
        Ok(())
    }

    async fn flush(&self) -> Result<(), SessionError> {
        // In-memory: nothing to persist.
        Ok(())
    }

    async fn close(&self) -> Result<(), SessionError> {
        // In-memory: nothing to release.
        Ok(())
    }

    async fn clear(&self) -> Result<(), SessionError> {
        self.events.write().await.clear();
        self.messages.write().await.clear();
        *self.snapshot.write().await = None;
        self.seq_counter.store(1, Ordering::SeqCst);
        Ok(())
    }
}

// ===========================================================================
// Internal helpers
// ===========================================================================

fn event_matches(event: &SessionEvent, filter: &EventQuery) -> bool {
    if let Some(ref et) = filter.event_type {
        if event.event_type != *et {
            return false;
        }
    }
    if let Some(ref role) = filter.role {
        match &event.message {
            Some(msg) if msg.role == *role => {}
            _ => return false,
        }
    }
    if let Some(ref text) = filter.text_contains {
        let content_str = match &event.message {
            Some(msg) => msg.content.to_string(),
            None => match &event.data {
                Some(d) => d.to_string(),
                None => String::new(),
            },
        };
        if !content_str.to_lowercase().contains(&text.to_lowercase()) {
            return false;
        }
    }
    true
}

fn safe_truncate(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    let mut end = max_bytes;
    while end > 0 && !s.is_char_boundary(end) {
        end -= 1;
    }
    &s[..end]
}

fn make_preview(event: &SessionEvent) -> String {
    let text = match &event.message {
        Some(msg) => match &msg.content {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        },
        None => match &event.data {
            Some(d) => d.to_string(),
            None => String::new(),
        },
    };
    if text.len() > 160 {
        format!("{}...", safe_truncate(&text, 160))
    } else {
        text
    }
}

/// Project a `SessionEvent` back into an `AgentMessage`, if the event carries
/// a message payload. Used by the in-memory message cache and by rollback.
fn event_to_message(event: &SessionEvent) -> Option<AgentMessage> {
    use crate::base::message::{Message, MessageRole};

    let msg = event.message.as_ref()?;
    let role = match msg.role.as_str() {
        "user" => MessageRole::User,
        "assistant" => MessageRole::Assistant,
        "system" => MessageRole::System,
        "tool" => {
            // tool_result events — reconstruct as an AgentMessage::ToolResult.
            // For the in-memory cache we project it into a Standard user
            // message with the tool content, because that's what the LLM
            // input assembly expects. If ToolResult reconstruction is needed
            // in the future, handle the "tool" branch explicitly.
            MessageRole::User
        }
        _ => return None,
    };

    let content = serde_json::from_value(msg.content.clone()).unwrap_or_default();
    Some(AgentMessage::Standard(Message {
        role,
        content,
        usage: None,
    }))
}
```

- [ ] **Step 5.2: Verify compile**

```bash
cargo build -p alva-kernel-abi
```

Expected: success. If you hit compile errors around `Message { role, content, usage }`, the struct layout may have changed — open `crates/alva-kernel-abi/src/base/message.rs` and match the current field list, then adjust `event_to_message`.

- [ ] **Step 5.3: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs
git commit -m "feat(kernel-abi): implement AgentSession for InMemoryAgentSession

Full trait impl: atomic seq assignment, event log + message cache write-through,
query/count with filter, rollback_after that rebuilds the cache from surviving
events, snapshot save/load, lifecycle no-ops (clear actually resets). Helper
functions for filter matching, preview truncation (unicode-safe), and
event-to-message projection."
```

---

## Task 6: Unit tests for `InMemoryAgentSession`

**Files:**
- Modify: `crates/alva-kernel-abi/src/agent_session.rs`

- [ ] **Step 6.1: Append the test module**

Append to the end of `crates/alva-kernel-abi/src/agent_session.rs`:

```rust
// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::message::{Message, MessageRole};

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            role: MessageRole::User,
            content: vec![crate::base::content::ContentBlock::Text {
                text: text.to_string(),
            }],
            usage: None,
        })
    }

    #[tokio::test]
    async fn new_session_has_id_and_no_parent() {
        let s = InMemoryAgentSession::new();
        assert!(!s.session_id().is_empty());
        assert!(s.parent_session_id().is_none());
    }

    #[tokio::test]
    async fn child_session_has_parent() {
        let root = InMemoryAgentSession::new();
        let child = InMemoryAgentSession::with_parent(root.session_id());
        assert_eq!(child.parent_session_id(), Some(root.session_id()));
    }

    #[tokio::test]
    async fn append_assigns_monotonic_seq() {
        let s = InMemoryAgentSession::new();
        let mut e1 = SessionEvent::progress(serde_json::json!({"n": 1}));
        let mut e2 = SessionEvent::progress(serde_json::json!({"n": 2}));
        let mut e3 = SessionEvent::progress(serde_json::json!({"n": 3}));
        e1.seq = 0;
        e2.seq = 0;
        e3.seq = 0;

        s.append(e1).await;
        s.append(e2).await;
        s.append(e3).await;

        let events = s.events.read().await;
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
        assert_eq!(events[2].seq, 3);
    }

    #[tokio::test]
    async fn concurrent_append_preserves_monotonic_seq() {
        use std::sync::Arc;

        let s = Arc::new(InMemoryAgentSession::new());
        let mut handles = Vec::new();
        for i in 0..100 {
            let s = s.clone();
            handles.push(tokio::spawn(async move {
                let e = SessionEvent::progress(serde_json::json!({"i": i}));
                s.append(e).await;
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let events = s.events.read().await;
        assert_eq!(events.len(), 100);

        // Collect seqs and verify they are exactly {1..=100} with no duplicates
        // and no gaps. Ordering in the Vec matches insertion order, which matches
        // seq order because append grabs the counter before pushing.
        let mut seqs: Vec<u64> = events.iter().map(|e| e.seq).collect();
        seqs.sort_unstable();
        for (i, seq) in seqs.iter().enumerate() {
            assert_eq!(*seq, (i + 1) as u64, "seq at index {} should be {}", i, i + 1);
        }
    }

    #[tokio::test]
    async fn append_message_updates_cache_and_events() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("hello")).await;
        s.append_message(user_msg("world")).await;

        // Message cache has both
        let msgs = s.messages().await;
        assert_eq!(msgs.len(), 2);

        // Events log has both as "user" events with correct seq
        let events = s.events.read().await;
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].event_type, "user");
        assert_eq!(events[1].event_type, "user");
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[1].seq, 2);
    }

    #[tokio::test]
    async fn recent_messages_returns_last_n_from_cache() {
        let s = InMemoryAgentSession::new();
        for i in 0..10 {
            s.append_message(user_msg(&format!("msg {}", i))).await;
        }

        let recent = s.recent_messages(3).await;
        assert_eq!(recent.len(), 3);

        // Verify it's the last three (msg 7, 8, 9).
        if let AgentMessage::Standard(m) = &recent[0] {
            assert!(m.text_content().contains("msg 7"));
        } else {
            panic!("expected Standard message");
        }
    }

    #[tokio::test]
    async fn recent_messages_larger_than_total_returns_all() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("one")).await;
        assert_eq!(s.recent_messages(100).await.len(), 1);
    }

    #[tokio::test]
    async fn query_by_event_type() {
        let s = InMemoryAgentSession::new();
        s.append(SessionEvent::user_message(serde_json::json!("hi"))).await;
        s.append(SessionEvent::progress(serde_json::json!({"ok": true}))).await;
        s.append(SessionEvent::progress(serde_json::json!({"ok": false}))).await;

        let progress = s.query(&EventQuery {
            event_type: Some("progress".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(progress.len(), 2);

        let users = s.query(&EventQuery {
            event_type: Some("user".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(users.len(), 1);
    }

    #[tokio::test]
    async fn rollback_after_drops_events_and_rebuilds_cache() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("one")).await;
        s.append_message(user_msg("two")).await;
        s.append_message(user_msg("three")).await;

        // Grab the uuid of the second event (the "two" message).
        let second_uuid = s.events.read().await[1].uuid.clone();

        // Rollback after "two": drops "three".
        let dropped = s.rollback_after(&second_uuid).await;
        assert_eq!(dropped, 1);

        // Events log has two items; message cache has two items.
        assert_eq!(s.events.read().await.len(), 2);
        assert_eq!(s.messages().await.len(), 2);
    }

    #[tokio::test]
    async fn snapshot_save_and_load() {
        let s = InMemoryAgentSession::new();
        assert!(s.load_snapshot().await.is_none());

        s.save_snapshot(b"ctx-bytes").await;
        assert_eq!(s.load_snapshot().await.unwrap(), b"ctx-bytes");
    }

    #[tokio::test]
    async fn lifecycle_methods_are_ok() {
        let s = InMemoryAgentSession::new();
        s.restore().await.unwrap();
        s.flush().await.unwrap();
        s.close().await.unwrap();
    }

    #[tokio::test]
    async fn clear_resets_everything() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("one")).await;
        s.save_snapshot(b"snap").await;

        s.clear().await.unwrap();

        assert_eq!(s.messages().await.len(), 0);
        assert!(s.load_snapshot().await.is_none());

        // After clear, seq counter restarts at 1.
        s.append(SessionEvent::progress(serde_json::json!({"after": "clear"}))).await;
        assert_eq!(s.events.read().await[0].seq, 1);
    }

    #[tokio::test]
    async fn scoped_session_stamps_emitter() {
        let session = Arc::new(InMemoryAgentSession::new());
        let scoped = ScopedSession::new(
            session.clone() as Arc<dyn AgentSession>,
            EventEmitter {
                kind: EmitterKind::Tool,
                id: "read_file".into(),
                instance: None,
            },
        );

        // Construct an event with a bogus emitter; scoped.append must overwrite it.
        let mut e = SessionEvent::progress(serde_json::json!({"x": 1}));
        e.emitter = EventEmitter {
            kind: EmitterKind::Runtime,
            id: "bogus".into(),
            instance: None,
        };
        scoped.append(e).await;

        // Read back via the Arc<InMemoryAgentSession> to assert the event's emitter.
        let events = session.events.read().await;
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].emitter.kind, EmitterKind::Tool);
        assert_eq!(events[0].emitter.id, "read_file");
    }
}
```

- [ ] **Step 6.2: Run the tests**

```bash
cargo test -p alva-kernel-abi --lib agent_session::tests
```

Expected: all tests pass. If any fail, read the failure output carefully and fix either the test or the impl (tests are more likely to be buggy; the impl has been designed). A common issue: the `text_content()` method on `Message` may not exist — if so, inspect the response, switch the assertion to inspect `content` directly.

- [ ] **Step 6.3: Commit**

```bash
git add crates/alva-kernel-abi/src/agent_session.rs
git commit -m "test(kernel-abi): cover InMemoryAgentSession invariants

- Monotonic seq under concurrent append (100 tasks)
- append_message round-trip to message cache and events log
- recent_messages slice semantics
- query by event type
- rollback_after drops events and rebuilds message cache
- snapshot save/load
- clear resets everything including seq counter
- ScopedSession stamps emitter regardless of caller-provided value"
```

---

## Task 7: Add `session()` to `ToolExecutionContext`

**Files:**
- Modify: `crates/alva-kernel-abi/src/tool/execution.rs`

- [ ] **Step 7.1: Update the trait**

Open `crates/alva-kernel-abi/src/tool/execution.rs`. Find the `ToolExecutionContext` trait definition (line 157). Add a new method with a default implementation returning `None`, placed right after `bus()`:

Locate this block:

```rust
    /// Cross-layer coordination bus handle.
    /// Returns None when bus is not wired (e.g., in tests using MinimalExecutionContext).
    fn bus(&self) -> Option<&BusHandle> {
        None
    }

    /// Downcast support for application-specific extensions.
    fn as_any(&self) -> &dyn Any;
```

Replace it with:

```rust
    /// Cross-layer coordination bus handle.
    /// Returns None when bus is not wired (e.g., in tests using MinimalExecutionContext).
    fn bus(&self) -> Option<&BusHandle> {
        None
    }

    /// Scoped session handle for this tool invocation.
    ///
    /// Returns `Some` when the runtime has wired an `AgentSession` into
    /// this execution context; events appended through the returned
    /// `ScopedSession` are automatically stamped with
    /// `EmitterKind::Tool` and this tool's registered id.
    ///
    /// Returns `None` for contexts that do not carry a session (tests,
    /// `MinimalExecutionContext`, standalone tool runners).
    fn session(&self) -> Option<&crate::agent_session::ScopedSession> {
        None
    }

    /// Downcast support for application-specific extensions.
    fn as_any(&self) -> &dyn Any;
```

- [ ] **Step 7.2: Update the module-level `// OUTPUT:` comment**

At the top of the file (line 2), the current comment is:

```rust
// OUTPUT: ProgressEvent, ToolContent, ToolOutput, ToolExecutionContext (trait), MinimalExecutionContext
```

Leave it as is — `ToolExecutionContext` is already listed, the new method is just a method on an existing trait.

- [ ] **Step 7.3: Verify `MinimalExecutionContext` still compiles**

`MinimalExecutionContext` does not need to change because the new method has a default returning `None`. Verify:

```bash
cargo build -p alva-kernel-abi
```

Expected: success. If you see an error about the default impl referencing `crate::agent_session::ScopedSession`, double-check that `pub mod agent_session;` is present in `lib.rs` (you added this in Task 1).

- [ ] **Step 7.4: Commit**

```bash
git add crates/alva-kernel-abi/src/tool/execution.rs
git commit -m "feat(kernel-abi): add session() accessor to ToolExecutionContext

Returns Option<&ScopedSession>. Default impl returns None so existing
context implementations (MinimalExecutionContext, test contexts) are
unchanged. Runtime-provided contexts will override to return a scoped
session stamped with EmitterKind::Tool and the tool's id."
```

---

## Task 8: The atomic switchover commit

**Files:**
- Modify: `crates/alva-kernel-core/src/state.rs`
- Modify: `crates/alva-kernel-core/src/run.rs`
- Modify: `crates/alva-kernel-core/src/run_child.rs`
- Modify: `crates/alva-kernel-core/src/builtins/test_helpers.rs`
- Modify: `crates/alva-kernel-core/src/middleware.rs`
- Modify: `crates/alva-kernel-core/tests/integration.rs`
- Modify: `crates/alva-kernel-core/src/state.rs` (tests section)
- Modify: other files that import `alva_kernel_abi::session::InMemorySession` (grep to find)

This is the big commit that flips `AgentState.session` from the old trait to the new one. All message-append call sites become `.await`-based. The old trait file still exists after this commit; Task 9 deletes it.

- [ ] **Step 8.1: Find every file that imports the old `InMemorySession`**

Run:

```bash
grep -rn --include='*.rs' 'alva_kernel_abi::session::InMemorySession\|use alva_kernel_abi::session\|session::AgentSession' crates/ | grep -v 'kernel-abi/src/session.rs'
```

Record the output. These are the files you need to update. Expected matches include at least:
- `crates/alva-kernel-core/src/builtins/test_helpers.rs`
- `crates/alva-kernel-core/tests/integration.rs`
- `crates/alva-kernel-core/src/state.rs` (test block)
- possibly `crates/alva-kernel-core/examples/middleware_basic.rs`
- possibly `crates/alva-app-core/*`, `crates/alva-host-*`, `crates/alva-agent-core/*`

For each file: replace `use alva_kernel_abi::session::InMemorySession` with `use alva_kernel_abi::agent_session::InMemoryAgentSession`, and replace `InMemorySession::new()` with `InMemoryAgentSession::new()`.

- [ ] **Step 8.2: Update `AgentState.session` type in `state.rs`**

Open `crates/alva-kernel-core/src/state.rs`. Change line 8 from:

```rust
use alva_kernel_abi::session::AgentSession;
```

to:

```rust
use alva_kernel_abi::agent_session::AgentSession;
```

In the `AgentState` struct doc comment block (around line 17-19), update to reflect that `session` now carries events as well as messages. Change:

```rust
    /// Session managing message history.
    pub session: Arc<dyn AgentSession>,
```

to:

```rust
    /// Session managing the unified event log (message history + runtime
    /// skeleton events + component-emitted events). The single source of
    /// truth for everything this agent does.
    pub session: Arc<dyn AgentSession>,
```

In the test block at the bottom of the file, change:

```rust
    use alva_kernel_abi::session::InMemorySession;
```

to:

```rust
    use alva_kernel_abi::agent_session::InMemoryAgentSession;
```

And change `InMemorySession::new()` to `InMemoryAgentSession::new()` in the test body.

- [ ] **Step 8.3: Update `MiddlewareContext` to carry a scoped session**

Open `crates/alva-kernel-core/src/middleware.rs`. Find the `MiddlewareContext` struct (line 149). Change from:

```rust
pub struct MiddlewareContext {
    pub bus: Option<alva_kernel_abi::BusHandle>,
    pub workspace: Option<std::path::PathBuf>,
}
```

to:

```rust
pub struct MiddlewareContext {
    pub bus: Option<alva_kernel_abi::BusHandle>,
    pub workspace: Option<std::path::PathBuf>,
    /// Scoped session for this middleware. None only in test setups that
    /// do not wire a session (e.g. unit tests of middleware hooks in isolation).
    /// Middleware that emits events should call `ctx.session.as_ref()?.append(...)`.
    pub session: Option<alva_kernel_abi::agent_session::ScopedSession>,
}
```

Find every call site that constructs `MiddlewareContext { bus: ..., workspace: ... }` and add `session: None` to each. Use:

```bash
grep -rn --include='*.rs' 'MiddlewareContext {' crates/
```

- [ ] **Step 8.4: Migrate `run.rs` call sites**

Open `crates/alva-kernel-core/src/run.rs`.

The 5 `state.session.append(msg)` sites are at lines 388, 596, 726, 767, 794 (seq numbers from the pre-migration state — the line numbers may drift slightly if you modified earlier lines). Find them with:

```bash
grep -n 'state.session.append(' crates/alva-kernel-core/src/run.rs
```

Replace each of the 5 with `state.session.append_message(...).await`. For example, line 388:

Before:
```rust
    for msg in input {
        state.session.append(msg.clone());
        let _ = fire_context_on_message(config, &agent_id, &msg).await;
    }
```

After:
```rust
    for msg in input {
        state.session.append_message(msg.clone()).await;
        let _ = fire_context_on_message(config, &agent_id, &msg).await;
    }
```

Apply the same transformation to the other 4 sites (596, 726, 767, 794).

Now migrate the reads. Find:

```bash
grep -n 'state.session.messages()\|state.session.recent(' crates/alva-kernel-core/src/run.rs
```

At line 464:

Before:
```rust
                state.session.recent(config.context_window)
```

After:
```rust
                state.session.recent_messages(config.context_window).await
```

At line 466:

Before:
```rust
                state.session.messages()
```

After:
```rust
                state.session.messages().await
```

- [ ] **Step 8.5: Migrate `run_child.rs` call sites**

Open `crates/alva-kernel-core/src/run_child.rs`. Run the same grep:

```bash
grep -n 'session.append\|session.messages\|session.recent' crates/alva-kernel-core/src/run_child.rs
```

Apply the same transformations: `.append(msg)` → `.append_message(msg).await`, `.messages()` → `.messages().await`, `.recent(n)` → `.recent_messages(n).await`.

- [ ] **Step 8.6: Update test helpers**

Open `crates/alva-kernel-core/src/builtins/test_helpers.rs`. Find any use of `InMemorySession` and replace with `InMemoryAgentSession`. Find the import:

```rust
use alva_kernel_abi::session::InMemorySession;
```

Replace with:

```rust
use alva_kernel_abi::agent_session::InMemoryAgentSession;
```

And update the `make_state` helper (or similar) to construct `InMemoryAgentSession` instead. If the helper calls `session.append(msg)` synchronously, that call must become `.append_message(msg).await` (which requires the helper to be async if it wasn't). If making the helper async would cascade into many test changes, an alternative is to use `tokio::runtime::Handle::current().block_on(...)` — but only do this as a last resort.

- [ ] **Step 8.7: Update integration tests**

Open `crates/alva-kernel-core/tests/integration.rs`. Grep for `InMemorySession` and `session.append`:

```bash
grep -n 'InMemorySession\|session\.append\|session\.messages\|session\.recent' crates/alva-kernel-core/tests/integration.rs
```

Apply the same transformations. Tests are already async (they use `#[tokio::test]`), so adding `.await` should be straightforward.

- [ ] **Step 8.8: Update other crates that touch the old trait**

Use the grep output from Step 8.1 to walk every other file. Common pattern: `use alva_kernel_abi::session::{AgentSession, InMemorySession}` → `use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession}`, and any sync `.append(msg)` becomes `.append_message(msg).await`.

Likely files to check:
- `crates/alva-agent-core/src/agent_builder.rs`
- `crates/alva-host-native/src/builder.rs`
- `crates/alva-host-wasm/src/agent.rs`
- `crates/alva-host-wasm/src/smoke.rs`
- `crates/alva-kernel-core/examples/middleware_basic.rs`
- `crates/alva-app-core/src/base_agent/agent.rs`
- `crates/alva-app-core/tests/e2e_agent_test.rs`
- `crates/alva-engine-adapter-alva/src/adapter.rs`
- `crates/alva-app/src/chat/gpui_chat.rs`

- [ ] **Step 8.9: Build the entire workspace**

```bash
cargo build --workspace
```

Expected: success. If you get compile errors, they should be isolated to files you missed in Step 8.8. Read each error, locate the file, apply the same transformation, and re-run.

- [ ] **Step 8.10: Run all tests**

```bash
cargo test --workspace
```

Expected: all tests pass. If tests fail, it is most likely because:
- A test helper constructs an agent with `InMemorySession::new()` that you missed → fix.
- A test asserts on message content but the new `append_message` path projects differently — compare carefully and adjust the test to the new behavior if the new behavior is correct.

- [ ] **Step 8.11: Commit**

```bash
git add -A
git commit -m "refactor(kernel-core): switch AgentState.session to new AgentSession trait

- AgentState.session is now Arc<dyn agent_session::AgentSession>
- run.rs: 5 append sites → append_message(..).await; messages()/recent(n)
  → .await forms
- run_child.rs: same migration
- MiddlewareContext gains a session field (Option<ScopedSession>)
- test_helpers, integration tests, example crates, host-* crates updated
  to construct InMemoryAgentSession

Old session.rs trait still exists in kernel-abi but is unused by kernel-core
after this commit. Deletion is in the next commit."
```

---

## Task 9: Delete `alva-kernel-abi/src/session.rs`

**Files:**
- Delete: `crates/alva-kernel-abi/src/session.rs`
- Modify: `crates/alva-kernel-abi/src/lib.rs`

- [ ] **Step 9.1: Delete the file**

```bash
rm crates/alva-kernel-abi/src/session.rs
```

- [ ] **Step 9.2: Remove `pub mod session;` and the re-export from `lib.rs`**

Open `crates/alva-kernel-abi/src/lib.rs`. Delete line 26:

```rust
pub mod session;
```

Delete line 60:

```rust
pub use session::{AgentSession, InMemorySession};
```

Add a new re-export block right after the existing re-exports (near line 60 area):

```rust
pub use agent_session::{
    AgentSession, InMemoryAgentSession, SessionError, SessionEvent, SessionMessage,
    EventEmitter, EmitterKind, ComponentDescriptor, ScopedSession, EventQuery, EventMatch,
};
```

- [ ] **Step 9.3: Build the workspace**

```bash
cargo build --workspace
```

Expected: success. If any file still imports from the old path, the error will point to it directly. Fix by switching to `alva_kernel_abi::AgentSession` (crate-root re-export) or `alva_kernel_abi::agent_session::AgentSession` (explicit module).

- [ ] **Step 9.4: Run all tests**

```bash
cargo test --workspace
```

Expected: all tests pass.

- [ ] **Step 9.5: Commit**

```bash
git add -A
git commit -m "refactor(kernel-abi): delete legacy session.rs

The old AgentSession trait + InMemorySession type are gone. Replaced
entirely by the new unified trait in agent_session.rs. Crate-root
re-exports are updated so external callers using alva_kernel_abi::AgentSession
continue to compile without source changes."
```

---

## Task 10: Add skeleton events to `run.rs`

**Files:**
- Modify: `crates/alva-kernel-core/src/run.rs`

This task adds the runtime-written events: `run_start`, `component_registry`, `iteration_start`, `llm_call_start`, `assistant`, `llm_call_end`, `tool_use`, `tool_result`, `iteration_end`, `run_end`. The existing `user` events (via `append_message`) already exist from Task 8.

The approach: track uuids of the current `run_start`, `iteration_start`, and `llm_call_start` in local variables as the loop runs, and use them as `parent_uuid` for downstream events.

- [ ] **Step 10.1: Add a helper for emitting raw runtime events**

Near the top of `run.rs`, after the existing `use` statements, add a small helper that constructs a `SessionEvent` with the `Runtime` emitter and appends it:

```rust
use alva_kernel_abi::agent_session::{
    AgentSession, ComponentDescriptor, EmitterKind, EventEmitter, SessionEvent,
};

/// Append a runtime-emitted event to the session. The emitter is always
/// `EventEmitter::runtime()`; callers only set event_type, parent_uuid, and
/// data. Returns the uuid of the appended event so callers can use it as
/// a parent for subsequent events.
async fn emit_runtime_event(
    session: &std::sync::Arc<dyn AgentSession>,
    event_type: &str,
    parent_uuid: Option<String>,
    data: Option<serde_json::Value>,
) -> String {
    let mut event = SessionEvent::new_runtime(event_type);
    event.parent_uuid = parent_uuid;
    event.data = data;
    let uuid = event.uuid.clone();
    session.append(event).await;
    uuid
}
```

Wait — `SessionEvent::new` is private in `agent_session.rs`. Add a public constructor for runtime use. Open `crates/alva-kernel-abi/src/agent_session.rs` and find the `impl SessionEvent` block. Add a new method:

```rust
    /// Construct a runtime event with the given `event_type`. Used by
    /// kernel-core to emit skeleton events.
    pub fn new_runtime(event_type: impl Into<String>) -> Self {
        Self::new(event_type)
    }
```

Save. Build:

```bash
cargo build -p alva-kernel-abi
```

Expected: success.

- [ ] **Step 10.2: Emit `run_start` and `component_registry`**

In `run.rs`, locate the function that is the entry point of a run (where the input messages are processed — around line 380 where the existing `for msg in input` loop begins, which is inside a function such as `start_run` or similar). Before the `for msg in input` loop, insert:

```rust
    // --- Session skeleton: run_start ---
    let run_start_uuid = emit_runtime_event(
        &state.session,
        "run_start",
        None,
        Some(serde_json::json!({
            "agent_id": agent_id.clone(),
            "max_iterations": config.max_iterations,
        })),
    ).await;

    // --- Session skeleton: component_registry ---
    // Collect descriptors for every tool and middleware in this run.
    let mut components: Vec<ComponentDescriptor> = Vec::new();
    for tool in &state.tools {
        components.push(ComponentDescriptor {
            kind: EmitterKind::Tool,
            id: tool.name().to_string(),
            name: tool.name().to_string(),
        });
    }
    for mw_name in config.middleware.names() {
        components.push(ComponentDescriptor {
            kind: EmitterKind::Middleware,
            id: mw_name.clone(),
            name: mw_name,
        });
    }
    emit_runtime_event(
        &state.session,
        "component_registry",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({ "components": components })),
    ).await;
```

If `config.middleware.names()` does not exist, check the `MiddlewareStack` struct for a public API that enumerates middleware names. If there is none, add one:

Open `crates/alva-kernel-core/src/middleware.rs`. In the `impl MiddlewareStack` block, add:

```rust
    /// Return the names of all middleware in the stack, in insertion order.
    pub fn names(&self) -> Vec<String> {
        self.layers.iter().map(|m| m.name().to_string()).collect()
    }
```

Build:

```bash
cargo build -p alva-kernel-core
```

Expected: success.

- [ ] **Step 10.3: Emit `run_end`**

At the end of the run entry point, after `on_agent_end` middleware fires, insert:

```rust
    // --- Session skeleton: run_end ---
    emit_runtime_event(
        &state.session,
        "run_end",
        Some(run_start_uuid.clone()),
        Some(serde_json::json!({
            "error": error.clone(),
        })),
    ).await;
```

(`error` is the `Option<String>` already present in the function.)

- [ ] **Step 10.4: Emit `iteration_start` / `iteration_end`**

Inside `run_loop`, locate the main iteration loop (search for `for iteration in` or a `while` with the iteration counter — around line 440 or so). Just before each iteration begins, emit:

```rust
        // Session skeleton: iteration boundary
        let iteration_start_uuid = emit_runtime_event(
            &state.session,
            "iteration_start",
            Some(run_start_uuid.clone()),
            Some(serde_json::json!({ "iteration": iteration })),
        ).await;
```

After the iteration body (before the loop's `continue` / end of iteration block), emit:

```rust
        emit_runtime_event(
            &state.session,
            "iteration_end",
            Some(iteration_start_uuid.clone()),
            None,
        ).await;
```

Note: `run_start_uuid` is defined in the parent function (`start_run` or whatever). If `run_loop` is a separate function, you need to pass `run_start_uuid` into it as a parameter. Grep:

```bash
grep -n 'async fn run_loop\|fn run_loop(' crates/alva-kernel-core/src/run.rs
```

Update the signature of `run_loop` to accept `run_start_uuid: &str` and thread it through.

- [ ] **Step 10.5: Emit `llm_call_start` / `llm_call_end`**

Inside the iteration body, locate the LLM call (look for `model.complete(...).await` or similar around line 560-580). Before the call, emit:

```rust
            let llm_start_uuid = emit_runtime_event(
                &state.session,
                "llm_call_start",
                Some(iteration_start_uuid.clone()),
                Some(serde_json::json!({
                    "iteration": iteration,
                    "message_count": session_messages.len(),
                })),
            ).await;
```

After the call completes (successfully), before the existing `state.session.append_message(response_msg.clone()).await` line, emit:

```rust
            emit_runtime_event(
                &state.session,
                "llm_call_end",
                Some(llm_start_uuid.clone()),
                Some(serde_json::json!({
                    "input_tokens": response.usage.as_ref().map(|u| u.input_tokens).unwrap_or(0),
                    "output_tokens": response.usage.as_ref().map(|u| u.output_tokens).unwrap_or(0),
                })),
            ).await;
```

(Check field names in `CompletionResponse` / `UsageMetadata`; adjust if they differ.)

- [ ] **Step 10.6: Emit `tool_use` / `tool_result`**

Inside the tool-execution loop in `run.rs` (look for the block that iterates over tool calls extracted from the assistant response — around lines 700-730), add:

Before the actual tool execution:

```rust
                // Session skeleton: tool_use
                let tool_use_uuid = emit_runtime_event(
                    &state.session,
                    "tool_use",
                    Some(llm_start_uuid.clone()),
                    Some(serde_json::json!({
                        "tool_name": tool_call.name.clone(),
                        "tool_call_id": tool_call.id.clone(),
                    })),
                ).await;
                let tool_start_time = std::time::Instant::now();
```

After the tool has executed and returned a result (before the existing `state.session.append_message(tool_msg.clone()).await`):

```rust
                // Session skeleton: tool_result
                emit_runtime_event(
                    &state.session,
                    "tool_result",
                    Some(tool_use_uuid.clone()),
                    Some(serde_json::json!({
                        "tool_call_id": tool_call.id.clone(),
                        "duration_ms": tool_start_time.elapsed().as_millis(),
                        "is_error": tool_output.is_error,
                    })),
                ).await;
```

- [ ] **Step 10.7: Build and test**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: everything builds and passes. If you get errors about `llm_start_uuid` or `iteration_start_uuid` being out of scope, that means a skeleton event is referencing a variable that isn't in scope at that line — trace the control flow and make sure each uuid is accessible where it's used.

- [ ] **Step 10.8: Commit**

```bash
git add -A
git commit -m "feat(kernel-core): emit session skeleton events from run.rs

Runtime now writes run_start / component_registry / iteration_start /
llm_call_start / llm_call_end / tool_use / tool_result / iteration_end /
run_end events inline in run.rs. Every event carries
EventEmitter::runtime() and is parented to the correct enclosing event
per the spec's parent_uuid contract.

Skeleton events are written unconditionally — they do not depend on any
middleware or extension being loaded."
```

---

## Task 11: Wire `ScopedSession` into tool and middleware call sites

**Files:**
- Modify: `crates/alva-kernel-core/src/run.rs`
- Possibly modify: wherever `ToolExecutionContext` instances are constructed (likely `alva-agent-core` or `alva-app-core`)

The goal: when the runtime dispatches a tool, the `ToolExecutionContext` it passes returns `Some(ScopedSession { Tool, tool.name() })` from `session()`. When it invokes a middleware hook, the `MiddlewareContext` carries `Some(ScopedSession { Middleware, middleware.name() })`.

- [ ] **Step 11.1: Find where `ToolExecutionContext` concrete instances are constructed**

Run:

```bash
grep -rn --include='*.rs' 'impl ToolExecutionContext for' crates/
```

The hits should include `MinimalExecutionContext` (already default-None, skip), a production context in agent-core or app-core, and possibly others. For each production context, identify its struct definition.

- [ ] **Step 11.2: Add a `session: Option<ScopedSession>` field to each production `ToolExecutionContext` impl**

For each non-minimal context struct, add:

```rust
pub session: Option<alva_kernel_abi::agent_session::ScopedSession>,
```

and in the `impl ToolExecutionContext`:

```rust
    fn session(&self) -> Option<&alva_kernel_abi::agent_session::ScopedSession> {
        self.session.as_ref()
    }
```

Ensure all existing constructors and call sites that build this struct pass `session: None` by default. This keeps everything compiling.

- [ ] **Step 11.3: Construct the `ScopedSession` when dispatching a tool in `run.rs`**

In `run.rs`, at the point where a tool is about to be invoked (search for the place where the `ToolExecutionContext` is built and passed to `tool.execute(...)`), construct the scoped session like:

```rust
                let scoped_session = alva_kernel_abi::agent_session::ScopedSession::new(
                    state.session.clone(),
                    alva_kernel_abi::agent_session::EventEmitter {
                        kind: alva_kernel_abi::agent_session::EmitterKind::Tool,
                        id: tool.name().to_string(),
                        instance: None,
                    },
                );
```

Then pass it into the context construction (setting the `session: Some(scoped_session)` field).

If `run.rs` does not construct the `ToolExecutionContext` directly (it may come from `agent-core` or `app-core`), then the construction happens upstream and this plan step happens there instead. Follow the trail via the tool dispatch code path.

- [ ] **Step 11.4: Wire middleware context**

At each `configure_all` or `run_before_llm_call` / `run_before_tool_call` invocation in the middleware stack, construct a `MiddlewareContext` with a scoped session stamped with `EmitterKind::Middleware` and the current middleware's name. The middleware stack provides the middleware name; use the `Middleware::name()` method.

Alternative (simpler): only set `session: Some(...)` on the context passed to hooks (not `configure`), and use a per-hook-invocation wrapper that sets the correct middleware name. This keeps the stack itself unchanged.

Open `crates/alva-kernel-core/src/middleware.rs`. Find `configure_all` and the hook-running methods (`run_before_llm_call`, `run_after_llm_call`, `run_before_tool_call`, `run_after_tool_call`, `run_on_agent_start`, `run_on_agent_end`). For each one, where the middleware loop iterates, pass a scoped session constructed per iteration.

This is structural and depends on how the middleware stack is currently wired. If the existing code passes a single `MiddlewareContext` to `configure_all`, you may need to refactor to produce a fresh context per layer. This is optional for this phase — you can also choose to leave `session: None` on middleware contexts for now and let middleware opt in by directly accessing `state.session` via a raw path (not recommended long-term but acceptable as a stopgap).

**For Phase 1 (this plan):** set `session: None` on all constructed `MiddlewareContext` for now. Scoping middleware sessions per-layer is deferred to Phase 2 — it requires a refactor of `MiddlewareStack::configure_all` that is out of scope here. Leave a `TODO(phase-2): per-middleware scoped session` comment at each `session: None` site.

- [ ] **Step 11.5: Build and test**

```bash
cargo build --workspace
cargo test --workspace
```

Expected: success.

- [ ] **Step 11.6: Commit**

```bash
git add -A
git commit -m "feat(kernel-core): wire ScopedSession into tool execution contexts

Production ToolExecutionContext implementations now carry
Option<ScopedSession>. Runtime constructs a Tool-kind scoped session
when dispatching each tool invocation, using the tool's name as the
emitter id.

MiddlewareContext currently gets session: None with a TODO for Phase 2 —
per-layer middleware scoped sessions require a MiddlewareStack refactor
that is deferred."
```

---

## Task 12: Integration test — full run produces a complete event stream

**Files:**
- Create: `crates/alva-kernel-core/tests/session_skeleton.rs`

- [ ] **Step 12.1: Write the integration test**

Create `crates/alva-kernel-core/tests/session_skeleton.rs` with:

```rust
//! Integration test: run a minimal agent with a stub model and verify
//! that the session event stream contains the expected skeleton events
//! in the expected parent chain.

use std::sync::Arc;

use alva_kernel_abi::agent_session::{
    AgentSession, EmitterKind, EventQuery, InMemoryAgentSession,
};
use alva_kernel_abi::{AgentMessage, Message, MessageRole};

// Pull in whatever test helpers kernel-core exposes. If they are not
// public, this test may need to live inside the kernel-core src tree as
// a #[cfg(test)] module instead. Adjust the import path accordingly.
use alva_kernel_core::builtins::test_helpers::helpers::{make_state_with_session, StubModel};

#[tokio::test]
async fn full_run_produces_skeleton_events_in_order() {
    // Build an in-memory session and a minimal agent state wrapping it.
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let state = make_state_with_session(session.clone());

    // Drive a run with a single user input and a stub model that
    // responds once then stops (details depend on test helpers).
    // ... (invoke the run loop or start_run function here)

    // Query the session for all events in order.
    let all_events = session.query(&EventQuery {
        limit: 1000,
        ..Default::default()
    }).await;

    let event_types: Vec<&str> = all_events.iter()
        .map(|em| em.event.event_type.as_str())
        .collect();

    // Assert the skeleton is present.
    assert!(event_types.contains(&"run_start"));
    assert!(event_types.contains(&"component_registry"));
    assert!(event_types.contains(&"iteration_start"));
    assert!(event_types.contains(&"llm_call_start"));
    assert!(event_types.contains(&"llm_call_end"));
    assert!(event_types.contains(&"iteration_end"));
    assert!(event_types.contains(&"run_end"));

    // Assert every event has seq assigned (none are 0).
    for em in &all_events {
        assert_ne!(em.event.seq, 0, "event {} has unassigned seq", em.event.uuid);
    }

    // Assert seq is strictly monotonic.
    let mut prev = 0u64;
    for em in &all_events {
        assert!(em.event.seq > prev, "seq not monotonic: {} !> {}", em.event.seq, prev);
        prev = em.event.seq;
    }

    // Assert run_start is parent of component_registry.
    let run_start = all_events.iter().find(|em| em.event.event_type == "run_start").unwrap();
    let component_registry = all_events.iter().find(|em| em.event.event_type == "component_registry").unwrap();
    assert_eq!(
        component_registry.event.parent_uuid.as_deref(),
        Some(run_start.event.uuid.as_str())
    );

    // Assert runtime is the emitter for every skeleton event.
    for em in &all_events {
        if matches!(em.event.event_type.as_str(),
            "run_start" | "component_registry" | "iteration_start" | "iteration_end" |
            "llm_call_start" | "llm_call_end" | "run_end")
        {
            assert_eq!(em.event.emitter.kind, EmitterKind::Runtime);
            assert_eq!(em.event.emitter.id, "kernel_core");
        }
    }
}
```

Note: this test references `make_state_with_session` which may not exist today. If `make_state` exists but doesn't take a session, either:
- Add a new helper `make_state_with_session(session: Arc<dyn AgentSession>)` to `test_helpers.rs`, or
- Modify this test to use the existing `make_state()` and extract the session from the returned state.

Also: the test needs to actually drive a run. The exact mechanism depends on what test helpers exist. If `kernel-core` exposes a way to invoke `run_loop` or `start_run` from a test, use that. Otherwise this test may need to live inside `kernel-core/src/run.rs` as a `#[cfg(test)]` module where it has access to the internals.

If driving a real run is too invasive for this phase, a simpler variant: just assert that writing events directly to the session works and that seq/emitter/parent chains behave as expected — the "full run" part is then covered by Task 10's existing kernel-core tests which should have been updated.

- [ ] **Step 12.2: Run the test**

```bash
cargo test -p alva-kernel-core --test session_skeleton
```

Expected: pass. If the test cannot be easily driven from an external `tests/` file because the entry points are private, move it to a `#[cfg(test)]` module inside `run.rs`.

- [ ] **Step 12.3: Commit**

```bash
git add -A
git commit -m "test(kernel-core): integration test for session skeleton events

Drives a minimal run and verifies that run_start, component_registry,
iteration_start/end, llm_call_start/end, and run_end events are all
written to the session with correct seq ordering, parent_uuid chains,
and Runtime emitter identity."
```

---

## Post-plan verification

After the final commit, run the full suite one more time:

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Expected: all three succeed.

Then scan for lingering references to the old trait:

```bash
grep -rn --include='*.rs' 'alva_kernel_abi::session::\|use alva_kernel_abi::session\b' crates/
```

Expected: no matches. If anything remains, it is a missed migration point — fix and add a new commit.

Check that the spec's success criteria 1, 3, and 7 are satisfied (criteria 2, 4, 5, 6 are Phase 2/3):

1. ✅ `alva-kernel-abi/src/session.rs` no longer exists — verify with `ls crates/alva-kernel-abi/src/session.rs` returns "No such file".
3. ✅ `alva-kernel-core/src/run.rs` writes the skeleton events — verified by Task 12's integration test.
7. ✅ A ScopedSession-wrapped tool write yields an event with `emitter.kind = Tool` — verified by the `scoped_session_stamps_emitter` unit test in Task 6.

---

## What this plan deliberately does not cover

- **Step 7 of spec §8 (delete SessionTracker, session_store.rs, recorder.rs, store.rs, child_recording.rs)** — these are Phase 3. Phase 1 leaves them untouched.
- **Step 8 of spec §8 (SQLite backend)** — Phase 2.
- **Step 9 of spec §8 (default backend wiring)** — Phase 2. Phase 1 defaults to `InMemoryAgentSession` only.
- **`SessionEventSink` + `TeeAgentSession`** — Phase 3.
- **`SessionRegistry` trait** — Phase 2.
- **Per-middleware scoped session wiring** — noted in Task 11 as a TODO for Phase 2.
- **Moving `alva-agent-context/src/session.rs` (`SessionAccess`) to merge with the new trait** — Phase 3. The two traits coexist until eval/CLI/context crates are migrated.
