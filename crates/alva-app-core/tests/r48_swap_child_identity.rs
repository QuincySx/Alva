// INPUT: BaseAgent + SubAgentPlugin spawn machinery, gated model, StoreSessionCell swap
// OUTPUT: R48 combination evidence — a child RUNNING through a root swap keeps its
//         identity, the swap lands afterwards, and the next root turn uses the new session
// POS:    Session-switch × recursion combination test (R48) — separate from the
//         adapter's repository-recursion test and from the app-core swap gate test,
//         neither of which exercises a live child across a swap.

use alva_app_core::extension::{SubAgentPlugin, ToolLockRegistryPlugin};
use alva_kernel_abi::{
    LanguageModel, Message, ModelConfig, StreamEvent,
};
use alva_test::{
    assertions::collect_events,
    fixtures::{make_assistant_message, make_tool_call_message},
};
use futures::stream::StreamExt as _;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;
use tokio::sync::Semaphore;

/// Model that scripts the parent→child→parent conversation and parks the
/// CHILD's LLM call on a gate, so the test can observe a provably in-flight
/// child turn while it performs a root swap.
struct GatedSpawnModel {
    inner: alva_test::mock_provider::MockLanguageModel,
    calls: AtomicUsize,
    child_entered: Arc<Semaphore>,
    child_release: Arc<Semaphore>,
}

impl GatedSpawnModel {
    fn new() -> (Arc<Self>, Arc<Semaphore>, Arc<Semaphore>) {
        let inner = alva_test::mock_provider::MockLanguageModel::new()
            // 1. parent: spawn a worker.
            .with_response(make_tool_call_message(
                "agent",
                serde_json::json!({"task": "leaf work", "role": "worker"}),
            ))
            // 2. child's answer (delivered after the gate releases).
            .with_response(make_assistant_message("child done"))
            // 3. parent's answer after the child returned.
            .with_response(make_assistant_message("parent done"));
        let child_entered = Arc::new(Semaphore::new(0));
        let child_release = Arc::new(Semaphore::new(0));
        (
            Arc::new(Self {
                inner,
                calls: AtomicUsize::new(0),
                child_entered: child_entered.clone(),
                child_release: child_release.clone(),
            }),
            child_entered,
            child_release,
        )
    }
}

#[async_trait::async_trait]
impl LanguageModel for GatedSpawnModel {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn alva_kernel_abi::Tool],
        config: &ModelConfig,
    ) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
        self.inner.complete(messages, tools, config).await
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn alva_kernel_abi::Tool],
        config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        let call = self.calls.fetch_add(1, Ordering::SeqCst);
        let base: std::pin::Pin<
            Box<dyn futures::Stream<Item = StreamEvent> + Send>,
        > = self.inner.stream(messages, tools, config);
        if call == 1 {
            // The child's LLM call: announce the child is mid-turn, then park
            // until the test releases it — across the whole root swap.
            let entered = self.child_entered.clone();
            let release = self.child_release.clone();
            let gate = futures::stream::once(async move {
                entered.add_permits(1);
                let _ = release.acquire().await;
                StreamEvent::Start
            });
            let tail: std::pin::Pin<
                Box<dyn futures::Stream<Item = StreamEvent> + Send>,
            > = Box::pin(
                base.skip(1), // drop the inner Start we already yielded
            );
            Box::pin(gate.chain(tail))
        } else {
            base
        }
    }

    fn model_id(&self) -> &str {
        "gated-spawn-model"
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_swap_during_a_running_child_preserves_child_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (model, child_entered, child_release) = GatedSpawnModel::new();
    let registry = Arc::new(alva_agent_core::ToolLockRegistry::new());
    let agent = Arc::new(
        alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .plugin(Box::new(ToolLockRegistryPlugin::with_registry(registry)))
            .plugin(Box::new(SubAgentPlugin::new(3, Duration::from_secs(10))))
            .max_iterations(6)
            .build(model)
            .await
            .unwrap(),
    );

    // The root session the child will be linked to.
    let original = agent.session_handle().await;
    let original_id = original.session_id().to_string();

    // Start the parent turn (it spawns the child, whose LLM call parks).
    let turn = {
        let agent = agent.clone();
        tokio::spawn(async move {
            collect_events(agent.prompt_text("spawn a worker").await).await
        })
    };
    tokio::time::timeout(Duration::from_secs(5), child_entered.acquire())
        .await
        .expect("child turn must start")
        .unwrap()
        .forget();

    // Child is provably running. Swap the ROOT session now — the child owns
    // its own agent state, so the swap must not disturb it; the swap itself
    // cannot land while the parent turn owns the root state, so run it
    // concurrently and expect it to complete only after the turn unwinds.
    let session_b: Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
        alva_agent_core::agent_session::InMemoryAgentSession::with_id("session-b".into()),
    );
    let session_b_id = session_b.session_id().to_string();
    let swap_task = {
        let agent = agent.clone();
        tokio::spawn(async move { agent.swap_session(session_b).await })
    };

    // Release the child; the parent finishes; the swap then lands.
    child_release.add_permits(1);
    let events = tokio::time::timeout(Duration::from_secs(10), turn)
        .await
        .expect("parent turn must finish")
        .expect("turn task must not panic");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "parent must finish: {events:?}"
    );
    let swapped = tokio::time::timeout(Duration::from_secs(5), swap_task)
        .await
        .expect("swap must land after the turn releases state")
        .expect("swap task must not panic");
    assert!(swapped, "swap_session must report a completed swap");

    // The next root turn runs on the NEW session.
    let current = agent.session_handle().await;
    assert_eq!(
        current.session_id(),
        session_b_id,
        "the swapped-in session must own subsequent root turns"
    );

    // The child's identity was fixed at spawn time: the ORIGINAL root
    // session carries the child link; the swapped-in session carries none.
    let original_links = original.records(0).await;
    let links_in_original: Vec<&alva_kernel_abi::SessionRecord> = original_links
        .iter()
        .filter(|record| {
            matches!(
                &record.payload,
                alva_kernel_abi::SessionRecordPayload::Execution(
                    alva_kernel_abi::ExecutionRecord::ChildSessionLinked { .. }
                )
            )
        })
        .collect();
    assert_eq!(
        links_in_original.len(),
        1,
        "the running child must be linked to the ORIGINAL root session, got {links_in_original:?}"
    );
    let child_session_id = match &links_in_original[0].payload {
        alva_kernel_abi::SessionRecordPayload::Execution(
            alva_kernel_abi::ExecutionRecord::ChildSessionLinked {
                child_session_id, ..
            },
        ) => child_session_id.clone(),
        other => unreachable!("filtered above: {other:?}"),
    };
    assert_ne!(
        child_session_id, original_id,
        "the child must have its own session identity"
    );
    let b_records = agent.session_handle().await.records(0).await;
    assert!(
        b_records
            .iter()
            .all(|record| !matches!(
                &record.payload,
                alva_kernel_abi::SessionRecordPayload::Execution(
                    alva_kernel_abi::ExecutionRecord::ChildSessionLinked { .. }
                )
            )),
        "the swapped-in session must not inherit the child link: {b_records:?}"
    );
}
