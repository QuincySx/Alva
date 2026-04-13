// INPUT:  crate::caps::Caps, crate::event::{BusEvent, EventBus}, crate::handle::BusHandle, std::sync::Arc, tokio::sync::broadcast
// OUTPUT: BusWriter
// POS:    Init-phase bus handle with write access — can register capabilities, emit events, and create read-only BusHandle snapshots.
use std::fmt;
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::caps::Caps;
use crate::event::{BusEvent, EventBus};
use crate::handle::BusHandle;

/// Init-phase bus handle — can register capabilities.
///
/// Only available during initialization (e.g., `BaseAgent::build()`).
/// After init, call `.freeze()` or distribute `BusHandle` (read-only)
/// to downstream layers.
///
/// Compile-time guarantee: downstream code receives `BusHandle` which
/// has no `provide()` method — attempting to register at runtime is a
/// compile error, not a runtime error.
#[derive(Clone)]
pub struct BusWriter {
    pub(crate) caps: Caps,
    pub(crate) events: EventBus,
}

impl BusWriter {
    /// Register a capability (only available on BusWriter, not BusHandle).
    pub fn provide<T: Send + Sync + ?Sized + 'static>(&self, value: Arc<T>) {
        self.caps.provide(value);
    }

    /// Look up a capability by type.
    pub fn get<T: Send + Sync + ?Sized + 'static>(&self) -> Option<Arc<T>> {
        self.caps.get()
    }

    /// Look up a capability by type, panicking if missing.
    pub fn require<T: Send + Sync + ?Sized + 'static>(&self) -> Arc<T> {
        self.caps.require()
    }

    /// Check whether a capability is registered.
    pub fn has<T: Send + Sync + ?Sized + 'static>(&self) -> bool {
        self.caps.has::<T>()
    }

    /// Emit an event to all subscribers.
    pub fn emit<E: BusEvent>(&self, event: E) {
        self.events.emit(event);
    }

    /// Subscribe to events of type `E`.
    pub fn subscribe<E: BusEvent>(&self) -> broadcast::Receiver<E> {
        self.events.subscribe()
    }

    /// Create a read-only handle for distribution to downstream layers.
    ///
    /// The returned `BusHandle` shares the same Caps and EventBus but
    /// does NOT have a `provide()` method.
    pub fn handle(&self) -> BusHandle {
        BusHandle {
            caps: self.caps.clone(),
            events: self.events.clone(),
        }
    }
}

impl fmt::Debug for BusWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BusWriter").finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bus::Bus;
    use crate::event::BusEvent;

    struct DatabasePool;
    struct HttpClient;

    #[derive(Clone, Debug, PartialEq)]
    struct TestEvent(String);
    impl BusEvent for TestEvent {}

    /// Helper: create a fresh BusWriter via Bus.
    fn make_writer() -> BusWriter {
        Bus::new().writer()
    }

    // ---- provide() + get() round-trip ----

    #[test]
    fn provide_and_get_round_trip() {
        let w = make_writer();
        w.provide(Arc::new(42_u32));
        let val = w.get::<u32>();
        assert!(val.is_some());
        assert_eq!(*val.unwrap(), 42);
    }

    #[test]
    fn get_missing_returns_none() {
        let w = make_writer();
        assert!(w.get::<DatabasePool>().is_none());
    }

    // ---- require() panics when missing ----

    #[test]
    #[should_panic(expected = "required capability")]
    fn require_panics_when_missing() {
        let w = make_writer();
        let _: Arc<DatabasePool> = w.require::<DatabasePool>();
    }

    #[test]
    fn require_returns_value_when_present() {
        let w = make_writer();
        w.provide(Arc::new(HttpClient));
        let _: Arc<HttpClient> = w.require::<HttpClient>();
    }

    // ---- has() returns correct bool ----

    #[test]
    fn has_returns_false_when_missing() {
        let w = make_writer();
        assert!(!w.has::<DatabasePool>());
    }

    #[test]
    fn has_returns_true_after_provide() {
        let w = make_writer();
        w.provide(Arc::new(DatabasePool));
        assert!(w.has::<DatabasePool>());
    }

    // ---- emit() + subscribe() ----

    #[tokio::test]
    async fn emit_and_subscribe_works() {
        let w = make_writer();
        let mut rx = w.subscribe::<TestEvent>();
        w.emit(TestEvent("hello".into()));
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg, TestEvent("hello".into()));
    }

    #[tokio::test]
    async fn emit_multiple_events() {
        let w = make_writer();
        let mut rx = w.subscribe::<TestEvent>();
        w.emit(TestEvent("first".into()));
        w.emit(TestEvent("second".into()));
        assert_eq!(rx.recv().await.unwrap(), TestEvent("first".into()));
        assert_eq!(rx.recv().await.unwrap(), TestEvent("second".into()));
    }

    // ---- handle() returns read-only BusHandle ----

    #[test]
    fn handle_can_read_what_writer_wrote() {
        let w = make_writer();
        w.provide(Arc::new(99_u32));
        let h = w.handle();
        let val = h.get::<u32>();
        assert!(val.is_some());
        assert_eq!(*val.unwrap(), 99);
    }

    #[test]
    fn handle_has_reflects_writer_state() {
        let w = make_writer();
        let h = w.handle();
        assert!(!h.has::<DatabasePool>());
        w.provide(Arc::new(DatabasePool));
        assert!(h.has::<DatabasePool>());
    }

    #[tokio::test]
    async fn handle_receives_events_from_writer() {
        let w = make_writer();
        let h = w.handle();
        let mut rx = h.subscribe::<TestEvent>();
        w.emit(TestEvent("via writer".into()));
        assert_eq!(rx.recv().await.unwrap(), TestEvent("via writer".into()));
    }

    #[test]
    fn writer_is_debug() {
        let w = make_writer();
        let debug = format!("{:?}", w);
        assert!(debug.contains("BusWriter"));
    }
}
