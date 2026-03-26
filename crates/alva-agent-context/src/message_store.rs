//! MessageStore trait — the storage interface that ContextPlugin depends on.
//!
//! This is the source of truth for conversation history. The agent loop writes to it,
//! the context plugin reads from it. Both go through this interface.
//!
//! Messages are organized by session, ordered chronologically.

use alva_types::AgentMessage;
use async_trait::async_trait;

/// A conversation turn = one user message + the agent's full response
/// (which may include multiple tool calls, thinking, etc.)
#[derive(Debug, Clone)]
pub struct Turn {
    /// Turn index (0-based, auto-incremented).
    pub index: usize,
    /// The user's message that started this turn.
    pub user_message: AgentMessage,
    /// All messages produced during the agent loop for this turn
    /// (assistant responses, tool calls, tool results, thinking, etc.)
    pub agent_messages: Vec<AgentMessage>,
    /// Timestamp when the turn started.
    pub started_at: i64,
    /// Timestamp when the turn completed (None if still running).
    pub completed_at: Option<i64>,
}

impl Turn {
    /// All messages in this turn (user + agent), in chronological order.
    pub fn all_messages(&self) -> Vec<&AgentMessage> {
        let mut msgs = vec![&self.user_message];
        msgs.extend(self.agent_messages.iter());
        msgs
    }

    /// Estimate total tokens for this turn.
    pub fn estimated_tokens(&self) -> usize {
        self.all_messages()
            .iter()
            .map(|m| crate::store::estimate_tokens(&Self::message_text(m)))
            .sum()
    }

    fn message_text(msg: &AgentMessage) -> String {
        match msg {
            AgentMessage::Standard(m) => m.text_content(),
            AgentMessage::Custom { data, .. } => data.to_string(),
        }
    }
}

/// The conversation history storage interface.
///
/// Implementations can be in-memory, SQLite, file-backed, etc.
/// The context plugin reads from this; the agent loop writes to this.
#[async_trait]
pub trait MessageStore: Send + Sync {
    /// Append a complete turn to the session history.
    async fn append_turn(&self, session_id: &str, turn: Turn);

    /// Get all turns for a session, in chronological order.
    async fn get_turns(&self, session_id: &str) -> Vec<Turn>;

    /// Get the last N turns.
    async fn get_recent_turns(&self, session_id: &str, count: usize) -> Vec<Turn>;

    /// Get total turn count.
    async fn turn_count(&self, session_id: &str) -> usize;

    /// Flatten all turns into a chronological message list.
    /// This is what gets fed to assemble().
    async fn get_all_messages(&self, session_id: &str) -> Vec<AgentMessage> {
        let turns = self.get_turns(session_id).await;
        turns
            .iter()
            .flat_map(|t| {
                let mut msgs = vec![t.user_message.clone()];
                msgs.extend(t.agent_messages.clone());
                msgs
            })
            .collect()
    }

    /// Get messages from the last N turns only.
    async fn get_recent_messages(&self, session_id: &str, turn_count: usize) -> Vec<AgentMessage> {
        let turns = self.get_recent_turns(session_id, turn_count).await;
        turns
            .iter()
            .flat_map(|t| {
                let mut msgs = vec![t.user_message.clone()];
                msgs.extend(t.agent_messages.clone());
                msgs
            })
            .collect()
    }

    /// Replace a specific turn (e.g., after compression rewrites its content).
    async fn replace_turn(&self, session_id: &str, turn_index: usize, turn: Turn);

    /// Remove turns older than the given index.
    async fn remove_turns_before(&self, session_id: &str, turn_index: usize);

    /// Clear all turns for a session.
    async fn clear(&self, session_id: &str);
}

/// In-memory implementation for development and testing.
pub struct InMemoryMessageStore {
    turns: tokio::sync::Mutex<std::collections::HashMap<String, Vec<Turn>>>,
}

impl InMemoryMessageStore {
    pub fn new() -> Self {
        Self {
            turns: tokio::sync::Mutex::new(std::collections::HashMap::new()),
        }
    }
}

impl Default for InMemoryMessageStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl MessageStore for InMemoryMessageStore {
    async fn append_turn(&self, session_id: &str, turn: Turn) {
        let mut store = self.turns.lock().await;
        store
            .entry(session_id.to_string())
            .or_default()
            .push(turn);
    }

    async fn get_turns(&self, session_id: &str) -> Vec<Turn> {
        let store = self.turns.lock().await;
        store.get(session_id).cloned().unwrap_or_default()
    }

    async fn get_recent_turns(&self, session_id: &str, count: usize) -> Vec<Turn> {
        let store = self.turns.lock().await;
        match store.get(session_id) {
            Some(turns) => {
                let skip = turns.len().saturating_sub(count);
                turns[skip..].to_vec()
            }
            None => vec![],
        }
    }

    async fn turn_count(&self, session_id: &str) -> usize {
        let store = self.turns.lock().await;
        store.get(session_id).map(|t| t.len()).unwrap_or(0)
    }

    async fn replace_turn(&self, session_id: &str, turn_index: usize, turn: Turn) {
        let mut store = self.turns.lock().await;
        if let Some(turns) = store.get_mut(session_id) {
            if turn_index < turns.len() {
                turns[turn_index] = turn;
            }
        }
    }

    async fn remove_turns_before(&self, session_id: &str, turn_index: usize) {
        let mut store = self.turns.lock().await;
        if let Some(turns) = store.get_mut(session_id) {
            if turn_index < turns.len() {
                turns.drain(..turn_index);
                // Re-index remaining turns
                for (i, turn) in turns.iter_mut().enumerate() {
                    turn.index = i;
                }
            }
        }
    }

    async fn clear(&self, session_id: &str) {
        let mut store = self.turns.lock().await;
        store.remove(session_id);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{ContentBlock, Message, MessageRole};

    /// Create a simple user AgentMessage with the given text.
    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: format!("msg-{}", text),
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 1000,
        })
    }

    /// Create an assistant AgentMessage with the given text.
    fn assistant_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: format!("msg-{}", text),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 2000,
        })
    }

    /// Build a Turn with one user message and one agent response.
    fn make_turn(index: usize, user_text: &str, agent_text: &str) -> Turn {
        Turn {
            index,
            user_message: user_msg(user_text),
            agent_messages: vec![assistant_msg(agent_text)],
            started_at: 1000,
            completed_at: Some(2000),
        }
    }

    #[tokio::test]
    async fn test_append_and_get_turns() {
        let store = InMemoryMessageStore::new();
        let sid = "session-1";

        store
            .append_turn(sid, make_turn(0, "hello", "hi there"))
            .await;
        store
            .append_turn(sid, make_turn(1, "how?", "fine"))
            .await;

        let turns = store.get_turns(sid).await;
        assert_eq!(turns.len(), 2);
        assert_eq!(turns[0].index, 0);
        assert_eq!(turns[1].index, 1);

        // Different session is empty.
        let other = store.get_turns("other-session").await;
        assert!(other.is_empty());
    }

    #[tokio::test]
    async fn test_get_recent_turns() {
        let store = InMemoryMessageStore::new();
        let sid = "s1";

        for i in 0..5 {
            store
                .append_turn(sid, make_turn(i, &format!("q{}", i), &format!("a{}", i)))
                .await;
        }

        let recent = store.get_recent_turns(sid, 2).await;
        assert_eq!(recent.len(), 2);
        assert_eq!(recent[0].index, 3);
        assert_eq!(recent[1].index, 4);

        // Requesting more than available returns all.
        let all = store.get_recent_turns(sid, 100).await;
        assert_eq!(all.len(), 5);
    }

    #[tokio::test]
    async fn test_get_all_messages() {
        let store = InMemoryMessageStore::new();
        let sid = "s1";

        store
            .append_turn(sid, make_turn(0, "q0", "a0"))
            .await;
        store
            .append_turn(sid, make_turn(1, "q1", "a1"))
            .await;

        let msgs = store.get_all_messages(sid).await;
        // 2 turns × (1 user + 1 assistant) = 4 messages.
        assert_eq!(msgs.len(), 4);

        // Verify order: user0, assistant0, user1, assistant1.
        let ids: Vec<String> = msgs
            .iter()
            .map(|m| match m {
                AgentMessage::Standard(msg) => msg.id.clone(),
                AgentMessage::Custom { .. } => "custom".to_string(),
            })
            .collect();
        assert_eq!(ids, vec!["msg-q0", "msg-a0", "msg-q1", "msg-a1"]);
    }

    #[tokio::test]
    async fn test_turn_count() {
        let store = InMemoryMessageStore::new();
        let sid = "s1";

        assert_eq!(store.turn_count(sid).await, 0);

        store
            .append_turn(sid, make_turn(0, "q0", "a0"))
            .await;
        assert_eq!(store.turn_count(sid).await, 1);

        store
            .append_turn(sid, make_turn(1, "q1", "a1"))
            .await;
        assert_eq!(store.turn_count(sid).await, 2);
    }

    #[tokio::test]
    async fn test_remove_turns_before() {
        let store = InMemoryMessageStore::new();
        let sid = "s1";

        for i in 0..5 {
            store
                .append_turn(sid, make_turn(i, &format!("q{}", i), &format!("a{}", i)))
                .await;
        }

        // Remove turns before index 3 (i.e., remove turns 0, 1, 2).
        store.remove_turns_before(sid, 3).await;

        let turns = store.get_turns(sid).await;
        assert_eq!(turns.len(), 2);
        // After removal, turns are re-indexed starting from 0.
        assert_eq!(turns[0].index, 0);
        assert_eq!(turns[1].index, 1);
    }

    #[tokio::test]
    async fn test_clear() {
        let store = InMemoryMessageStore::new();
        let sid = "s1";

        store
            .append_turn(sid, make_turn(0, "q0", "a0"))
            .await;
        assert_eq!(store.turn_count(sid).await, 1);

        store.clear(sid).await;
        assert_eq!(store.turn_count(sid).await, 0);
        assert!(store.get_turns(sid).await.is_empty());
    }
}
