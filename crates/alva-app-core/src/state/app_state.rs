//! Core application state and store.

use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{RwLock, watch};

/// Application-level state shared across the session.
///
/// This is the single source of truth — UI layers read from it,
/// and the agent runtime writes to it via `AppStateStore`.
#[derive(Debug, Clone)]
pub struct AppState {
    // -- Session --
    pub session_id: String,
    pub model: String,
    pub message_count: usize,

    // -- Tokens --
    pub input_tokens: u64,
    pub output_tokens: u64,

    // -- Mode --
    pub plan_mode: bool,
    pub vim_mode: bool,

    // -- Loading --
    pub is_loading: bool,
    pub loading_message: Option<String>,

    // -- Tools --
    pub tool_names: Vec<String>,
    pub mcp_tool_names: Vec<String>,

    // -- Tasks --
    pub tasks: HashMap<String, TaskEntry>,

    // -- Workspace --
    pub workspace: String,
    pub git_branch: Option<String>,
}

/// A lightweight task entry visible in the state.
#[derive(Debug, Clone)]
pub struct TaskEntry {
    pub id: String,
    pub status: TaskStatus,
    pub description: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            session_id: String::new(),
            model: String::new(),
            message_count: 0,
            input_tokens: 0,
            output_tokens: 0,
            plan_mode: false,
            vim_mode: false,
            is_loading: false,
            loading_message: None,
            tool_names: Vec::new(),
            mcp_tool_names: Vec::new(),
            tasks: HashMap::new(),
            workspace: String::new(),
            git_branch: None,
        }
    }
}

/// Subscriber trait for state change notifications.
pub trait StateSubscriber: Send + Sync {
    /// Called when the state changes. The snapshot is a clone at notification time.
    fn on_state_change(&self, state: &AppState);
}

/// Thread-safe state store with `watch` channel for change notification.
///
/// Writers call `update()` with a closure that mutates the state.
/// Readers can `snapshot()` or subscribe via `watch_rx()`.
pub struct AppStateStore {
    state: Arc<RwLock<AppState>>,
    notify_tx: watch::Sender<u64>,
    notify_rx: watch::Receiver<u64>,
    version: Arc<std::sync::atomic::AtomicU64>,
}

impl AppStateStore {
    pub fn new(initial: AppState) -> Self {
        let (tx, rx) = watch::channel(0u64);
        Self {
            state: Arc::new(RwLock::new(initial)),
            notify_tx: tx,
            notify_rx: rx,
            version: Arc::new(std::sync::atomic::AtomicU64::new(0)),
        }
    }

    /// Get a read-only snapshot of the current state.
    pub async fn snapshot(&self) -> AppState {
        self.state.read().await.clone()
    }

    /// Update the state with a closure. Notifies all watchers after the update.
    pub async fn update<F>(&self, f: F)
    where
        F: FnOnce(&mut AppState),
    {
        {
            let mut state = self.state.write().await;
            f(&mut state);
        }
        let v = self
            .version
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let _ = self.notify_tx.send(v + 1);
    }

    /// Get a `watch::Receiver` that is notified on every state change.
    ///
    /// The value is a monotonically increasing version number.
    pub fn watch_rx(&self) -> watch::Receiver<u64> {
        self.notify_rx.clone()
    }

    /// Current version number (increments on every update).
    pub fn version(&self) -> u64 {
        self.version
            .load(std::sync::atomic::Ordering::Relaxed)
    }

    /// Get a clone of the inner `Arc<RwLock<AppState>>` for direct access
    /// in performance-sensitive paths (e.g., rendering loops).
    pub fn shared(&self) -> Arc<RwLock<AppState>> {
        self.state.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn initial_state_is_default() {
        let store = AppStateStore::new(AppState::default());
        let snap = store.snapshot().await;
        assert_eq!(snap.session_id, "");
        assert_eq!(snap.message_count, 0);
        assert_eq!(snap.input_tokens, 0);
        assert!(!snap.plan_mode);
        assert!(!snap.is_loading);
    }

    #[tokio::test]
    async fn update_modifies_state() {
        let store = AppStateStore::new(AppState::default());
        store
            .update(|s| {
                s.session_id = "abc-123".to_string();
                s.model = "claude-opus".to_string();
                s.message_count = 5;
            })
            .await;

        let snap = store.snapshot().await;
        assert_eq!(snap.session_id, "abc-123");
        assert_eq!(snap.model, "claude-opus");
        assert_eq!(snap.message_count, 5);
    }

    #[tokio::test]
    async fn update_increments_version() {
        let store = AppStateStore::new(AppState::default());
        assert_eq!(store.version(), 0);

        store.update(|s| s.message_count = 1).await;
        assert_eq!(store.version(), 1);

        store.update(|s| s.message_count = 2).await;
        assert_eq!(store.version(), 2);
    }

    #[tokio::test]
    async fn watch_notifies_on_update() {
        let store = AppStateStore::new(AppState::default());
        let mut rx = store.watch_rx();

        store.update(|s| s.input_tokens = 100).await;

        // The receiver should have been notified
        rx.changed().await.unwrap();
        let version = *rx.borrow_and_update();
        assert_eq!(version, 1);
    }

    #[tokio::test]
    async fn concurrent_reads_and_writes() {
        let store = Arc::new(AppStateStore::new(AppState::default()));

        let mut handles = Vec::new();

        // Spawn 10 writers
        for i in 0..10u64 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                s.update(|state| {
                    state.input_tokens += i;
                })
                .await;
            }));
        }

        // Spawn 10 readers
        for _ in 0..10 {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let _snap = s.snapshot().await;
            }));
        }

        for h in handles {
            h.await.unwrap();
        }

        let snap = store.snapshot().await;
        // Sum of 0..10 = 45
        assert_eq!(snap.input_tokens, 45);
    }

    #[tokio::test]
    async fn token_accumulation() {
        let store = AppStateStore::new(AppState::default());

        store
            .update(|s| {
                s.input_tokens += 1000;
                s.output_tokens += 200;
            })
            .await;
        store
            .update(|s| {
                s.input_tokens += 500;
                s.output_tokens += 100;
            })
            .await;

        let snap = store.snapshot().await;
        assert_eq!(snap.input_tokens, 1500);
        assert_eq!(snap.output_tokens, 300);
    }

    #[tokio::test]
    async fn task_management() {
        let store = AppStateStore::new(AppState::default());

        store
            .update(|s| {
                s.tasks.insert(
                    "t1".to_string(),
                    TaskEntry {
                        id: "t1".to_string(),
                        status: TaskStatus::Running,
                        description: "Build feature".to_string(),
                    },
                );
            })
            .await;

        let snap = store.snapshot().await;
        assert_eq!(snap.tasks.len(), 1);
        assert_eq!(snap.tasks["t1"].status, TaskStatus::Running);

        store
            .update(|s| {
                if let Some(t) = s.tasks.get_mut("t1") {
                    t.status = TaskStatus::Completed;
                }
            })
            .await;

        let snap = store.snapshot().await;
        assert_eq!(snap.tasks["t1"].status, TaskStatus::Completed);
    }

    #[tokio::test]
    async fn plan_mode_toggle() {
        let store = AppStateStore::new(AppState::default());

        store.update(|s| s.plan_mode = true).await;
        assert!(store.snapshot().await.plan_mode);

        store.update(|s| s.plan_mode = false).await;
        assert!(!store.snapshot().await.plan_mode);
    }
}
