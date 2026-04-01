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
