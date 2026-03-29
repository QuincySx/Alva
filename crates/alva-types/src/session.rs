// INPUT:  async_trait, uuid, std::sync::RwLock, crate::base::message::AgentMessage
// OUTPUT: pub trait AgentSession, pub struct InMemorySession
// POS:    Session abstraction — single source of truth for agent message history.
use std::sync::RwLock;

use async_trait::async_trait;

use crate::base::message::AgentMessage;

/// AgentSession manages all messages for an Agent's conversation.
/// It is the single source of truth for message history.
#[async_trait]
pub trait AgentSession: Send + Sync {
    /// Unique session identifier.
    fn id(&self) -> &str;
    /// Parent session ID (for sub-agents). None for root agents.
    fn parent_id(&self) -> Option<&str>;
    /// Append a message to the session.
    fn append(&self, message: AgentMessage);
    /// Get all messages (for UI rendering, export).
    fn messages(&self) -> Vec<AgentMessage>;
    /// Get the most recent N messages (for context assembly).
    fn recent(&self, n: usize) -> Vec<AgentMessage>;
    /// Persist to storage backend.
    async fn flush(&self);
    /// Restore from storage backend.
    async fn restore(&self) -> Vec<AgentMessage>;
}

/// In-memory session — default implementation, no persistence.
pub struct InMemorySession {
    id: String,
    parent_id: Option<String>,
    messages: RwLock<Vec<AgentMessage>>,
}

impl InMemorySession {
    /// Create a new root session with a random UUID v4 id.
    pub fn new() -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: None,
            messages: RwLock::new(Vec::new()),
        }
    }

    /// Create a child session linked to a parent session.
    pub fn with_parent(parent_id: &str) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            parent_id: Some(parent_id.to_owned()),
            messages: RwLock::new(Vec::new()),
        }
    }
}

impl Default for InMemorySession {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl AgentSession for InMemorySession {
    fn id(&self) -> &str {
        &self.id
    }

    fn parent_id(&self) -> Option<&str> {
        self.parent_id.as_deref()
    }

    fn append(&self, message: AgentMessage) {
        self.messages
            .write()
            .expect("session RwLock poisoned")
            .push(message);
    }

    fn messages(&self) -> Vec<AgentMessage> {
        self.messages
            .read()
            .expect("session RwLock poisoned")
            .clone()
    }

    fn recent(&self, n: usize) -> Vec<AgentMessage> {
        let guard = self.messages.read().expect("session RwLock poisoned");
        let len = guard.len();
        if n >= len {
            guard.clone()
        } else {
            guard[len - n..].to_vec()
        }
    }

    async fn flush(&self) {
        // No-op for in-memory session — nothing to persist.
    }

    async fn restore(&self) -> Vec<AgentMessage> {
        // For in-memory session, restore simply returns current messages.
        self.messages()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::base::message::Message;

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message::user(text))
    }

    #[test]
    fn new_session_has_id() {
        let s = InMemorySession::new();
        assert!(!s.id().is_empty());
    }

    #[test]
    fn no_parent_by_default() {
        let s = InMemorySession::new();
        assert!(s.parent_id().is_none());
    }

    #[test]
    fn child_has_parent() {
        let parent = InMemorySession::new();
        let child = InMemorySession::with_parent(parent.id());
        assert_eq!(child.parent_id(), Some(parent.id()));
    }

    #[test]
    fn append_and_retrieve() {
        let s = InMemorySession::new();
        s.append(user_msg("hello"));
        s.append(user_msg("world"));
        assert_eq!(s.messages().len(), 2);
    }

    #[test]
    fn recent_returns_last_n() {
        let s = InMemorySession::new();
        for i in 0..10 {
            s.append(user_msg(&format!("msg {}", i)));
        }
        let recent = s.recent(3);
        assert_eq!(recent.len(), 3);
        if let AgentMessage::Standard(m) = &recent[0] {
            assert!(m.text_content().contains("msg 7"));
        }
    }

    #[test]
    fn recent_larger_than_total() {
        let s = InMemorySession::new();
        s.append(user_msg("one"));
        assert_eq!(s.recent(100).len(), 1);
    }

    #[test]
    fn empty_session() {
        let s = InMemorySession::new();
        assert!(s.messages().is_empty());
        assert!(s.recent(5).is_empty());
    }

    #[tokio::test]
    async fn flush_and_restore() {
        let s = InMemorySession::new();
        s.append(user_msg("saved"));
        s.flush().await;
        let restored = s.restore().await;
        assert_eq!(restored.len(), 1);
    }

    #[test]
    fn unique_ids() {
        let a = InMemorySession::new();
        let b = InMemorySession::new();
        assert_ne!(a.id(), b.id());
    }
}
