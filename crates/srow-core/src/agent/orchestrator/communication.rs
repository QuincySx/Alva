// INPUT:  serde, std::collections, tokio::sync, uuid, chrono
// OUTPUT: AgentMessage, MessageBus
// POS:    Inter-Agent message passing via per-agent mailboxes managed by a shared message bus.
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tokio::sync::RwLock;

/// A message exchanged between Agents through the Orchestrator
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentMessage {
    /// Unique message ID
    pub id: String,
    /// Sender Agent instance ID (or "orchestrator" for system messages)
    pub from: String,
    /// Recipient Agent instance ID
    pub to: String,
    /// Message content
    pub content: String,
    /// Timestamp (ISO 8601)
    pub timestamp: String,
}

impl AgentMessage {
    pub fn new(from: impl Into<String>, to: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            from: from.into(),
            to: to.into(),
            content: content.into(),
            timestamp: chrono::Utc::now().to_rfc3339(),
        }
    }
}

/// Message bus for inter-Agent communication.
///
/// The Orchestrator relays messages between Agents. Each Agent has a mailbox
/// (Vec of messages) that it can read from. Messages are persisted in memory
/// for the lifetime of the orchestration session.
pub struct MessageBus {
    /// Per-agent mailbox: agent_instance_id -> Vec<AgentMessage>
    mailboxes: RwLock<HashMap<String, Vec<AgentMessage>>>,
}

impl MessageBus {
    pub fn new() -> Self {
        Self {
            mailboxes: RwLock::new(HashMap::new()),
        }
    }

    /// Send a message to an Agent's mailbox
    pub async fn send(&self, message: AgentMessage) {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes
            .entry(message.to.clone())
            .or_default()
            .push(message);
    }

    /// Read all messages for an Agent (non-destructive)
    pub async fn read(&self, agent_id: &str) -> Vec<AgentMessage> {
        let mailboxes = self.mailboxes.read().await;
        mailboxes.get(agent_id).cloned().unwrap_or_default()
    }

    /// Drain (consume) all messages for an Agent
    pub async fn drain(&self, agent_id: &str) -> Vec<AgentMessage> {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.remove(agent_id).unwrap_or_default()
    }

    /// Create a mailbox for a new Agent instance
    pub async fn register(&self, agent_id: &str) {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.entry(agent_id.to_string()).or_default();
    }

    /// Remove a mailbox when an Agent instance is destroyed
    pub async fn unregister(&self, agent_id: &str) {
        let mut mailboxes = self.mailboxes.write().await;
        mailboxes.remove(agent_id);
    }
}

impl Default for MessageBus {
    fn default() -> Self {
        Self::new()
    }
}
