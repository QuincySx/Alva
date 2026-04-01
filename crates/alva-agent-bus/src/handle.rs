use std::fmt;
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::caps::Caps;
use crate::event::{BusEvent, EventBus};

/// Clone-friendly facade that provides access to [`Caps`] and [`EventBus`].
///
/// Distributed to each layer from a [`Bus`](crate::bus::Bus).
#[derive(Clone)]
pub struct BusHandle {
    pub(crate) caps: Caps,
    pub(crate) events: EventBus,
}

impl BusHandle {
    /// Register a capability.
    pub fn provide<T: Send + Sync + 'static>(&self, value: Arc<T>) {
        self.caps.provide(value);
    }

    /// Look up a capability by type.
    pub fn get<T: Send + Sync + 'static>(&self) -> Option<Arc<T>> {
        self.caps.get()
    }

    /// Look up a capability by type, panicking if missing.
    pub fn require<T: Send + Sync + 'static>(&self) -> Arc<T> {
        self.caps.require()
    }

    /// Check whether a capability is registered.
    pub fn has<T: Send + Sync + 'static>(&self) -> bool {
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
}

impl fmt::Debug for BusHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BusHandle").finish_non_exhaustive()
    }
}
