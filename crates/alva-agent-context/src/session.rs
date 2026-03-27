// INPUT:  serde, serde_json, async_trait, tokio::sync::Mutex, std::collections::HashMap, uuid, chrono
// OUTPUT: pub struct SessionEvent, pub struct SessionMessage, pub struct EventQuery, pub struct EventMatch, pub trait SessionAccess, pub struct InMemorySession
// POS:    Event-based session storage — append-only event log with query/rollback/snapshot, modeled after Claude Code's JSONL format.
//! Session storage — append-only event log.
//!
//! Session records everything that happens: user messages, assistant responses,
//! tool calls, progress events, system events. Unlike ContextStore (which manages
//! the runtime LLM window), Session is the permanent record of "what happened".
//!
//! Design: each event is a self-contained JSON-serializable record. Storage
//! backends (SQLite, files, remote) implement `SessionAccess`. The in-memory
//! implementation is provided for testing.

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------------------
// Event types
// ---------------------------------------------------------------------------

/// A single event in the session log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Unique identifier for this event.
    pub uuid: String,
    /// Parent event (e.g., tool_result points to tool_use).
    pub parent_uuid: Option<String>,
    /// Event type: "user", "assistant", "system", "progress", etc.
    #[serde(rename = "type")]
    pub event_type: String,
    /// Timestamp (epoch millis).
    pub timestamp: i64,
    /// Conversation message (present for user/assistant events).
    pub message: Option<SessionMessage>,
    /// Arbitrary payload (present for progress/system events).
    pub data: Option<serde_json::Value>,
}

/// A conversation message within a session event.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// "user", "assistant", "tool"
    pub role: String,
    /// Message content — string or content blocks array.
    pub content: serde_json::Value,
}

impl SessionEvent {
    /// Create a user message event.
    pub fn user_message(content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "user".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "user".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create an assistant message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "assistant".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "assistant".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create a tool result event linked to a parent tool_use.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: Some(parent_tool_use_uuid.to_string()),
            event_type: "tool_result".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: Some(SessionMessage {
                role: "tool".to_string(),
                content,
            }),
            data: None,
        }
    }

    /// Create a progress event (tool execution status, hook triggers, etc.)
    pub fn progress(data: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "progress".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: None,
            data: Some(data),
        }
    }

    /// Create a system event.
    pub fn system(data: serde_json::Value) -> Self {
        Self {
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            event_type: "system".to_string(),
            timestamp: chrono::Utc::now().timestamp_millis(),
            message: None,
            data: Some(data),
        }
    }
}

// ---------------------------------------------------------------------------
// Query
// ---------------------------------------------------------------------------

/// Filter criteria for querying session events.
/// All fields are optional — None means "don't filter on this".
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    /// Filter by event type ("user", "assistant", "progress", etc.)
    pub event_type: Option<String>,
    /// Filter by message role ("user", "assistant", "tool")
    pub role: Option<String>,
    /// Text search in message content
    pub text_contains: Option<String>,
    /// Only events after this uuid (cursor-based pagination)
    pub after_uuid: Option<String>,
    /// Only the last N matching events
    pub last_n: Option<usize>,
    /// Maximum results to return
    pub limit: usize,
}

/// A query result with preview text.
#[derive(Debug, Clone)]
pub struct EventMatch {
    pub event: SessionEvent,
    /// Truncated preview of the content (for display).
    pub preview: String,
}

// ---------------------------------------------------------------------------
// SessionAccess trait
// ---------------------------------------------------------------------------

/// The session storage interface.
///
/// Append-only event log with query and rollback support.
/// Implementations: InMemorySession (testing), SQLite (desktop), file (CLI), remote (cloud).
#[async_trait]
pub trait SessionAccess: Send + Sync {
    /// Session identifier.
    fn session_id(&self) -> &str;

    /// Append an event to the log.
    async fn append(&self, event: SessionEvent);

    /// Query events matching the filter. Storage layer does the filtering.
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    /// Count events matching the filter (without loading content).
    async fn count(&self, filter: &EventQuery) -> usize;

    /// Rollback: delete all events after the given uuid.
    /// Returns the number of events removed.
    async fn rollback_after(&self, uuid: &str) -> usize;

    /// Save a context snapshot (binary, opaque to storage).
    async fn save_snapshot(&self, data: &[u8]);

    /// Load the last saved context snapshot.
    async fn load_snapshot(&self) -> Option<Vec<u8>>;

    /// Clear all events and snapshots.
    async fn clear(&self);
}

// ---------------------------------------------------------------------------
// InMemorySession — for testing
// ---------------------------------------------------------------------------

/// In-memory session storage. All data in a Vec behind a Mutex.
pub struct InMemorySession {
    session_id: String,
    events: tokio::sync::Mutex<Vec<SessionEvent>>,
    snapshot: tokio::sync::Mutex<Option<Vec<u8>>>,
}

impl InMemorySession {
    pub fn new(session_id: impl Into<String>) -> Self {
        Self {
            session_id: session_id.into(),
            events: tokio::sync::Mutex::new(Vec::new()),
            snapshot: tokio::sync::Mutex::new(None),
        }
    }
}

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
        format!("{}...", &text[..160])
    } else {
        text
    }
}

#[async_trait]
impl SessionAccess for InMemorySession {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn append(&self, event: SessionEvent) {
        self.events.lock().await.push(event);
    }

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        let events = self.events.lock().await;

        // Find start position (after_uuid cursor)
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

        // last_n: keep only the last N
        if let Some(n) = filter.last_n {
            let skip = matches.len().saturating_sub(n);
            matches = matches.into_iter().skip(skip).collect();
        }

        // limit
        if filter.limit > 0 {
            matches.truncate(filter.limit);
        }

        matches
    }

    async fn count(&self, filter: &EventQuery) -> usize {
        let events = self.events.lock().await;
        events.iter().filter(|e| event_matches(e, filter)).count()
    }

    async fn rollback_after(&self, uuid: &str) -> usize {
        let mut events = self.events.lock().await;
        if let Some(pos) = events.iter().position(|e| e.uuid == *uuid) {
            let removed = events.len() - pos - 1;
            events.truncate(pos + 1);
            removed
        } else {
            0
        }
    }

    async fn save_snapshot(&self, data: &[u8]) {
        *self.snapshot.lock().await = Some(data.to_vec());
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.snapshot.lock().await.clone()
    }

    async fn clear(&self) {
        self.events.lock().await.clear();
        *self.snapshot.lock().await = None;
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_append_and_count() {
        let session = InMemorySession::new("s1");
        assert_eq!(
            session.count(&EventQuery { limit: 100, ..Default::default() }).await,
            0
        );

        session.append(SessionEvent::user_message(serde_json::json!("hello"))).await;
        session.append(SessionEvent::assistant_message(serde_json::json!("hi"))).await;

        assert_eq!(
            session.count(&EventQuery { limit: 100, ..Default::default() }).await,
            2
        );
    }

    #[tokio::test]
    async fn test_query_by_role() {
        let session = InMemorySession::new("s1");
        session.append(SessionEvent::user_message(serde_json::json!("q1"))).await;
        session.append(SessionEvent::assistant_message(serde_json::json!("a1"))).await;
        session.append(SessionEvent::user_message(serde_json::json!("q2"))).await;
        session.append(SessionEvent::progress(serde_json::json!({"tool": "read"}))).await;

        let users = session.query(&EventQuery {
            role: Some("user".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(users.len(), 2);

        let assistants = session.query(&EventQuery {
            role: Some("assistant".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(assistants.len(), 1);
    }

    #[tokio::test]
    async fn test_query_by_type() {
        let session = InMemorySession::new("s1");
        session.append(SessionEvent::user_message(serde_json::json!("hi"))).await;
        session.append(SessionEvent::progress(serde_json::json!({"status": "running"}))).await;
        session.append(SessionEvent::progress(serde_json::json!({"status": "done"}))).await;

        let progress = session.query(&EventQuery {
            event_type: Some("progress".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(progress.len(), 2);
    }

    #[tokio::test]
    async fn test_query_text_search() {
        let session = InMemorySession::new("s1");
        session.append(SessionEvent::user_message(serde_json::json!("help me fix the bug"))).await;
        session.append(SessionEvent::user_message(serde_json::json!("show me the logs"))).await;
        session.append(SessionEvent::assistant_message(serde_json::json!("looking at the bug now"))).await;

        let results = session.query(&EventQuery {
            text_contains: Some("bug".into()),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(results.len(), 2); // user "fix the bug" + assistant "the bug now"
    }

    #[tokio::test]
    async fn test_query_last_n() {
        let session = InMemorySession::new("s1");
        for i in 0..10 {
            session.append(SessionEvent::user_message(serde_json::json!(format!("msg-{}", i)))).await;
        }

        let last3 = session.query(&EventQuery {
            last_n: Some(3),
            limit: 100,
            ..Default::default()
        }).await;
        assert_eq!(last3.len(), 3);
        assert!(last3[0].preview.contains("msg-7"));
        assert!(last3[2].preview.contains("msg-9"));
    }

    #[tokio::test]
    async fn test_rollback() {
        let session = InMemorySession::new("s1");
        let mut uuids = Vec::new();
        for i in 0..5 {
            let event = SessionEvent::user_message(serde_json::json!(format!("msg-{}", i)));
            uuids.push(event.uuid.clone());
            session.append(event).await;
        }

        // Rollback after event 2 — events 3 and 4 should be removed.
        let removed = session.rollback_after(&uuids[2]).await;
        assert_eq!(removed, 2);
        assert_eq!(
            session.count(&EventQuery { limit: 100, ..Default::default() }).await,
            3
        );
    }

    #[tokio::test]
    async fn test_snapshot() {
        let session = InMemorySession::new("s1");

        assert!(session.load_snapshot().await.is_none());

        let data = b"context-state-bytes";
        session.save_snapshot(data).await;

        let loaded = session.load_snapshot().await.unwrap();
        assert_eq!(loaded, data);
    }

    #[tokio::test]
    async fn test_clear() {
        let session = InMemorySession::new("s1");
        session.append(SessionEvent::user_message(serde_json::json!("hi"))).await;
        session.save_snapshot(b"snap").await;

        session.clear().await;
        assert_eq!(
            session.count(&EventQuery { limit: 100, ..Default::default() }).await,
            0
        );
        assert!(session.load_snapshot().await.is_none());
    }
}
