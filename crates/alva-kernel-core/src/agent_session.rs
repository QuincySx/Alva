// INPUT:  async_trait, serde_json, uuid, futures_util, tokio, std,
//         alva_kernel_abi (AgentSession contract + AgentMessage)
// OUTPUT: InMemoryAgentSession, ListenableInMemorySession -- concrete in-memory
//         AgentSession backends. The contract is re-exported for one-path use.
// POS:    Concrete AgentSession implementations. The contract (AgentSession /
//         SessionEvent / EventQuery / ... traits + value types) lives in
//         alva-kernel-abi (L1); these are the in-memory backends that implement
//         it. Re-exporting the contract lets consumers pull both the traits and
//         the backends from alva_kernel_core::agent_session with one import.

pub use alva_kernel_abi::agent_session::{
    AgentSession, ComponentDescriptor, EmitterKind, EventEmitter, EventMatch, EventQuery,
    ScopedSession, SessionError, SessionEvent, SessionEventListener, SessionEventStream,
    SessionMessage,
};

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use alva_kernel_abi::base::message::AgentMessage;

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

    /// Replay a batch of pre-persisted `SessionEvent`s into this in-memory
    /// session — used by storage backends (SQLite, JSON file, etc.) to
    /// rebuild state from disk at startup.
    ///
    /// Unlike [`AgentSession::append`] (which assigns a fresh seq and only
    /// touches the event log), this method:
    /// 1. Preserves each event's original `seq` — no renumbering
    /// 2. Rebuilds the `messages` projection by deserializing `event.data`
    ///    for events whose role indicates a `AgentMessage` payload
    /// 3. Advances `seq_counter` past the largest restored seq so future
    ///    writes don't collide
    ///
    /// Without this method, backends that replay raw events via `append`
    /// end up with an empty `messages` cache — `session.messages()` then
    /// returns `[]` on reload, which the UI renders as "history wiped".
    pub async fn restore_events(&self, events: Vec<SessionEvent>) {
        let mut max_seq: u64 = 0;
        let mut rebuilt_messages: VecDeque<AgentMessage> = VecDeque::new();
        for event in &events {
            if event.seq > max_seq {
                max_seq = event.seq;
            }
            // Events originally written via `append_message` carry the
            // full AgentMessage in `data`. That's the authoritative source
            // for rebuilding the messages cache.
            if let Some(data) = &event.data {
                if let Ok(msg) = serde_json::from_value::<AgentMessage>(data.clone()) {
                    rebuilt_messages.push_back(msg);
                }
            }
        }
        {
            let mut log = self.events.write().await;
            *log = events;
        }
        {
            let mut msgs = self.messages.write().await;
            *msgs = rebuilt_messages;
        }
        // Advance the counter past every restored seq, so next `append`
        // or `append_message` gets a unique new seq.
        let next = max_seq.saturating_add(1);
        let current = self.seq_counter.load(Ordering::SeqCst);
        if next > current {
            self.seq_counter.store(next, Ordering::SeqCst);
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
        use alva_kernel_abi::base::message::MessageRole;

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
            AgentMessage::Standard(m) | AgentMessage::Steering(m) | AgentMessage::FollowUp(m) => m,
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
        let content = serde_json::to_value(&m.content).unwrap_or_else(|_| serde_json::json!([]));
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
        // Seq assignment and push share ONE critical section: concurrent
        // appenders would otherwise grab seqs in one order and win the lock
        // in another, storing events out of seq order (e.g. a tool_result
        // ahead of its tool_use). Storage order == seq order is the contract
        // every replay/projection consumer builds on.
        //
        // Raw events go into the event log ONLY. Message-bearing events
        // should use append_message so that both the log and the cache
        // stay consistent.
        let mut events = self.events.write().await;
        event.seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        events.push(event);
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        // Classify for display, serialize for perfect round-trip.
        let (event_type, session_msg) = Self::classify_message(&msg);
        let mut event = SessionEvent::new_runtime(event_type);
        event.parent_uuid = parent_uuid;
        event.message = session_msg;
        event.data = Some(serde_json::to_value(&msg).unwrap_or(serde_json::Value::Null));

        // One critical section for seq + both stores (lock order events →
        // messages, same as restore/rollback), so the log, the message
        // cache, and seq order can never diverge. The cache holds the
        // original AgentMessage — no round-trip.
        let mut events = self.events.write().await;
        let mut messages = self.messages.write().await;
        event.seq = self.seq_counter.fetch_add(1, Ordering::SeqCst);
        events.push(event);
        messages.push_back(msg);
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
// ListenableInMemorySession
// ===========================================================================
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

    /// Broadcast one event to every listener, then reap any that reported
    /// themselves inactive. The notify loop runs under a read guard (many
    /// appends can broadcast concurrently); the reap takes the write guard
    /// only when something actually died, so the steady state (all live)
    /// pays no extra lock. Broadcast happens OUTSIDE any inner-session lock —
    /// listeners run arbitrary async code and may read this very session.
    async fn broadcast(&self, event: &SessionEvent) {
        let mut any_dead = false;
        {
            let listeners = self.listeners.read().await;
            for l in listeners.iter() {
                l.on_event(event).await;
                if !l.is_active() {
                    any_dead = true;
                }
            }
        }
        if any_dead {
            self.listeners.write().await.retain(|l| l.is_active());
        }
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
        // Assign seq inside inner's events write lock (same critical section
        // as the push — see InMemoryAgentSession::append for why), then
        // broadcast OUTSIDE the lock: listeners run arbitrary async code and
        // may read this very session, which would deadlock under the held
        // write guard. We do NOT call self.inner.append() so that we have
        // the post-seq-assign event in hand to broadcast.
        let mut e = event;
        {
            let mut events = self.inner.events.write().await;
            e.seq = self.inner.seq_counter.fetch_add(1, Ordering::SeqCst);
            events.push(e.clone());
        }

        self.broadcast(&e).await;
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        // Replicate InMemoryAgentSession::append_message logic so we can
        // hold the fully-constructed event for broadcasting.
        let (event_type, session_msg) = InMemoryAgentSession::classify_message(&msg);
        let mut event = SessionEvent::new_runtime(event_type);
        event.message = session_msg;
        event.parent_uuid = parent_uuid;
        event.data = Some(serde_json::to_value(&msg).unwrap_or(serde_json::Value::Null));

        // One critical section for seq + both stores (lock order events →
        // messages, same as InMemoryAgentSession::append_message); broadcast
        // after release — listeners may read this session.
        {
            let mut events = self.inner.events.write().await;
            let mut messages = self.inner.messages.write().await;
            event.seq = self.inner.seq_counter.fetch_add(1, Ordering::SeqCst);
            events.push(event.clone());
            messages.push_back(msg);
        }

        self.broadcast(&event).await;
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

    /// Override: return a stream that yields historical events with
    /// `seq > from_seq`, then follows live events appended thereafter.
    ///
    /// History snapshot and listener registration happen under the events
    /// write lock, so no event is dropped or duplicated across the
    /// historical-to-live boundary.
    async fn subscribe_events(&self, from_seq: u64) -> SessionEventStream {
        use futures_util::stream::{self, StreamExt};

        let (tx, rx) = tokio::sync::mpsc::unbounded_channel::<SessionEvent>();

        // Atomically snapshot history AND register the listener under the
        // events write lock. Any `append` takes `events.write()` first, so
        // while we hold that lock no new event can be published to the log.
        // After we release, new appends go to both the log and the listener.
        // Events with seq <= the snapshot high-water are in (1); events
        // after are in (2). No dup, no loss.
        let events_guard = self.inner.events.write().await;
        let history: Vec<SessionEvent> = events_guard
            .iter()
            .filter(|e| e.seq > from_seq)
            .cloned()
            .collect();

        let listener: Arc<dyn SessionEventListener> = Arc::new(ChannelListener { tx });
        self.listeners.write().await.push(listener);
        drop(events_guard);

        let history_stream = stream::iter(history);
        let live_stream =
            stream::unfold(rx, |mut rx| async move { rx.recv().await.map(|e| (e, rx)) });

        Box::pin(history_stream.chain(live_stream))
    }
}

// Listener that funnels received events into an mpsc channel. Used by
// `ListenableInMemorySession::subscribe_events` to back the returned
// stream.
struct ChannelListener {
    tx: tokio::sync::mpsc::UnboundedSender<SessionEvent>,
}

#[async_trait]
impl SessionEventListener for ChannelListener {
    async fn on_event(&self, event: &SessionEvent) {
        // Receiver dropped → stream was dropped by consumer. Send fails
        // silently; `is_active` then reports dead so the session reaps us.
        let _ = self.tx.send(event.clone());
    }

    fn is_active(&self) -> bool {
        !self.tx.is_closed()
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
    use alva_kernel_abi::base::message::{Message, MessageRole};

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

    /// STORAGE order must equal seq order under concurrent appends — the
    /// projection/live-tail layers replay the events vec as-is, so an event
    /// stored out of seq order (e.g. a `tool_result` landing before its
    /// `tool_use`) corrupts every consumer downstream. Requires a
    /// multi-thread runtime: on `current_thread` the appends serialize and
    /// the race can never manifest. Do NOT sort before asserting — sorting
    /// is exactly what would hide the bug this test exists to catch.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_append_preserves_monotonic_seq() {
        use std::sync::Arc;

        let s = Arc::new(InMemoryAgentSession::new());
        let mut handles = Vec::new();
        for t in 0..8 {
            let s = s.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..250 {
                    let e = SessionEvent::progress(serde_json::json!({"t": t, "i": i}));
                    s.append(e).await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let events = s.events.read().await;
        assert_eq!(events.len(), 2000);

        // Strict: seq in STORAGE order must be exactly 1, 2, 3, … — dense,
        // duplicate-free, AND in place. seq is assigned inside the events
        // write lock, so insertion order and seq order cannot diverge.
        for (i, e) in events.iter().enumerate() {
            assert_eq!(
                e.seq,
                (i + 1) as u64,
                "storage order diverged from seq order at index {i}"
            );
        }
    }

    /// Same invariant for the message path, which additionally keeps the
    /// message cache in step with the event log under one critical section.
    #[tokio::test(flavor = "multi_thread", worker_threads = 8)]
    async fn concurrent_append_message_preserves_storage_order() {
        use std::sync::Arc;

        let s = Arc::new(InMemoryAgentSession::new());
        let mut handles = Vec::new();
        for t in 0..8 {
            let s = s.clone();
            handles.push(tokio::spawn(async move {
                for i in 0..250 {
                    s.append_message(user_msg(&format!("{t}-{i}")), None).await;
                }
            }));
        }
        for h in handles {
            h.await.unwrap();
        }

        let events = s.events.read().await;
        assert_eq!(events.len(), 2000);
        for (i, e) in events.iter().enumerate() {
            assert_eq!(
                e.seq,
                (i + 1) as u64,
                "storage order diverged from seq order at index {i}"
            );
        }
        drop(events);
        assert_eq!(s.messages().await.len(), 2000);
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
            s.append_message(user_msg(&format!("msg {}", i)), None)
                .await;
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
    async fn messages_since_returns_only_newer_messages() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("one"), None).await; // seq 1
        s.append_message(user_msg("two"), None).await; // seq 2
        s.append_message(user_msg("three"), None).await; // seq 3

        assert_eq!(s.messages_since(0).await.len(), 3);
        assert_eq!(s.messages_since(1).await.len(), 2);
        assert_eq!(s.messages_since(2).await.len(), 1);
        assert_eq!(s.messages_since(3).await.len(), 0);
        assert_eq!(s.messages_since(99).await.len(), 0);
    }

    #[tokio::test]
    async fn messages_since_skips_non_message_events() {
        let s = InMemoryAgentSession::new();
        s.append_message(user_msg("m1"), None).await; // seq 1 (message)
        s.append(SessionEvent::progress(serde_json::json!({"p": 1})))
            .await; // seq 2 (no data-backed message)
        s.append_message(user_msg("m2"), None).await; // seq 3 (message)

        // Default impl deserializes from event.data — only the two messages
        // round-trip; the raw progress event has `data` but it's not an
        // AgentMessage, so it's skipped.
        let all = s.messages_since(0).await;
        assert_eq!(all.len(), 2);
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
        s.append(SessionEvent::user_message(serde_json::json!("hi")))
            .await;
        s.append(SessionEvent::progress(serde_json::json!({"ok": true})))
            .await;
        s.append(SessionEvent::progress(serde_json::json!({"ok": false})))
            .await;

        let progress = s
            .query(&EventQuery {
                event_type: Some("progress".into()),
                limit: 100,
                ..Default::default()
            })
            .await;
        assert_eq!(progress.len(), 2);

        let users = s
            .query(&EventQuery {
                event_type: Some("user".into()),
                limit: 100,
                ..Default::default()
            })
            .await;
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
    async fn restore_events_rebuilds_both_event_log_and_messages_cache() {
        // Build a session, write a user + assistant message through the
        // real write path (append_message), pull the raw events out, then
        // replay them into a FRESH session via restore_events. The fresh
        // session should match the original on both `.events()` and
        // `.messages()` — proving that a backend round-trip through
        // SQLite/JSON files rebuilds the messages cache correctly. Regression
        // guard for: https://github.com/.../issues/… (history missing after
        // app restart — cache stays empty after `append` replay).
        let original = InMemoryAgentSession::new();
        original.append_message(user_msg("hi"), None).await;
        original
            .append_message(
                AgentMessage::Standard(Message {
                    id: "m2".into(),
                    role: MessageRole::Assistant,
                    content: vec![alva_kernel_abi::base::content::ContentBlock::Text {
                        text: "hello there".into(),
                    }],
                    tool_call_id: None,
                    usage: None,
                    timestamp: 0,
                }),
                None,
            )
            .await;
        let captured_events: Vec<SessionEvent> =
            original.events.read().await.iter().cloned().collect();
        assert_eq!(captured_events.len(), 2);
        assert_eq!(original.messages().await.len(), 2);

        // Fresh session — plain `append` would leave messages cache empty.
        let restored = InMemoryAgentSession::new();
        restored.restore_events(captured_events.clone()).await;

        // Both views must match the original.
        let replayed_events = restored.events.read().await;
        assert_eq!(replayed_events.len(), 2);
        // Seq numbers must be preserved, not reassigned.
        assert_eq!(replayed_events[0].seq, captured_events[0].seq);
        assert_eq!(replayed_events[1].seq, captured_events[1].seq);
        drop(replayed_events);

        let replayed_msgs = restored.messages().await;
        assert_eq!(replayed_msgs.len(), 2);

        // Next append must take a seq past the largest restored — no collisions.
        restored
            .append(SessionEvent::progress(
                serde_json::json!({"after": "restore"}),
            ))
            .await;
        let log = restored.events.read().await;
        let new_event = log.last().unwrap();
        let max_restored = captured_events.iter().map(|e| e.seq).max().unwrap();
        assert!(new_event.seq > max_restored);
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
        s.append(SessionEvent::progress(
            serde_json::json!({"after": "clear"}),
        ))
        .await;
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

    fn make_test_listener() -> (
        Arc<TestListener>,
        Arc<tokio::sync::Mutex<Vec<SessionEvent>>>,
    ) {
        let received = Arc::new(tokio::sync::Mutex::new(Vec::new()));
        let listener = Arc::new(TestListener {
            received: received.clone(),
        });
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

        session
            .append(SessionEvent::progress(serde_json::json!({"x": 1})))
            .await;

        assert_eq!(r1.lock().await.len(), 1, "first listener should fire");
        assert_eq!(r2.lock().await.len(), 1, "second listener should fire");
    }

    #[tokio::test]
    async fn default_subscribe_events_replays_history_and_ends() {
        use futures_util::StreamExt;

        // InMemoryAgentSession doesn't override subscribe_events, so it
        // gets the default impl: one-shot history snapshot, no live tail.
        let s = InMemoryAgentSession::new();
        s.append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;
        s.append(SessionEvent::progress(serde_json::json!({"n": 2})))
            .await;

        let mut stream = s.subscribe_events(0).await;
        let e1 = stream.next().await.expect("seq 1");
        assert_eq!(e1.seq, 1);
        let e2 = stream.next().await.expect("seq 2");
        assert_eq!(e2.seq, 2);

        // Default impl has no live tail — stream ends after history.
        assert!(
            stream.next().await.is_none(),
            "default impl must end after history"
        );
    }

    #[tokio::test]
    async fn subscribe_events_replays_history_then_tails_live() {
        use futures_util::StreamExt;

        let session = ListenableInMemorySession::new();
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 2})))
            .await;

        // Subscribe from seq 0: replay seq 1 and 2, then wait on live.
        let mut stream = session.subscribe_events(0).await;

        let e1 = stream.next().await.expect("seq 1 should replay");
        assert_eq!(e1.seq, 1);
        let e2 = stream.next().await.expect("seq 2 should replay");
        assert_eq!(e2.seq, 2);

        // Append a live event after subscription; stream should yield it.
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 3})))
            .await;
        let e3 = stream.next().await.expect("seq 3 should arrive live");
        assert_eq!(e3.seq, 3);
    }

    #[tokio::test]
    async fn subscribe_events_from_seq_skips_early_history() {
        use futures_util::StreamExt;

        let session = ListenableInMemorySession::new();
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 2})))
            .await;
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 3})))
            .await;

        // Subscribe after seq 1: history yields seq 2 and 3 only.
        let mut stream = session.subscribe_events(1).await;
        let e = stream.next().await.unwrap();
        assert_eq!(e.seq, 2);
        let e = stream.next().await.unwrap();
        assert_eq!(e.seq, 3);
    }

    #[tokio::test]
    async fn subscribe_events_dropping_stream_does_not_panic_future_appends() {
        let session = ListenableInMemorySession::new();
        {
            let _stream = session.subscribe_events(0).await;
            // drop stream at end of scope
        }
        // Subsequent appends must succeed (no panic / no hang) even though
        // the ChannelListener's receiver is gone.
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;
        let events = session.inner.events.read().await;
        assert_eq!(events.len(), 1);
    }

    /// D-3 regression: a subscription whose stream was dropped must be
    /// REAPED, not kept forever. On a long-lived parent session a live-tail
    /// subscriber that comes and goes (each `subscribe_events` pushes a
    /// ChannelListener) otherwise accumulates dead listeners unboundedly —
    /// every future append then broadcasts into channels no one reads.
    #[tokio::test]
    async fn dropped_subscription_is_reaped_on_next_append() {
        let session = ListenableInMemorySession::new();
        {
            let _stream = session.subscribe_events(0).await;
            assert_eq!(
                session.listeners.read().await.len(),
                1,
                "subscribe_events registers one listener"
            );
        } // stream dropped → receiver dropped → the ChannelListener is dead

        session
            .append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;

        assert_eq!(
            session.listeners.read().await.len(),
            0,
            "the dead subscription's listener must be reaped, not leaked"
        );
    }

    /// A still-live explicit listener (the common case: ForwardToSession on
    /// a child) must NOT be reaped by the same sweep — only genuinely dead
    /// ones go.
    #[tokio::test]
    async fn live_explicit_listener_survives_the_reap() {
        struct AlwaysLive;
        #[async_trait]
        impl SessionEventListener for AlwaysLive {
            async fn on_event(&self, _e: &SessionEvent) {}
        }

        let session = ListenableInMemorySession::new();
        session.subscribe(Arc::new(AlwaysLive)).await;
        session
            .append(SessionEvent::progress(serde_json::json!({"n": 1})))
            .await;
        assert_eq!(
            session.listeners.read().await.len(),
            1,
            "a live listener must survive the dead-listener sweep"
        );
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
        child
            .subscribe(Arc::new(ForwardToSession {
                target: parent.clone() as Arc<dyn AgentSession>,
            }))
            .await;

        // Append to child — should appear in parent via the listener.
        child
            .append(SessionEvent::progress(serde_json::json!({"from": "child"})))
            .await;

        let parent_events = parent.inner.events.read().await;
        assert_eq!(
            parent_events.len(),
            1,
            "parent should have received the child event"
        );
        assert_eq!(parent_events[0].event_type, "progress");
    }
}
