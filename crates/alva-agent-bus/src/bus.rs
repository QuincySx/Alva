use crate::caps::Caps;
use crate::event::EventBus;
use crate::handle::BusHandle;

/// Top-level coordination bus.
///
/// Owns the [`Caps`] registry and [`EventBus`]; **not** Clone.
/// Call [`Bus::handle`] to create lightweight [`BusHandle`]s for distribution.
pub struct Bus {
    caps: Caps,
    events: EventBus,
}

impl Bus {
    /// Create a new bus with empty capabilities and no event channels.
    pub fn new() -> Self {
        Self {
            caps: Caps::new(),
            events: EventBus::new(),
        }
    }

    /// Create a [`BusHandle`] that shares this bus's capabilities and events.
    pub fn handle(&self) -> BusHandle {
        BusHandle {
            caps: self.caps.clone(),
            events: self.events.clone(),
        }
    }
}

impl Default for Bus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::BusEvent;
    use std::sync::Arc;

    struct MyService;

    #[derive(Clone, Debug, PartialEq)]
    struct Ping(u32);
    impl BusEvent for Ping {}

    #[test]
    fn handles_share_caps() {
        let bus = Bus::new();
        let h1 = bus.handle();
        let h2 = bus.handle();
        h1.provide(Arc::new(MyService));
        assert!(h2.has::<MyService>());
    }

    #[tokio::test]
    async fn handles_share_events() {
        let bus = Bus::new();
        let h1 = bus.handle();
        let h2 = bus.handle();
        let mut rx = h1.subscribe::<Ping>();
        h2.emit(Ping(7));
        assert_eq!(rx.recv().await.unwrap(), Ping(7));
    }

    #[test]
    fn handle_is_debug() {
        let bus = Bus::new();
        let h = bus.handle();
        let debug = format!("{:?}", h);
        assert!(debug.contains("BusHandle"));
    }
}
