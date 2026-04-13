// INPUT:  alva_kernel_abi::context (SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch), async_trait, tokio::sync::Mutex
// OUTPUT: re-exports SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch; provides InMemorySession
// POS:    Re-exports session traits/types from alva_kernel_abi::context and provides the InMemorySession implementation.
//! Session storage — trait re-exported from `alva_kernel_abi::context`, with InMemorySession implementation.

use async_trait::async_trait;

// Re-export traits and types from alva_kernel_abi::context
pub use alva_kernel_abi::context::{
    EventMatch, EventQuery, SessionAccess, SessionEvent, SessionMessage,
};

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

        // Rollback after event 2 -- events 3 and 4 should be removed.
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

    #[test]
    fn test_make_preview_multibyte_no_panic() {
        // 60 CJK chars * 3 bytes = 180 bytes, exceeds the 160-byte limit.
        let long_chinese = "你好世界".repeat(15); // 60 chars = 180 bytes
        let event = SessionEvent::user_message(serde_json::json!(long_chinese));
        let preview = make_preview(&event);
        // Must not panic and must end with "..."
        assert!(preview.ends_with("..."));
        // The truncated portion (before "...") must be valid UTF-8 and at most 160 bytes.
        let without_dots = &preview[..preview.len() - 3];
        assert!(without_dots.len() <= 160);
    }

    #[test]
    fn test_make_preview_short_multibyte() {
        let short = "你好世界";
        let event = SessionEvent::user_message(serde_json::json!(short));
        let preview = make_preview(&event);
        assert_eq!(preview, "你好世界");
    }

    #[test]
    fn test_safe_truncate_char_boundary() {
        // "你好" = 6 bytes. Truncating at 4 should yield "你" (3 bytes), not panic.
        assert_eq!(safe_truncate("你好", 4), "你");
        assert_eq!(safe_truncate("你好", 6), "你好");
        assert_eq!(safe_truncate("你好", 100), "你好");
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
