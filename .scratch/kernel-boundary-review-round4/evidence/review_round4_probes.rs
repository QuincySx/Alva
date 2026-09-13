// INPUT: real listenable session, runtime tool context, deterministic async gates
// OUTPUT: review-only regressions for session assembly and broadcast delivery
// POS: fourth-round probes of production public interfaces; no implementation changes.
use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_kernel_abi::{CancellationToken, ConversationLog, SessionEvent, SessionEventListener, SessionStore, StoreSessionCell};
use alva_kernel_abi::tool::execution::ToolExecutionContext;
use alva_kernel_core::RuntimeExecutionContext;
use futures_util::StreamExt;
use std::sync::Arc;
use std::time::Duration;

struct DummyModel;
#[async_trait::async_trait]
impl alva_kernel_abi::LanguageModel for DummyModel {
    async fn complete(&self, _: &[alva_kernel_abi::Message], _: &[&dyn alva_kernel_abi::Tool], _: &alva_kernel_abi::ModelConfig) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
        Ok(alva_kernel_abi::CompletionResponse::from_message(alva_kernel_abi::Message::system("ok")))
    }
    fn stream(&self, _: &[alva_kernel_abi::Message], _: &[&dyn alva_kernel_abi::Tool], _: &alva_kernel_abi::ModelConfig) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = alva_kernel_abi::StreamEvent> + Send>> {
        Box::pin(futures_util::stream::empty())
    }
    fn model_id(&self) -> &str { "review-dummy" }
}

#[tokio::test]
async fn assembly_exposes_the_key_consumed_by_tool_batch() {
    let agent = alva_agent_core::AgentBuilder::new().model(Arc::new(DummyModel)).build().await.unwrap();
    let state = agent.state().lock().await;
    assert!(state.extensions.get::<Arc<dyn alva_kernel_abi::PersistentSessions>>().is_some());
    // Exactly the lookup at kernel-core/src/tool_batch.rs:650.
    assert!(state.extensions.get::<StoreSessionCell>().is_some(), "tool_batch reads a different typed key than the builder registers");
}

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

#[tokio::test]
async fn typed_append_wakes_idle_driver() {
    let session = ListenableInMemorySession::new();
    let mut live = session.subscribe_events(0).await;
    let record = |name: &str| alva_kernel_abi::SessionRecord::from_event(SessionEvent::new_runtime(name));
    session.append_records(vec![record("first")]).await.unwrap();
    tokio::time::timeout(Duration::from_millis(200), live.next()).await.unwrap().unwrap();
    // Current-thread runtime: let the driver finish draining and park.
    tokio::task::yield_now().await;
    session.append_records(vec![record("second")]).await.unwrap();
    let event = tokio::time::timeout(Duration::from_millis(200), live.next()).await
        .expect("typed record committed but idle driver never woke").unwrap();
    assert_eq!(event.event_type, "second");
}

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
            if event.event_type == "new" { return; }
        }
        panic!("stream ended without new event");
    }).await;
    assert!(result.is_ok(), "old in-flight delivery advances reset cursor and skips new seq 1");
}
#[test]
fn dynamic_tool_context_preserves_store_cell() {
    let session = Arc::new(ListenableInMemorySession::new());
    let cell = Arc::new(StoreSessionCell::new(session));
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
    let context = RuntimeExecutionContext::new(CancellationToken::new(), "tool".into(), tx, "parent".into())
        .with_store_session(cell);
    assert!(context.store_session_cell().is_some());
    let dynamic: &dyn ToolExecutionContext = &context;
    assert!(dynamic.store_session().is_some());
    assert!(dynamic.store_session_cell().is_some(), "agent_spawn uses dyn ToolExecutionContext and must receive the configured cell");
}
#[tokio::test]
async fn subscription_after_clear_receives_new_live_event() {
    let session = ListenableInMemorySession::new();
    session.append(SessionEvent::new_runtime("before-clear")).await.unwrap();
    session.clear().await.unwrap();
    let mut stream = session.subscribe_events(0).await;
    session.append(SessionEvent::new_runtime("after-clear")).await.unwrap();
    let event = tokio::time::timeout(Duration::from_millis(100), stream.next()).await
        .expect("new subscriber after clear lost the next committed event").unwrap();
    assert_eq!(event.event_type, "after-clear");
}
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
    let finished = tokio::time::timeout(Duration::from_millis(100), &mut append).await.is_ok();
    listener.release.add_permits(1);
    if !finished { append.await.unwrap(); }
    assert!(finished, "the first append remains blocked on the arbitrary listener callback");
}
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
            session.append(SessionEvent::new_runtime("committed-before-abort")).await.unwrap();
            caller_liveness.acquire().await.unwrap().forget();
        })
    };
    listener.entered.acquire().await.unwrap().forget();
    append.abort();
    assert!(append.await.unwrap_err().is_cancelled(), "must actually cancel during delivery");
    listener.release.add_permits(1);
    let event = tokio::time::timeout(Duration::from_millis(100), live.next()).await
        .expect("already subscribed consumer permanently lost the committed event after append cancellation").unwrap();
    assert_eq!(event.event_type, "committed-before-abort");
}
