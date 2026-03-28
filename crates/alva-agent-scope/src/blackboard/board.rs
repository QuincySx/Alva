// INPUT:  tokio::sync::{RwLock, broadcast}, std::sync::Arc
// OUTPUT: Blackboard
// POS:    The shared data structure — agent registry + append-only message stream + notification.

use std::sync::Arc;

use tokio::sync::{broadcast, RwLock};

use super::message::BoardMessage;
use super::profile::AgentProfile;

/// Shared blackboard — the "chat room" all agents read/write to.
///
/// Thread-safe, lock-free reads for the common path (message count check),
/// RwLock for actual reads/writes. Broadcast channel for real-time
/// notifications so agents don't need to poll.
pub struct Blackboard {
    inner: Arc<RwLock<BlackboardInner>>,
    notify_tx: broadcast::Sender<BoardMessage>,
}

struct BlackboardInner {
    profiles: Vec<AgentProfile>,
    messages: Vec<BoardMessage>,
}

impl Blackboard {
    pub fn new() -> Self {
        let (notify_tx, _) = broadcast::channel(256);
        Self {
            inner: Arc::new(RwLock::new(BlackboardInner {
                profiles: Vec::new(),
                messages: Vec::new(),
            })),
            notify_tx,
        }
    }

    // ── Registry ────────────────────────────────────────────────────────

    /// Register an agent in the room. Idempotent — re-registering the
    /// same id updates the profile.
    pub async fn register(&self, profile: AgentProfile) {
        let mut inner = self.inner.write().await;
        if let Some(existing) = inner.profiles.iter_mut().find(|p| p.id == profile.id) {
            *existing = profile;
        } else {
            inner.profiles.push(profile);
        }
    }

    /// Get all registered profiles.
    pub async fn profiles(&self) -> Vec<AgentProfile> {
        self.inner.read().await.profiles.clone()
    }

    /// Get a specific agent's profile.
    pub async fn profile(&self, agent_id: &str) -> Option<AgentProfile> {
        self.inner
            .read()
            .await
            .profiles
            .iter()
            .find(|p| p.id == agent_id)
            .cloned()
    }

    /// Number of registered agents.
    pub async fn agent_count(&self) -> usize {
        self.inner.read().await.profiles.len()
    }

    // ── Messages ────────────────────────────────────────────────────────

    /// Post a message to the board. All subscribers are notified.
    pub async fn post(&self, msg: BoardMessage) {
        // Best-effort notify — if no receivers, that's fine.
        let _ = self.notify_tx.send(msg.clone());
        self.inner.write().await.messages.push(msg);
    }

    /// Total message count.
    pub async fn message_count(&self) -> usize {
        self.inner.read().await.messages.len()
    }

    /// Get all messages (full history).
    pub async fn all_messages(&self) -> Vec<BoardMessage> {
        self.inner.read().await.messages.clone()
    }

    /// Get messages since a given index (for incremental reads).
    pub async fn messages_since(&self, index: usize) -> Vec<BoardMessage> {
        let inner = self.inner.read().await;
        if index >= inner.messages.len() {
            Vec::new()
        } else {
            inner.messages[index..].to_vec()
        }
    }

    /// Get messages relevant to a specific agent:
    /// - Broadcasts (no mentions)
    /// - Messages that @mention this agent
    /// - Messages from agents this agent depends on
    /// - Introduction messages (always relevant)
    pub async fn messages_for(&self, agent_id: &str) -> Vec<BoardMessage> {
        let inner = self.inner.read().await;
        let profile = inner.profiles.iter().find(|p| p.id == agent_id);
        let deps: Vec<&str> = profile
            .map(|p| p.depends_on.iter().map(|s| s.as_str()).collect())
            .unwrap_or_default();

        inner
            .messages
            .iter()
            .filter(|m| {
                m.is_broadcast()
                    || m.mentions_agent(agent_id)
                    || deps.contains(&m.from.as_str())
                    || matches!(m.kind, super::message::MessageKind::Introduction)
            })
            .cloned()
            .collect()
    }

    /// Render recent messages as a chat log string for LLM context injection.
    ///
    /// `max_messages` limits how many recent messages to include.
    /// Returns (rendered_text, total_message_count).
    pub async fn render_chat_log(&self, max_messages: usize) -> (String, usize) {
        let inner = self.inner.read().await;
        let total = inner.messages.len();
        let start = total.saturating_sub(max_messages);
        let recent = &inner.messages[start..];

        let text = if start > 0 {
            let mut lines = vec![format!("... ({} earlier messages omitted)\n", start)];
            lines.extend(recent.iter().map(|m| m.to_chat_line()));
            lines.join("\n")
        } else {
            recent
                .iter()
                .map(|m| m.to_chat_line())
                .collect::<Vec<_>>()
                .join("\n")
        };

        (text, total)
    }

    /// Render messages relevant to a specific agent as a chat log.
    pub async fn render_chat_log_for(
        &self,
        agent_id: &str,
        max_messages: usize,
    ) -> (String, usize) {
        let relevant = self.messages_for(agent_id).await;
        let total = relevant.len();
        let start = total.saturating_sub(max_messages);
        let recent = &relevant[start..];

        let text = if start > 0 {
            let mut lines = vec![format!("... ({} earlier messages omitted)\n", start)];
            lines.extend(recent.iter().map(|m| m.to_chat_line()));
            lines.join("\n")
        } else {
            recent
                .iter()
                .map(|m| m.to_chat_line())
                .collect::<Vec<_>>()
                .join("\n")
        };

        (text, total)
    }

    // ── Subscriptions ───────────────────────────────────────────────────

    /// Subscribe to real-time message notifications.
    pub fn subscribe(&self) -> broadcast::Receiver<BoardMessage> {
        self.notify_tx.subscribe()
    }

    // ── Serialization ───────────────────────────────────────────────────

    /// Snapshot the entire board as JSON (for persistence / debugging).
    pub async fn to_json(&self) -> serde_json::Value {
        let inner = self.inner.read().await;
        serde_json::json!({
            "profiles": inner.profiles,
            "messages": inner.messages,
        })
    }
}

impl Default for Blackboard {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::message::MessageKind;

    fn planner() -> AgentProfile {
        AgentProfile::new("planner", "requirements analysis")
            .provides_to(["generator"])
    }

    fn generator() -> AgentProfile {
        AgentProfile::new("generator", "code implementation")
            .depends_on(["planner"])
            .provides_to(["evaluator"])
    }

    fn evaluator() -> AgentProfile {
        AgentProfile::new("evaluator", "quality review")
            .depends_on(["generator"])
    }

    #[tokio::test]
    async fn register_and_list_profiles() {
        let board = Blackboard::new();
        board.register(planner()).await;
        board.register(generator()).await;

        assert_eq!(board.agent_count().await, 2);

        let p = board.profile("planner").await;
        assert!(p.is_some());
        assert_eq!(p.unwrap().role, "requirements analysis");
    }

    #[tokio::test]
    async fn re_register_updates_profile() {
        let board = Blackboard::new();
        board.register(AgentProfile::new("gen", "v1")).await;
        board.register(AgentProfile::new("gen", "v2")).await;

        assert_eq!(board.agent_count().await, 1);
        assert_eq!(board.profile("gen").await.unwrap().role, "v2");
    }

    #[tokio::test]
    async fn post_and_read_messages() {
        let board = Blackboard::new();

        board.post(BoardMessage::new("planner", "spec done")).await;
        board
            .post(
                BoardMessage::new("planner", "start coding please")
                    .with_mention("generator"),
            )
            .await;

        assert_eq!(board.message_count().await, 2);

        let all = board.all_messages().await;
        assert_eq!(all[0].content, "spec done");
        assert!(all[1].mentions_agent("generator"));
    }

    #[tokio::test]
    async fn messages_since_returns_incremental() {
        let board = Blackboard::new();

        board.post(BoardMessage::new("a", "msg 0")).await;
        board.post(BoardMessage::new("a", "msg 1")).await;
        board.post(BoardMessage::new("a", "msg 2")).await;

        let since_1 = board.messages_since(1).await;
        assert_eq!(since_1.len(), 2);
        assert_eq!(since_1[0].content, "msg 1");
    }

    #[tokio::test]
    async fn messages_for_filters_correctly() {
        let board = Blackboard::new();
        board.register(planner()).await;
        board.register(generator()).await;
        board.register(evaluator()).await;

        // Introduction (visible to all)
        board
            .post(BoardMessage::new("planner", "Hi I'm planner").with_kind(MessageKind::Introduction))
            .await;

        // Broadcast (visible to all)
        board
            .post(BoardMessage::new("system", "Budget at 80%"))
            .await;

        // Directed at generator (visible to generator)
        board
            .post(
                BoardMessage::new("planner", "Start coding")
                    .with_mention("generator"),
            )
            .await;

        // From planner (generator depends_on planner, so visible)
        board
            .post(BoardMessage::new("planner", "Updated spec"))
            .await;

        // Evaluator talking to itself (NOT visible to generator)
        board
            .post(
                BoardMessage::new("evaluator", "Reviewing my criteria")
                    .with_mention("evaluator"),
            )
            .await;

        let gen_msgs = board.messages_for("generator").await;

        // Should see: intro, broadcast, directed @gen, from planner (dep)
        // Should NOT see: evaluator self-mention
        assert_eq!(gen_msgs.len(), 4);
        assert!(gen_msgs.iter().all(|m| m.from != "evaluator"));
    }

    #[tokio::test]
    async fn render_chat_log_limits_messages() {
        let board = Blackboard::new();
        for i in 0..10 {
            board
                .post(BoardMessage::new("agent", format!("msg {}", i)))
                .await;
        }

        let (log, total) = board.render_chat_log(3).await;
        assert_eq!(total, 10);
        assert!(log.contains("7 earlier messages omitted"));
        assert!(log.contains("msg 7"));
        assert!(log.contains("msg 9"));
        assert!(!log.contains("msg 6"));
    }

    #[tokio::test]
    async fn broadcast_subscriber_receives_messages() {
        let board = Blackboard::new();
        let mut rx = board.subscribe();

        board.post(BoardMessage::new("planner", "hello")).await;

        let received = rx.recv().await.unwrap();
        assert_eq!(received.from, "planner");
        assert_eq!(received.content, "hello");
    }

    #[tokio::test]
    async fn to_json_snapshot() {
        let board = Blackboard::new();
        board.register(planner()).await;
        board.post(BoardMessage::new("planner", "hi")).await;

        let json = board.to_json().await;
        assert!(json["profiles"].is_array());
        assert!(json["messages"].is_array());
        assert_eq!(json["profiles"].as_array().unwrap().len(), 1);
    }
}
