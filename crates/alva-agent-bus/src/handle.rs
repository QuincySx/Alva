use std::fmt;
use std::sync::Arc;

use tokio::sync::broadcast;

use crate::caps::Caps;
use crate::event::{BusEvent, EventBus};

/// Runtime bus handle — read-only capabilities + events.
///
/// Distributed to all downstream layers (middleware, tools, context).
/// Does NOT have `provide()` — capability registration is only possible
/// via [`BusWriter`](crate::writer::BusWriter) during initialization.
///
/// This is a compile-time guarantee: if your code receives a `BusHandle`,
/// you cannot register capabilities, only discover and use them.
#[derive(Clone)]
pub struct BusHandle {
    pub(crate) caps: Caps,
    pub(crate) events: EventBus,
}

impl BusHandle {
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
}

impl fmt::Debug for BusHandle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("BusHandle").finish_non_exhaustive()
    }
}
