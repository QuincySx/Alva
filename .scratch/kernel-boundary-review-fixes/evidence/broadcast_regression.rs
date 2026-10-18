// INPUT: listenable session, deterministic async listener gates
// OUTPUT: temporary review probe for broadcast order
// POS: Exercise concurrent append callbacks that yield.
use std::sync::{Arc, Mutex};
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{ConversationLog, SessionStore, SessionEvent, SessionEventListener};
use futures_util::StreamExt;

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
    let first = { let session = session.clone(); tokio::spawn(async move {
        session.append(SessionEvent::new_runtime("first")).await.unwrap();
    })};
    listener.entered.acquire().await.unwrap().forget();
    session.append(SessionEvent::new_runtime("second")).await.unwrap();
    listener.release.add_permits(1);
    first.await.unwrap();
    let seqs = vec![stream.next().await.unwrap().seq, stream.next().await.unwrap().seq];
    assert_eq!(seqs, vec![1, 2], "even the channel subscriber receives out-of-order events");
}
