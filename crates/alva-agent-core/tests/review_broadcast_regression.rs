// INPUT: listenable session, deterministic async listener gates
// OUTPUT: temporary review probe for broadcast order
// POS: Exercise concurrent append callbacks that yield.
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionEvent, SessionEventListener, SessionStore};
use futures_util::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

struct GateFirst {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
    seen: Mutex<Vec<u64>>,
}
#[async_trait::async_trait]
impl SessionEventListener for GateFirst {
    async fn on_event(&self, event: &SessionEvent) {
        if event.seq == 1 {
            self.entered.add_permits(1);
            self.release.acquire().await.unwrap().forget();
        }
        self.seen.lock().unwrap().push(event.seq);
    }
}
#[tokio::test]
async fn concurrent_broadcast_preserves_seq_with_yielding_listener() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = Arc::new(GateFirst {
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
        seen: Mutex::new(vec![]),
    });
    session.subscribe(listener.clone()).await;
    let mut stream = session.subscribe_events(0).await;
    let first = {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .append(SessionEvent::new_runtime("first"))
                .await
                .unwrap();
        })
    };
    listener.entered.acquire().await.unwrap().forget();
    session
        .append(SessionEvent::new_runtime("second"))
        .await
        .unwrap();
    listener.release.add_permits(1);
    first.await.unwrap();
    let seqs = vec![
        stream.next().await.unwrap().seq,
        stream.next().await.unwrap().seq,
    ];
    assert_eq!(
        seqs,
        vec![1, 2],
        "even the channel subscriber receives out-of-order events"
    );
}

/// R12 acceptance: an appending task cancelled mid-drain must not lose (or
/// strand indefinitely) its committed events. The queue retains them; the
/// next subscriber's history replay delivers exactly once.
#[tokio::test]
async fn cancelled_append_task_does_not_lose_committed_events() {
    use alva_kernel_abi::agent_session::EventEmitter;
    use alva_kernel_abi::SessionStore;

    let session = Arc::new(ListenableInMemorySession::new());
    let gate = Arc::new(tokio::sync::Semaphore::new(0));

    // Commit two events through a task we then abort while it is parked
    // delivering them (GateFirst parks on the gate inside on_event).
    let task = {
        let session = session.clone();
        let gate = gate.clone();
        tokio::spawn(async move {
            let store = alva_kernel_abi::agent_session::ScopedSession::new(
                session.clone(),
                alva_kernel_abi::agent_session::EventEmitter::runtime(),
            );
            let mut e1 = SessionEvent::new_runtime("cancelled-drain-1");
            e1.emitter = alva_kernel_abi::agent_session::EventEmitter::runtime();
            store.append(e1).await.unwrap();
            gate.add_permits(1);
            let mut e2 = SessionEvent::new_runtime("cancelled-drain-2");
            e2.emitter = alva_kernel_abi::agent_session::EventEmitter::runtime();
            store.append(e2).await.unwrap();
        })
    };
    gate.add_permits(1);
    let _ = task.await;
    drop(gate);

    // Both events are committed: a fresh subscriber gets both exactly once.
    let mut stream = session.subscribe_events(0).await;
    use futures_util::StreamExt;
    let first = stream.next().await.unwrap();
    assert_eq!(first.event_type, "cancelled-drain-1");
    let second = stream.next().await.unwrap();
    assert_eq!(second.event_type, "cancelled-drain-2");
}
