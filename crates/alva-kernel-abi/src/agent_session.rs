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

    /// Construct a runtime event with the given `event_type`. Used by
    /// kernel-core to emit skeleton events.
    pub fn new_runtime(event_type: impl Into<String>) -> Self {
        Self::new(event_type)
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
    /// `emitter = Runtime` and the given `parent_uuid` (or None for unparented
    /// messages) and appends it.
    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>);

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

    /// Return a clone of the wrapped AgentSession handle. Callers that
    /// bypass emitter stamping (e.g. to forward events with their original
    /// emitter intact) use this escape hatch.
    pub fn inner(&self) -> Arc<dyn AgentSession> {
        self.inner.clone()
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

    /// Classify an `AgentMessage` into an `event_type` string and an optional
    /// `SessionMessage` for display. Used by `append_message` when building
    /// the corresponding `SessionEvent`.
    ///
    /// The full original `AgentMessage` is NOT represented here — it gets
    /// serialized into the event's `data` field by `append_message` for
    /// perfect round-trip on rollback. This method only produces what
    /// `query` / preview consumers need for display.
    fn classify_message(msg: &AgentMessage) -> (String, Option<SessionMessage>) {
        use crate::base::message::MessageRole;

        // Derive event_type per variant.
        let event_type = match msg {
            AgentMessage::Standard(m) => match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool_result",
            },
            AgentMessage::Steering(_) => "user_steering",
            AgentMessage::FollowUp(_) => "system_followup",
            AgentMessage::Marker(_) => "marker",
            AgentMessage::Extension { type_name, .. } => {
                // Extension events carry no SessionMessage.
                return (format!("extension:{}", type_name), None);
            }
        };

        // Extract the inner Message for the three variants that have one.
        let m = match msg {
            AgentMessage::Standard(m)
            | AgentMessage::Steering(m)
            | AgentMessage::FollowUp(m) => m,
            AgentMessage::Marker(_) => {
                // Markers carry no message content.
                return (event_type.to_string(), None);
            }
            AgentMessage::Extension { .. } => unreachable!("handled above"),
        };

        let role_str = match m.role {
            MessageRole::User => "user",
            MessageRole::Assistant => "assistant",
            MessageRole::System => "system",
            MessageRole::Tool => "tool",
        };
        let content = serde_json::to_value(&m.content)
            .unwrap_or_else(|_| serde_json::json!([]));
        let session_msg = SessionMessage {
            role: role_str.to_string(),
            content,
        };

        (event_type.to_string(), Some(session_msg))
    }
}

#[async_trait]
impl AgentSession for InMemoryAgentSession {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn parent_session_id(&self) -> Option<&str> {
        self.parent_session_id.as_deref()
    }

    async fn append(&self, mut event: SessionEvent) {
        // Assign seq atomically. This is the only place seq is assigned
        // on the raw-event write path.
        event.seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        // Raw events go into the event log ONLY. Message-bearing events
        // should use append_message so that both the log and the cache
        // stay consistent.
        self.events.write().await.push(event);
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        // Classify for display, serialize for perfect round-trip.
        let (event_type, session_msg) = Self::classify_message(&msg);
        let mut event = SessionEvent::new(event_type);
        event.parent_uuid = parent_uuid;
        event.message = session_msg;
        event.data = Some(
            serde_json::to_value(&msg).unwrap_or(serde_json::Value::Null),
        );
        event.seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);

        // Push to events log and directly to the message cache.
        // The cache holds the original AgentMessage — no round-trip.
        self.events.write().await.push(event);
        self.messages.write().await.push_back(msg);
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
        let Some(pos) = events.iter().position(|e| e.uuid == *uuid) else {
            return 0;
        };

        let removed = events.len() - pos - 1;
        events.truncate(pos + 1);

        // Clone the surviving events for cache rebuild, then drop the
        // events lock before acquiring the messages lock.
        let surviving: Vec<SessionEvent> = events.iter().cloned().collect();
        drop(events);

        // Rebuild the message cache by deserializing the AgentMessage
        // from each surviving event's `data` field. Events without a
        // serialized AgentMessage in `data` (progress, hooks, skeleton
        // events) are skipped — they were never in the cache.
        let mut msgs = self.messages.write().await;
        msgs.clear();
        for ev in &surviving {
            if let Some(data) = &ev.data {
                if let Ok(m) = serde_json::from_value::<AgentMessage>(data.clone()) {
                    msgs.push_back(m);
                }
            }
        }

        removed
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
// SessionEventListener + ListenableInMemorySession
// ===========================================================================

/// Observer that receives a copy of every `SessionEvent` written to a
/// `ListenableInMemorySession`. Listeners are called in subscription order,
/// synchronously within each write call (after the write itself completes),
/// so they see the assigned `seq`.
///
/// Typical use: forward child-agent events into a parent session so that
/// the parent's event log contains a complete nested sub-run view.
#[async_trait]
pub trait SessionEventListener: Send + Sync {
    async fn on_event(&self, event: &SessionEvent);
}

/// An `InMemoryAgentSession` wrapper that broadcasts every written event to
/// a list of `SessionEventListener`s after the write completes.
///
/// Lives in the same module as `InMemoryAgentSession` so it can access
/// its private fields directly for the combined seq-assign + notify pattern.
pub struct ListenableInMemorySession {
    inner: InMemoryAgentSession,
    listeners: RwLock<Vec<Arc<dyn SessionEventListener>>>,
}

impl ListenableInMemorySession {
    /// Create a fresh root listenable session.
    pub fn new() -> Self {
        Self {
            inner: InMemoryAgentSession::new(),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Create a child listenable session linked to a parent session id.
    pub fn with_parent(parent_id: impl Into<String>) -> Self {
        Self {
            inner: InMemoryAgentSession::with_parent(parent_id),
            listeners: RwLock::new(Vec::new()),
        }
    }

    /// Register a listener. It will receive every event written after
    /// this call completes.
    pub async fn subscribe(&self, listener: Arc<dyn SessionEventListener>) {
        self.listeners.write().await.push(listener);
    }
}

impl Default for ListenableInMemorySession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentSession for ListenableInMemorySession {
    fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    fn parent_session_id(&self) -> Option<&str> {
        self.inner.parent_session_id()
    }

    async fn append(&self, event: SessionEvent) {
        // Assign seq directly via inner's atomic counter, then push to
        // inner's events vec. We do NOT call self.inner.append() so that
        // we have the post-seq-assign event in hand to broadcast.
        let mut e = event;
        e.seq = self.inner.seq_counter.fetch_add(1, Ordering::SeqCst);
        self.inner.events.write().await.push(e.clone());

        let listeners = self.listeners.read().await;
        for l in listeners.iter() {
            l.on_event(&e).await;
        }
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        // Replicate InMemoryAgentSession::append_message logic so we can
        // hold the fully-constructed event for broadcasting.
        let (event_type, session_msg) = InMemoryAgentSession::classify_message(&msg);
        let mut event = SessionEvent::new(event_type);
        event.message = session_msg;
        event.parent_uuid = parent_uuid;
        event.data = Some(
            serde_json::to_value(&msg).unwrap_or(serde_json::Value::Null),
        );
        // Assign seq via inner's counter (same-module private field access).
        event.seq = self.inner.seq_counter.fetch_add(1, Ordering::SeqCst);

        // Store in inner's fields.
        self.inner.events.write().await.push(event.clone());
        self.inner.messages.write().await.push_back(msg);

        // Notify listeners.
        let listeners = self.listeners.read().await;
        for l in listeners.iter() {
            l.on_event(&event).await;
        }
    }

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.inner.query(filter).await
    }

    async fn count(&self, filter: &EventQuery) -> usize {
        self.inner.count(filter).await
    }

    async fn messages(&self) -> Vec<AgentMessage> {
        self.inner.messages().await
    }

    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage> {
        self.inner.recent_messages(n).await
    }

    async fn rollback_after(&self, uuid: &str) -> usize {
        self.inner.rollback_after(uuid).await
    }

    async fn save_snapshot(&self, data: &[u8]) {
        self.inner.save_snapshot(data).await
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.inner.load_snapshot().await
    }

    async fn restore(&self) -> Result<(), SessionError> {
        self.inner.restore().await
    }

    async fn flush(&self) -> Result<(), SessionError> {
        self.inner.flush().await
    }

    async fn close(&self) -> Result<(), SessionError> {
        self.inner.close().await
    }

    async fn clear(&self) -> Result<(), SessionError> {
        self.inner.clear().await
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

// Note: no `event_to_message` helper is needed. `append_message` serializes
// the full `AgentMessage` into `event.data`, and `rollback_after` deserializes
// it back during cache rebuild. The message cache holds the original
// `AgentMessage` values as they were passed in by the caller — no round-trip
// through `SessionMessage` is required, preserving variant information
// (`Steering`, `FollowUp`, `Marker`, `Extension`) perfectly.

impl Default for InMemoryAgentSession {
    fn default() -> Self {
        Self::new()
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::message::Message;

    fn user_msg(text: &str) -> AgentMessage {
        // Use the `Message::user` factory — the `Message` struct has 6 fields
        // (id, role, content, tool_call_id, usage, timestamp) and the factory
        // fills them sensibly.
        AgentMessage::Standard(Message::user(text))
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
        s.append_message(user_msg("hello"), None).await;
        s.append_message(user_msg("world"), None).await;

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
            s.append_message(user_msg(&format!("msg {}", i)), None).await;
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
        s.append_message(user_msg("one"), None).await;
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
        s.append_message(user_msg("one"), None).await;
        s.append_message(user_msg("two"), None).await;
        s.append_message(user_msg("three"), None).await;

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
        s.append_message(user_msg("one"), None).await;
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

    // -----------------------------------------------------------------------
    // ListenableInMemorySession tests
    // -----------------------------------------------------------------------

    struct TestListener {
        received: Arc<tokio::sync::Mutex<Vec<SessionEvent>>>,
    }

    #[async_trait]
    impl SessionEventListener for TestListener {
        async fn on_event(&self, event: &SessionEvent) {
            self.received.lock().await.push(event.clone());
        }
    }

    fn make_test_listener() -> (Arc<TestListener>, Arc<tokio::sync::Mutex<Vec<SessionEvent>>>) {
        let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let listener = Arc::new(TestListener { received: received.clone() });
        (listener, received)
    }

    #[tokio::test]
    async fn listenable_session_notifies_listener_on_append() {
        let session = ListenableInMemorySession::new();
        let (listener, received) = make_test_listener();
        session.subscribe(listener).await;

        let e = SessionEvent::progress(serde_json::json!({"n": 42}));
        session.append(e).await;

        let got = received.lock().await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].seq, 1);
        assert_eq!(got[0].event_type, "progress");
    }

    #[tokio::test]
    async fn listenable_session_notifies_on_append_message() {
        let session = ListenableInMemorySession::new();
        let (listener, received) = make_test_listener();
        session.subscribe(listener).await;

        session.append_message(user_msg("hello"), None).await;

        let got = received.lock().await;
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].seq, 1);
        assert_eq!(got[0].event_type, "user");
        // Verify data holds the original AgentMessage (serialized with tag "kind").
        let data = got[0].data.as_ref().expect("data should be set");
        assert_eq!(
            data.get("kind").and_then(|v| v.as_str()),
            Some("Standard"),
            "data should hold AgentMessage::Standard"
        );
    }

    #[tokio::test]
    async fn listenable_session_multiple_listeners_fire_in_order() {
        let session = ListenableInMemorySession::new();
        let (l1, r1) = make_test_listener();
        let (l2, r2) = make_test_listener();
        session.subscribe(l1).await;
        session.subscribe(l2).await;

        session.append(SessionEvent::progress(serde_json::json!({"x": 1}))).await;

        assert_eq!(r1.lock().await.len(), 1, "first listener should fire");
        assert_eq!(r2.lock().await.len(), 1, "second listener should fire");
    }

    #[tokio::test]
    async fn listenable_session_nested_forward() {
        // ForwardToSession listener defined inline.
        struct ForwardToSession {
            target: Arc<dyn AgentSession>,
        }

        #[async_trait]
        impl SessionEventListener for ForwardToSession {
            async fn on_event(&self, event: &SessionEvent) {
                self.target.append(event.clone()).await;
            }
        }

        let parent = Arc::new(ListenableInMemorySession::new());
        let child = Arc::new(ListenableInMemorySession::new());

        // Attach forwarder: child events -> parent session.
        child.subscribe(Arc::new(ForwardToSession {
            target: parent.clone() as Arc<dyn AgentSession>,
        })).await;

        // Append to child — should appear in parent via the listener.
        child.append(SessionEvent::progress(serde_json::json!({"from": "child"}))).await;

        let parent_events = parent.inner.events.read().await;
        assert_eq!(parent_events.len(), 1, "parent should have received the child event");
        assert_eq!(parent_events[0].event_type, "progress");
    }
}
