// INPUT:  tokio::sync::{mpsc, RwLock}, std::collections::HashMap, std::sync::Arc, serde
// OUTPUT: AgentMessage (swarm), MessageType, MailboxSystem
// POS:    Channel-based inter-agent messaging for swarm coordination.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};

/// A message exchanged between agents in a swarm.
///
/// Not to be confused with `alva_types::AgentMessage` which represents
/// LLM conversation messages. This is a control-plane message between
/// agent processes/tasks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwarmMessage {
    /// Sender agent ID or name.
    pub from: String,
    /// Recipient agent ID or name.
    pub to: String,
    /// Message content (text).
    pub content: String,
    /// Optional short summary (for leader dashboards).
    pub summary: Option<String>,
    /// Unix timestamp (seconds).
    pub timestamp: u64,
    /// What kind of message this is.
    pub message_type: SwarmMessageType,
}

/// The kind of swarm control message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SwarmMessageType {
    /// Regular text message between agents.
    Text,
    /// Request an agent to shut down gracefully.
    ShutdownRequest,
    /// Acknowledge a shutdown request.
    ShutdownResponse,
    /// Request plan approval from the leader.
    PlanApprovalRequest,
    /// Leader's response to a plan approval request.
    PlanApprovalResponse,
}

/// Mailbox system for agent-to-agent communication within a swarm.
///
/// Each registered agent gets a dedicated `mpsc` channel. Messages can be
/// addressed by agent ID or human-readable name (resolved via an internal
/// registry).
pub struct MailboxSystem {
    /// Mailboxes indexed by agent ID (and optionally name).
    mailboxes: Arc<RwLock<HashMap<String, mpsc::Sender<SwarmMessage>>>>,
    /// Agent name -> agent ID mapping for name-based addressing.
    name_registry: Arc<RwLock<HashMap<String, String>>>,
}

impl MailboxSystem {
    pub fn new() -> Self {
        Self {
            mailboxes: Arc::new(RwLock::new(HashMap::new())),
            name_registry: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register an agent and return a receiver for its mailbox.
    ///
    /// If `name` is provided, the agent can also be addressed by name.
    pub async fn register(
        &self,
        agent_id: &str,
        name: Option<&str>,
    ) -> mpsc::Receiver<SwarmMessage> {
        let (tx, rx) = mpsc::channel(100);

        self.mailboxes
            .write()
            .await
            .insert(agent_id.to_string(), tx.clone());

        if let Some(name) = name {
            self.name_registry
                .write()
                .await
                .insert(name.to_string(), agent_id.to_string());
            // Also register by name directly for convenience
            self.mailboxes
                .write()
                .await
                .insert(name.to_string(), tx);
        }

        rx
    }

    /// Unregister an agent, removing its mailbox and name mapping.
    pub async fn unregister(&self, agent_id: &str) {
        self.mailboxes.write().await.remove(agent_id);

        // Also remove from name registry
        let mut registry = self.name_registry.write().await;
        registry.retain(|_, v| v != agent_id);
    }

    /// Send a message to a specific agent (by ID or name).
    pub async fn send(&self, message: SwarmMessage) -> Result<(), String> {
        let mailboxes = self.mailboxes.read().await;

        // Try direct lookup first (by ID or name)
        if let Some(tx) = mailboxes.get(&message.to) {
            tx.send(message)
                .await
                .map_err(|e| format!("Failed to send message: {}", e))?;
            return Ok(());
        }

        // Try name resolution
        let registry = self.name_registry.read().await;
        if let Some(agent_id) = registry.get(&message.to) {
            if let Some(tx) = mailboxes.get(agent_id) {
                tx.send(message)
                    .await
                    .map_err(|e| format!("Failed to send message: {}", e))?;
                return Ok(());
            }
        }

        Err(format!("Agent not found: {}", message.to))
    }

    /// Broadcast a message to all registered agents except the sender.
    pub async fn broadcast(&self, from: &str, content: &str) {
        let mailboxes = self.mailboxes.read().await;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        for (to, tx) in mailboxes.iter() {
            if to != from {
                let msg = SwarmMessage {
                    from: from.to_string(),
                    to: to.to_string(),
                    content: content.to_string(),
                    summary: None,
                    timestamp: now,
                    message_type: SwarmMessageType::Text,
                };
                let _ = tx.send(msg).await;
            }
        }
    }

    /// List all registered agent IDs/names.
    pub async fn list_agents(&self) -> Vec<String> {
        self.mailboxes.read().await.keys().cloned().collect()
    }
}

impl Default for MailboxSystem {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn register_and_send() {
        let system = MailboxSystem::new();
        let mut rx = system.register("agent-1", Some("alice")).await;

        let msg = SwarmMessage {
            from: "agent-2".to_string(),
            to: "agent-1".to_string(),
            content: "hello".to_string(),
            summary: None,
            timestamp: 0,
            message_type: SwarmMessageType::Text,
        };
        system.send(msg).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.content, "hello");
        assert_eq!(received.from, "agent-2");
    }

    #[tokio::test]
    async fn send_by_name() {
        let system = MailboxSystem::new();
        let mut rx = system.register("agent-1", Some("alice")).await;

        let msg = SwarmMessage {
            from: "agent-2".to_string(),
            to: "alice".to_string(),
            content: "hi alice".to_string(),
            summary: None,
            timestamp: 0,
            message_type: SwarmMessageType::Text,
        };
        system.send(msg).await.unwrap();

        let received = rx.recv().await.unwrap();
        assert_eq!(received.content, "hi alice");
    }

    #[tokio::test]
    async fn send_to_unknown_agent_fails() {
        let system = MailboxSystem::new();
        let msg = SwarmMessage {
            from: "agent-1".to_string(),
            to: "nonexistent".to_string(),
            content: "hello?".to_string(),
            summary: None,
            timestamp: 0,
            message_type: SwarmMessageType::Text,
        };
        let result = system.send(msg).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Agent not found"));
    }

    #[tokio::test]
    async fn unregister_removes_agent() {
        let system = MailboxSystem::new();
        let _rx = system.register("agent-1", Some("alice")).await;

        assert!(system.list_agents().await.contains(&"agent-1".to_string()));

        system.unregister("agent-1").await;

        // ID removed
        assert!(!system.list_agents().await.contains(&"agent-1".to_string()));
    }

    #[tokio::test]
    async fn broadcast_excludes_sender() {
        let system = MailboxSystem::new();
        let _rx1 = system.register("agent-1", None).await;
        let mut rx2 = system.register("agent-2", None).await;
        let mut rx3 = system.register("agent-3", None).await;

        system.broadcast("agent-1", "team update").await;

        // agent-2 and agent-3 should receive, agent-1 should not
        let msg2 = rx2.recv().await.unwrap();
        assert_eq!(msg2.content, "team update");
        assert_eq!(msg2.from, "agent-1");

        let msg3 = rx3.recv().await.unwrap();
        assert_eq!(msg3.content, "team update");
    }

    #[tokio::test]
    async fn list_agents_returns_all_keys() {
        let system = MailboxSystem::new();
        let _rx1 = system.register("agent-1", Some("alice")).await;
        let _rx2 = system.register("agent-2", None).await;

        let agents = system.list_agents().await;
        // Should contain agent-1, alice (name alias), and agent-2
        assert!(agents.contains(&"agent-1".to_string()));
        assert!(agents.contains(&"alice".to_string()));
        assert!(agents.contains(&"agent-2".to_string()));
    }
}
