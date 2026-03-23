use async_trait::async_trait;
use std::collections::HashMap;
use tokio::sync::Mutex;

/// Trait for persisting and retrieving checkpoint data.
///
/// Checkpoints allow an agent session to save its state at specific points
/// so it can be resumed, replayed, or inspected later.
#[async_trait]
pub trait CheckpointSaver: Send + Sync {
    /// Save checkpoint data under the given id, overwriting any previous value.
    async fn save(&self, id: &str, data: serde_json::Value) -> Result<(), agent_types::AgentError>;

    /// Load checkpoint data by id. Returns `None` if no checkpoint exists.
    async fn load(&self, id: &str) -> Result<Option<serde_json::Value>, agent_types::AgentError>;

    /// List all checkpoint ids.
    async fn list(&self) -> Result<Vec<String>, agent_types::AgentError>;

    /// Delete a checkpoint by id. No-op if it does not exist.
    async fn delete(&self, id: &str) -> Result<(), agent_types::AgentError>;
}

/// A simple in-memory checkpoint saver backed by a `HashMap`.
///
/// Suitable for testing and short-lived sessions. Data is lost when the
/// saver is dropped.
pub struct InMemoryCheckpointSaver {
    store: Mutex<HashMap<String, serde_json::Value>>,
}

impl InMemoryCheckpointSaver {
    pub fn new() -> Self {
        Self {
            store: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryCheckpointSaver {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl CheckpointSaver for InMemoryCheckpointSaver {
    async fn save(&self, id: &str, data: serde_json::Value) -> Result<(), agent_types::AgentError> {
        self.store.lock().await.insert(id.to_string(), data);
        Ok(())
    }

    async fn load(&self, id: &str) -> Result<Option<serde_json::Value>, agent_types::AgentError> {
        Ok(self.store.lock().await.get(id).cloned())
    }

    async fn list(&self) -> Result<Vec<String>, agent_types::AgentError> {
        Ok(self.store.lock().await.keys().cloned().collect())
    }

    async fn delete(&self, id: &str) -> Result<(), agent_types::AgentError> {
        self.store.lock().await.remove(id);
        Ok(())
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[tokio::test]
    async fn save_and_load() {
        let saver = InMemoryCheckpointSaver::new();
        let data = json!({"messages": [{"role": "user", "text": "hello"}]});

        saver.save("cp-1", data.clone()).await.unwrap();
        let loaded = saver.load("cp-1").await.unwrap();
        assert_eq!(loaded, Some(data));
    }

    #[tokio::test]
    async fn load_nonexistent_returns_none() {
        let saver = InMemoryCheckpointSaver::new();
        let loaded = saver.load("does-not-exist").await.unwrap();
        assert_eq!(loaded, None);
    }

    #[tokio::test]
    async fn list_checkpoints() {
        let saver = InMemoryCheckpointSaver::new();
        saver.save("a", json!(1)).await.unwrap();
        saver.save("b", json!(2)).await.unwrap();
        saver.save("c", json!(3)).await.unwrap();

        let mut ids = saver.list().await.unwrap();
        ids.sort();
        assert_eq!(ids, vec!["a", "b", "c"]);
    }

    #[tokio::test]
    async fn delete_checkpoint() {
        let saver = InMemoryCheckpointSaver::new();
        saver.save("x", json!("data")).await.unwrap();
        assert!(saver.load("x").await.unwrap().is_some());

        saver.delete("x").await.unwrap();
        assert!(saver.load("x").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn delete_nonexistent_is_noop() {
        let saver = InMemoryCheckpointSaver::new();
        // Should not error
        saver.delete("nope").await.unwrap();
    }

    #[tokio::test]
    async fn save_overwrites_previous() {
        let saver = InMemoryCheckpointSaver::new();
        saver.save("k", json!("old")).await.unwrap();
        saver.save("k", json!("new")).await.unwrap();

        let loaded = saver.load("k").await.unwrap();
        assert_eq!(loaded, Some(json!("new")));
    }
}
