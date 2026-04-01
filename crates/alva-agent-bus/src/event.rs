use std::any::{Any, TypeId};
use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use tokio::sync::broadcast;

/// Default capacity for event broadcast channels.
const DEFAULT_CHANNEL_CAPACITY: usize = 64;

/// Marker trait for events that can be sent through the [`EventBus`].
pub trait BusEvent: Clone + Send + Sync + 'static {}

/// Typed publish/subscribe event bus backed by `tokio::sync::broadcast` channels.
///
/// Channels are created lazily on first `emit` or `subscribe` for each event type.
#[derive(Clone)]
pub struct EventBus {
    channels: Arc<RwLock<HashMap<TypeId, Box<dyn Any + Send + Sync>>>>,
}

impl EventBus {
    /// Create a new, empty event bus.
    pub fn new() -> Self {
        Self {
            channels: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Emit an event to all current subscribers (non-blocking).
    ///
    /// If there are no subscribers, the event is silently dropped.
    pub fn emit<E: BusEvent>(&self, event: E) {
        let channels = self.channels.read();
        if let Some(boxed) = channels.get(&TypeId::of::<E>()) {
            if let Some(tx) = boxed.downcast_ref::<broadcast::Sender<E>>() {
                // Ignore send errors — they just mean no active receivers.
                let _ = tx.send(event);
            }
        }
        // No channel exists → no subscribers → silently drop.
    }

    /// Subscribe to events of type `E`.
    ///
    /// Creates the channel lazily if it doesn't exist yet.
    pub fn subscribe<E: BusEvent>(&self) -> broadcast::Receiver<E> {
        let mut channels = self.channels.write();
        let tx = channels
            .entry(TypeId::of::<E>())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel::<E>(DEFAULT_CHANNEL_CAPACITY);
                Box::new(tx)
            })
            .downcast_ref::<broadcast::Sender<E>>()
            .expect("EventBus: type mismatch in channel map")
            .clone();
        tx.subscribe()
    }

    /// Check whether there are active subscribers for event type `E`.
    pub fn has_subscribers<E: BusEvent>(&self) -> bool {
        let channels = self.channels.read();
        channels
            .get(&TypeId::of::<E>())
            .and_then(|boxed| boxed.downcast_ref::<broadcast::Sender<E>>())
            .map(|tx| tx.receiver_count() > 0)
            .unwrap_or(false)
    }
}

impl Default for EventBus {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    struct ChatMessage(String);
    impl BusEvent for ChatMessage {}

    #[derive(Clone, Debug, PartialEq)]
    struct SystemAlert(u32);
    impl BusEvent for SystemAlert {}

    #[tokio::test]
    async fn emit_and_subscribe() {
        let bus = EventBus::new();
        let mut rx = bus.subscribe::<ChatMessage>();
        bus.emit(ChatMessage("hello".into()));
        let msg = rx.recv().await.unwrap();
        assert_eq!(msg, ChatMessage("hello".into()));
    }

    #[tokio::test]
    async fn broadcast_to_multiple_subscribers() {
        let bus = EventBus::new();
        let mut rx1 = bus.subscribe::<ChatMessage>();
        let mut rx2 = bus.subscribe::<ChatMessage>();
        bus.emit(ChatMessage("hi".into()));
        assert_eq!(rx1.recv().await.unwrap(), ChatMessage("hi".into()));
        assert_eq!(rx2.recv().await.unwrap(), ChatMessage("hi".into()));
    }

    #[tokio::test]
    async fn different_types_are_isolated() {
        let bus = EventBus::new();
        let mut rx_chat = bus.subscribe::<ChatMessage>();
        let mut rx_alert = bus.subscribe::<SystemAlert>();

        bus.emit(ChatMessage("msg".into()));
        bus.emit(SystemAlert(42));

        assert_eq!(rx_chat.recv().await.unwrap(), ChatMessage("msg".into()));
        assert_eq!(rx_alert.recv().await.unwrap(), SystemAlert(42));
    }

    #[test]
    fn emit_without_subscriber_does_not_panic() {
        let bus = EventBus::new();
        bus.emit(ChatMessage("nobody listening".into()));
    }

    #[test]
    fn has_subscribers_reflects_state() {
        let bus = EventBus::new();
        assert!(!bus.has_subscribers::<ChatMessage>());
        let _rx = bus.subscribe::<ChatMessage>();
        assert!(bus.has_subscribers::<ChatMessage>());
    }

    #[test]
    fn dropped_subscriber_ok() {
        let bus = EventBus::new();
        let rx = bus.subscribe::<ChatMessage>();
        drop(rx);
        // Emitting after all subscribers dropped should not panic.
        bus.emit(ChatMessage("gone".into()));
        assert!(!bus.has_subscribers::<ChatMessage>());
    }

    #[tokio::test]
    async fn clone_shares_state() {
        let bus = EventBus::new();
        let bus2 = bus.clone();
        let mut rx = bus.subscribe::<ChatMessage>();
        bus2.emit(ChatMessage("from clone".into()));
        assert_eq!(rx.recv().await.unwrap(), ChatMessage("from clone".into()));
    }
}
