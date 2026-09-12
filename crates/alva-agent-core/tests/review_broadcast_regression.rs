// INPUT: listenable session, deterministic async listener gates, tokio + futures executors
// OUTPUT: broadcast delivery contract regressions (order, appender isolation, driver wake, clear generations)
// POS: R03/R12/R16/R25/R26 regressions — appenders commit and return, a dedicated driver delivers, clear isolates generations.
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

/// Blocks inside `on_event` until released; `entered` counts entries so tests
/// can wait deterministically for the driver to reach the callback.
struct BlockingListener {
    entered: tokio::sync::Semaphore,
    release: tokio::sync::Semaphore,
}
#[async_trait::async_trait]
impl SessionEventListener for BlockingListener {
    async fn on_event(&self, _: &SessionEvent) {
        self.entered.add_permits(1);
        self.release.acquire().await.unwrap().forget();
    }
}
fn gate() -> Arc<BlockingListener> {
    Arc::new(BlockingListener {
        entered: tokio::sync::Semaphore::new(0),
        release: tokio::sync::Semaphore::new(0),
    })
}

/// R25 acceptance: with a runtime present, a blocking listener must not block
/// any appender. `append` commits, queues, wakes the driver, and returns —
/// delivery (including the blocked callback) is the driver's business.
#[tokio::test]
async fn first_append_finishes_while_listener_is_blocked() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = gate();
    session.subscribe(listener.clone()).await;
    let mut append = {
        let session = session.clone();
        tokio::spawn(async move { session.append(SessionEvent::new_runtime("first")).await.unwrap(); })
    };
    listener.entered.acquire().await.unwrap().forget();
    let finished = tokio::time::timeout(Duration::from_millis(100), &mut append)
        .await
        .is_ok();
    listener.release.add_permits(1);
    if !finished {
        append.await.unwrap();
    }
    assert!(
        finished,
        "the first append remains blocked on the arbitrary listener callback"
    );
}

/// R25 acceptance: the same appender contract holds for `append_message` and
/// `append_records`.
#[tokio::test]
async fn append_message_and_records_return_while_listener_is_blocked() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = gate();
    session.subscribe(listener.clone()).await;
    let mut appends = {
        let session = session.clone();
        tokio::spawn(async move {
            session
                .append_message(
                    alva_kernel_abi::AgentMessage::Standard(alva_kernel_abi::base::message::Message::user("typed block")),
                    None,
                )
                .await
                .unwrap();
            let record =
                |name: &str| alva_kernel_abi::SessionRecord::from_event(SessionEvent::new_runtime(name));
            session.append_records(vec![record("typed")]).await.unwrap();
        })
    };
    listener.entered.acquire().await.unwrap().forget();
    let finished = tokio::time::timeout(Duration::from_millis(100), &mut appends)
        .await
        .is_ok();
    listener.release.add_permits(2);
    if !finished {
        appends.await.unwrap();
    }
    assert!(
        finished,
        "append_message/append_records block on the listener callback"
    );
}

/// R25 acceptance: after the driver finishes the first batch it parks; a
/// later `append_records` must wake it. Proof must come from an ALREADY
/// subscribed stream — a fresh subscriber's history replay would mask a
/// dead driver.
#[tokio::test]
async fn typed_append_wakes_idle_driver() {
    let session = ListenableInMemorySession::new();
    let mut live = session.subscribe_events(0).await;
    let record =
        |name: &str| alva_kernel_abi::SessionRecord::from_event(SessionEvent::new_runtime(name));
    session.append_records(vec![record("first")]).await.unwrap();
    tokio::time::timeout(Duration::from_millis(200), live.next())
        .await
        .unwrap()
        .unwrap();
    // Current-thread runtime: let the driver finish draining and park.
    tokio::task::yield_now().await;
    session.append_records(vec![record("second")]).await.unwrap();
    let event = tokio::time::timeout(Duration::from_millis(200), live.next())
        .await
        .expect("typed record committed but idle driver never woke")
        .unwrap();
    assert_eq!(event.event_type, "second");
}

/// R25 acceptance: really aborting an outer caller (JoinError::is_cancelled)
/// must not strand events it already committed — an existing live
/// subscription still receives them from the driver.
#[tokio::test]
async fn cancelled_appender_preserves_existing_live_subscription() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = gate();
    session.subscribe(listener.clone()).await;
    let mut live = session.subscribe_events(0).await;
    let caller_liveness = Arc::new(tokio::sync::Semaphore::new(0));
    let append = {
        let session = session.clone();
        let caller_liveness = caller_liveness.clone();
        tokio::spawn(async move {
            session
                .append(SessionEvent::new_runtime("committed-before-abort"))
                .await
                .unwrap();
            caller_liveness.acquire().await.unwrap().forget();
        })
    };
    listener.entered.acquire().await.unwrap().forget();
    append.abort();
    assert!(
        append.await.unwrap_err().is_cancelled(),
        "must actually cancel while the driver is blocked delivering"
    );
    listener.release.add_permits(1);
    let event = tokio::time::timeout(Duration::from_millis(100), live.next())
        .await
        .expect("already subscribed consumer permanently lost the committed event after append cancellation")
        .unwrap();
    assert_eq!(event.event_type, "committed-before-abort");
}

/// R12 acceptance (R25 fix): an appending task REALLY aborted mid-flight must
/// not lose (or strand indefinitely) its committed events. The queue retains
/// them; a fresh subscriber's history replay delivers exactly once. Unlike
/// the pre-R25 version of this test, the abort is real (checked via
/// `JoinError::is_cancelled`), not just a completed task.
#[tokio::test]
async fn cancelled_append_task_does_not_lose_committed_events() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = gate();
    session.subscribe(listener.clone()).await;

    let task = {
        let session = session.clone();
        tokio::spawn(async move {
            let store = alva_kernel_abi::agent_session::ScopedSession::new(
                session.clone(),
                alva_kernel_abi::agent_session::EventEmitter::runtime(),
            );
            let mut e1 = SessionEvent::new_runtime("cancelled-drain-1");
            e1.emitter = alva_kernel_abi::agent_session::EventEmitter::runtime();
            store.append(e1).await.unwrap();
            // Park the appender itself; the committed event is the driver's
            // responsibility now. Aborting HERE proves delivery survives the
            // appender's cancellation.
            futures_util::future::pending::<()>().await;
        })
    };
    listener.entered.acquire().await.unwrap().forget();
    task.abort();
    assert!(
        task.await.unwrap_err().is_cancelled(),
        "test must abort the appender for real"
    );
    listener.release.add_permits(1);

    // The event is committed: a fresh subscriber gets it exactly once.
    let mut stream = session.subscribe_events(0).await;
    let first = tokio::time::timeout(Duration::from_millis(200), stream.next())
        .await
        .expect("committed event lost after appender abort")
        .unwrap();
    assert_eq!(first.event_type, "cancelled-drain-1");
    assert!(
        tokio::time::timeout(Duration::from_millis(30), stream.next())
            .await
            .is_err(),
        "history replay delivered the aborted appender's event twice"
    );
}

/// R26 acceptance: clear during an in-flight delivery. The old generation's
/// blocked callback must not advance the reset cursor past the new
/// generation's first event.
#[tokio::test]
async fn clear_during_delivery_does_not_skip_new_generation() {
    let session = Arc::new(ListenableInMemorySession::new());
    let listener = gate();
    session.subscribe(listener.clone()).await;
    let mut live = session.subscribe_events(0).await;
    let first = {
        let session = session.clone();
        tokio::spawn(async move { session.append(SessionEvent::new_runtime("old")).await.unwrap(); })
    };
    listener.entered.acquire().await.unwrap().forget();
    session.clear().await.unwrap();
    session.append(SessionEvent::new_runtime("new")).await.unwrap();
    listener.release.add_permits(2);
    first.await.unwrap();
    let result = tokio::time::timeout(Duration::from_millis(200), async {
        while let Some(event) = live.next().await {
            if event.event_type == "new" {
                return;
            }
        }
        panic!("stream ended without new event");
    })
    .await;
    assert!(
        result.is_ok(),
        "old in-flight delivery advances reset cursor and skips new seq 1"
    );
}

/// R26 acceptance: restore rebuilds history without breaking the
/// history-to-live boundary for streams created afterwards.
#[tokio::test]
async fn restore_then_append_reaches_new_subscriber() {
    let session = ListenableInMemorySession::new();
    let mut restored = SessionEvent::new_runtime("restored");
    restored.seq = 7;
    session.restore_events(vec![restored]).await;
    let mut stream = session.subscribe_events(0).await;
    session
        .append(SessionEvent::new_runtime("live-after-restore"))
        .await
        .unwrap();
    let first = tokio::time::timeout(Duration::from_millis(200), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.event_type, "restored");
    let second = tokio::time::timeout(Duration::from_millis(200), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.event_type, "live-after-restore");
    assert_eq!(second.seq, 8, "seq continues past restored history");
}

/// R26 acceptance: rollback truncates history but never reuses seqs — the
/// next live event still reaches subscribers (history/live boundary).
#[tokio::test]
async fn rollback_then_append_still_reaches_subscriber() {
    let session = ListenableInMemorySession::new();
    session
        .append(SessionEvent::new_runtime("keep"))
        .await
        .unwrap();
    let mut stream = session.subscribe_events(0).await;
    let keep = stream.next().await.unwrap(); // "keep" via history replay
    session
        .append(SessionEvent::new_runtime("doomed"))
        .await
        .unwrap();
    let doomed = tokio::time::timeout(Duration::from_millis(200), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(doomed.event_type, "doomed");
    // rollback AFTER `keep` removes the doomed event from the log.
    let removed = session.rollback_after(&keep.uuid).await;
    assert_eq!(removed, 1, "rollback removes the doomed event");
    session
        .append(SessionEvent::new_runtime("after-rollback"))
        .await
        .unwrap();
    let next = tokio::time::timeout(Duration::from_millis(200), stream.next())
        .await
        .expect("post-rollback append lost")
        .unwrap();
    assert_eq!(next.event_type, "after-rollback");
    assert_eq!(next.seq, 3, "seq counter must not reset on rollback");
}

/// R25 acceptance: the executor-neutral path (no Tokio runtime) advances
/// delivery inline — append returns only after the event is delivered.
#[test]
fn without_tokio_runtime_appends_deliver_inline() {
    let session = ListenableInMemorySession::new();
    let mut stream = futures_executor::block_on(session.subscribe_events(0));
    futures_executor::block_on(session.append(SessionEvent::new_runtime("inline")))
        .unwrap();
    let event = futures_executor::block_on(stream.next()).unwrap();
    assert_eq!(event.event_type, "inline");
}

/// R25 acceptance: dropped subscribers are reaped instead of accumulating.
#[tokio::test]
async fn dropped_stream_listener_is_reaped() {
    let session = Arc::new(ListenableInMemorySession::new());
    {
        let stream = session.subscribe_events(0).await;
        drop(stream);
    }
    session
        .append(SessionEvent::new_runtime("trigger-reap"))
        .await
        .unwrap();
    // Give the driver a chance to run the reap, then assert via a
    // follow-up delivery completing promptly.
    let mut probe = session.subscribe_events(0).await;
    let event = tokio::time::timeout(Duration::from_millis(200), probe.next())
        .await
        .expect("reap path blocked later deliveries")
        .unwrap();
    assert_eq!(event.event_type, "trigger-reap");
}

/// R25 acceptance: a session dropped with events still queued must not hang
/// or panic the runtime — the driver exits via its Weak handle.
#[tokio::test]
async fn dropped_session_with_queued_events_shuts_down_cleanly() {
    let listener = gate();
    let spawned = tokio::spawn(async move {
        let session = Arc::new(ListenableInMemorySession::new());
        session.subscribe(listener.clone()).await;
        session
            .append(SessionEvent::new_runtime("never-delivered"))
            .await
            .unwrap();
        // Driver is blocked in the listener; drop the session with the batch
        // still in flight.
        drop(session);
        // Releasing after the drop proves the driver tolerates its core
        // vanishing mid-callback (upgrade fails on next loop).
        listener.release.add_permits(1);
    });
    spawned.await.unwrap();
}
