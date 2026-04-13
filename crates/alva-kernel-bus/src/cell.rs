use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

/// Default capacity for the watch broadcast channel.
const WATCH_CAPACITY: usize = 16;

/// Observable shared state cell.
///
/// Holds a value of type `T` that can be read, written, and watched for changes.
/// Clone shares the underlying state (like `Arc`).
#[derive(Clone)]
pub struct StateCell<T: Clone + Send + Sync + 'static> {
    value: Arc<RwLock<T>>,
    tx: broadcast::Sender<T>,
}

impl<T: Clone + Send + Sync + 'static> StateCell<T> {
    /// Create a new `StateCell` with the given initial value.
    pub fn new(initial: T) -> Self {
        let (tx, _) = broadcast::channel(WATCH_CAPACITY);
        Self {
            value: Arc::new(RwLock::new(initial)),
            tx,
        }
    }

    /// Read the current value.
    pub fn get(&self) -> T {
        self.value.read().clone()
    }

    /// Replace the current value and notify watchers.
    pub fn set(&self, value: T) {
        {
            let mut guard = self.value.write();
            *guard = value.clone();
        }
        // Ignore send errors — no active watchers is fine.
        let _ = self.tx.send(value);
    }

    /// Mutate the value in-place via a closure and notify watchers.
    pub fn update(&self, f: impl FnOnce(&mut T)) {
        let new_value = {
            let mut guard = self.value.write();
            f(&mut *guard);
            guard.clone()
        };
        let _ = self.tx.send(new_value);
    }

    /// Subscribe to change notifications.
    ///
    /// The receiver will get the new value each time `set` or `update` is called.
    pub fn watch(&self) -> broadcast::Receiver<T> {
        self.tx.subscribe()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn get_initial_value() {
        let cell = StateCell::new(42);
        assert_eq!(cell.get(), 42);
    }

    #[test]
    fn set_updates_value() {
        let cell = StateCell::new(0);
        cell.set(10);
        assert_eq!(cell.get(), 10);
    }

    #[tokio::test]
    async fn watch_receives_changes() {
        let cell = StateCell::new(0);
        let mut rx = cell.watch();
        cell.set(5);
        assert_eq!(rx.recv().await.unwrap(), 5);
    }

    #[test]
    fn update_in_place() {
        let cell = StateCell::new(vec![1, 2]);
        cell.update(|v| v.push(3));
        assert_eq!(cell.get(), vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn update_notifies_watchers() {
        let cell = StateCell::new(10);
        let mut rx = cell.watch();
        cell.update(|v| *v += 5);
        assert_eq!(rx.recv().await.unwrap(), 15);
    }

    #[test]
    fn clone_shares_state() {
        let cell = StateCell::new(1);
        let cell2 = cell.clone();
        cell.set(99);
        assert_eq!(cell2.get(), 99);
    }

    #[tokio::test]
    async fn multiple_watchers() {
        let cell = StateCell::new("a".to_string());
        let mut rx1 = cell.watch();
        let mut rx2 = cell.watch();
        cell.set("b".to_string());
        assert_eq!(rx1.recv().await.unwrap(), "b");
        assert_eq!(rx2.recv().await.unwrap(), "b");
    }
}
