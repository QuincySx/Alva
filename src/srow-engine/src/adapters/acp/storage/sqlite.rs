//! ACP message persistence layer.
//!
//! This module defines the storage interface for ACP messages.
//! The actual SQLite implementation requires `tokio-rusqlite` which will be
//! added as a dependency when Sub-6 (persistence) is implemented.
//! For now, this provides an in-memory stub that satisfies the API contract.

use crate::adapters::acp::AcpError;

/// ACP message storage.
///
/// Phase 1: in-memory Vec storage (no SQLite dependency).
/// Phase 2 (Sub-6): backed by tokio-rusqlite with `acp_messages` table.
pub struct AcpMessageStorage {
    messages: tokio::sync::Mutex<Vec<StoredMessage>>,
}

#[derive(Debug, Clone)]
struct StoredMessage {
    id: String,
    conversation_id: String,
    process_id: String,
    message_type: String,
    content: serde_json::Value,
    timestamp_ms: i64,
    created_at: String,
}

impl AcpMessageStorage {
    /// Create a new in-memory storage instance.
    pub fn new() -> Self {
        Self {
            messages: tokio::sync::Mutex::new(Vec::new()),
        }
    }

    /// Record an ACP message (tool call / permission / error / finish).
    pub async fn record_message(
        &self,
        conversation_id: &str,
        process_id: &str,
        message_type: &str,
        content: &serde_json::Value,
    ) -> Result<(), AcpError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now_ms = chrono::Utc::now().timestamp_millis();
        let now_str = chrono::Utc::now().to_rfc3339();

        self.messages.lock().await.push(StoredMessage {
            id,
            conversation_id: conversation_id.to_string(),
            process_id: process_id.to_string(),
            message_type: message_type.to_string(),
            content: content.clone(),
            timestamp_ms: now_ms,
            created_at: now_str,
        });

        Ok(())
    }

    /// Query historical messages for a session.
    pub async fn get_messages(
        &self,
        conversation_id: &str,
    ) -> Result<Vec<serde_json::Value>, AcpError> {
        let messages = self.messages.lock().await;
        let results: Vec<serde_json::Value> = messages
            .iter()
            .filter(|m| m.conversation_id == conversation_id)
            .map(|m| m.content.clone())
            .collect();
        Ok(results)
    }
}

impl Default for AcpMessageStorage {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_record_and_get_messages() {
        let storage = AcpMessageStorage::new();
        let content = serde_json::json!({"tool_name": "read_file", "output": "file content"});

        storage
            .record_message("conv-1", "proc-1", "acp_tool_call", &content)
            .await
            .unwrap();

        let messages = storage.get_messages("conv-1").await.unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0]["tool_name"], "read_file");

        // Different conversation should return empty
        let other = storage.get_messages("conv-2").await.unwrap();
        assert!(other.is_empty());
    }
}
