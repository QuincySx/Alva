// INPUT:  async_trait, serde, serde_json, uuid, chrono, futures_util, std,
//         crate::AgentMessage
// OUTPUT: AgentSession trait, SessionEvent, SessionMessage, EventEmitter,
//         EmitterKind, ComponentDescriptor, ScopedSession, SessionError,
//         EventQuery, EventMatch, SessionEventListener, SessionEventStream
// POS:    Unified session CONTRACT -- traits + value types only. The concrete
//         in-memory backends (InMemoryAgentSession / ListenableInMemorySession)
//         moved to alva-kernel-core (L2); this crate keeps the pure contract.

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

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
        e.message = Some(SessionMessage {
            role: "user".into(),
            content,
        });
        e
    }

    /// Construct an assistant-message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("assistant");
        e.message = Some(SessionMessage {
            role: "assistant".into(),
            content,
        });
        e
    }

    /// Construct a tool_result event linked to a parent tool_use uuid.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        let mut e = Self::new("tool_result");
        e.parent_uuid = Some(parent_tool_use_uuid.to_string());
        e.message = Some(SessionMessage {
            role: "tool".into(),
            content,
        });
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
/// ## Durability contract
///
/// `append` and `append_message` return `()`. This is deliberate — the
/// implementation owns durability and error recovery entirely:
///
/// - **Transient failures** (network blips, disk pressure, broker
///   reconnects, etc.) MUST be handled internally: buffer, retry, or
///   dead-letter locally. Do NOT drop events silently.
/// - **Permanent failures** (archived session, expired auth, quota exceeded,
///   backend wedged beyond retry) MUST be surfaced on the next call to
///   `flush` / `close`, carried in `SessionError::Other` or a more
///   specific variant. Per-event result returns are intentionally not
///   provided — hot-path callers cannot meaningfully handle each append's
///   fate, and `flush` already exists as the sync point.
/// - The runtime guarantees `flush` is invoked periodically (see Lifecycle
///   contract below), so permanent failures surface within a bounded window.
///
/// In-memory backends may no-op everything; remote/persistent backends
/// are expected to maintain an internal write queue with back-pressure
/// and escalate unrecoverable errors at flush time.
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
    ///
    /// Returns `()` — see the "Durability contract" on the trait doc.
    /// Transient failures are absorbed internally; permanent failures
    /// surface on the next `flush` / `close`.
    async fn append(&self, event: SessionEvent);

    /// Append an `AgentMessage` as a user / assistant / tool_result event.
    /// The backend translates the message into a `SessionEvent` with
    /// `emitter = Runtime` and the given `parent_uuid` (or None for unparented
    /// messages) and appends it.
    ///
    /// Same durability contract as `append`.
    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>);

    // --- Read: event-level ---

    /// Query events matching the filter, ordered by `seq` ascending.
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    /// Count events matching the filter.
    async fn count(&self, filter: &EventQuery) -> usize;

    // --- Read: message-level (hot path for LLM input assembly) ---

    /// All messages in append order, projected from events.
    ///
    /// **Must be served from an O(1) local projection.** This is called on
    /// the hot path of `run_agent` (every iteration before the LLM request),
    /// so implementations MUST NOT round-trip to remote storage on each
    /// call. Remote backends should implement `SubscribableSession` and
    /// keep a local mirror warm from that stream.
    async fn messages(&self) -> Vec<AgentMessage>;

    /// The last N messages, projected from events. Same cache requirement
    /// as `messages`.
    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage>;

    /// Return messages whose source event has `seq > after_seq`, in append
    /// order. Used by remote backends to incrementally sync a local mirror
    /// without re-fetching the full history each turn.
    ///
    /// The default implementation scans the event log via `query` and
    /// deserializes the `AgentMessage` out of each event's `data` field,
    /// so it matches `messages()` in cost. Network-backed backends should
    /// override this with an efficient single-roundtrip query (e.g. a
    /// server-side filter on `seq`).
    ///
    /// Events without a serialized `AgentMessage` in `data` (progress,
    /// skeleton, extension events) are skipped.
    async fn messages_since(&self, after_seq: u64) -> Vec<AgentMessage> {
        let matches = self.query(&EventQuery::default()).await;
        matches
            .into_iter()
            .filter(|m| m.event.seq > after_seq)
            .filter_map(|m| {
                m.event
                    .data
                    .and_then(|d| serde_json::from_value::<AgentMessage>(d).ok())
            })
            .collect()
    }

    // --- Subscription ---

    /// Subscribe to the session's event stream, returning a stream that
    /// yields (in order):
    ///
    /// 1. All events currently in the log with `seq > from_seq`.
    /// 2. All subsequently appended events, for as long as the stream is
    ///    held — **if the backend supports live tailing**.
    ///
    /// The default implementation covers (1) only: a one-shot snapshot
    /// of historical events that ends immediately after replay. Backends
    /// that support broadcasting new events to subscribers
    /// (e.g. `ListenableInMemorySession`, remote HTTP/SSE backends) MUST
    /// override to provide both (1) and (2) with no gap or duplication at
    /// the boundary.
    ///
    /// Typical uses:
    /// - `from_seq = 0` — "give me everything". Remote backends use this
    ///   at construction time to populate a local mirror; live-tail impls
    ///   then keep it warm.
    /// - `from_seq = last_seen` — resume after a reader disconnect.
    ///
    /// Dropping the returned stream unsubscribes. Cleanup of internal
    /// listener state is allowed to be lazy.
    async fn subscribe_events(&self, from_seq: u64) -> SessionEventStream {
        let matches = self.query(&EventQuery::default()).await;
        let history: Vec<SessionEvent> = matches
            .into_iter()
            .filter(|m| m.event.seq > from_seq)
            .map(|m| m.event)
            .collect();
        Box::pin(futures_util::stream::iter(history))
    }

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

    /// Replace the live conversation projection with `replacement` while
    /// PRESERVING the append-only event log.
    ///
    /// This is what compaction uses instead of `clear()`. `clear()` wipes the
    /// entire log — skeleton/causal events (run_start, llm_call_*, tool_use)
    /// included — which destroys the Inspector view, eval replay, and audit
    /// trail. `compact_messages` rewrites only the message projection the
    /// agent loop re-reads each turn; the historical events stay.
    ///
    /// The caller appends a `compaction` marker event immediately BEFORE
    /// calling this. That marker is the reconstruction reset point: a backend
    /// that rebuilds its projection from the log (see
    /// `InMemoryAgentSession::restore_events`) starts the message projection
    /// fresh at the last `compaction` marker, so a reload reproduces exactly
    /// `replacement` rather than the full pre-compaction history.
    ///
    /// The default implementation is the older destructive path (clear +
    /// re-append) for backends that cannot separate projection from log; the
    /// in-memory backends override it to keep the log intact.
    async fn compact_messages(&self, replacement: Vec<AgentMessage>) {
        let _ = self.clear().await;
        for msg in replacement {
            self.append_message(msg, None).await;
        }
    }
}

// ===========================================================================
// Session event stream
// ===========================================================================

/// Stream of `SessionEvent`s. Returned by `AgentSession::subscribe_events`.
pub type SessionEventStream = futures_util::stream::BoxStream<'static, SessionEvent>;

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
// SessionEventListener (contract)
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

    /// Is this listener still worth broadcasting to? A broadcasting session
    /// reaps listeners that report `false` after each notify, so a listener
    /// backed by a dropped consumer (e.g. a `subscribe_events` stream that
    /// was dropped) does not accumulate forever. Defaults to `true` — a
    /// listener with no liveness signal is treated as permanent (the right
    /// default for forwarders that live as long as their target).
    fn is_active(&self) -> bool {
        true
    }
}
