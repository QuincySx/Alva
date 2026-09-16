// INPUT: BaseAgent + SubAgentPlugin spawn machinery, gated model, StoreSessionCell swap
// OUTPUT: R48 combination evidence — a child RUNNING through a root swap keeps its
//         identity, the swap lands afterwards, and the next root turn uses the new session
// POS:    Session-switch × recursion combination test (R48) — separate from the
//         adapter's repository-recursion test and from the app-core swap gate test,
//         neither of which exercises a live child across a swap.

use alva_app_core::extension::{SubAgentPlugin, ToolLockRegistryPlugin};
use alva_kernel_abi::{
    ConversationLog, LanguageModel, Message, ModelConfig, StreamEvent,
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
            .with_response(make_assistant_message("parent done"))
            // 4. the SECOND root turn, run on the swapped-in session.
            .with_response(make_assistant_message("second turn done"));
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


/// R71: root session wrapper that CAPTURES the child sessions created through
/// the spawn machinery, so the test can query the child's own identity and
/// follow-up records instead of only the parent-side link.
struct ChildCaptureSession {
    inner: Arc<alva_agent_core::agent_session::ListenableInMemorySession>,
    children: Arc<std::sync::Mutex<Vec<std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>>>>,
}

impl ChildCaptureSession {
    fn new() -> (
        Arc<Self>,
        Arc<std::sync::Mutex<Vec<std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>>>>,
    ) {
        let children: Arc<std::sync::Mutex<Vec<std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        (
            Arc::new(Self {
                inner: Arc::new(alva_agent_core::agent_session::ListenableInMemorySession::new()),
                children: children.clone(),
            }),
            children,
        )
    }
}

#[async_trait::async_trait]
impl alva_kernel_abi::ConversationLog for ChildCaptureSession {
    fn session_id(&self) -> &str {
        self.inner.session_id()
    }
    async fn messages(&self) -> Result<Vec<alva_kernel_abi::AgentMessage>, alva_kernel_abi::SessionError> {
        self.inner.messages().await
    }
    async fn recent_messages(
        &self,
        n: usize,
    ) -> Result<Vec<alva_kernel_abi::AgentMessage>, alva_kernel_abi::SessionError> {
        self.inner.recent_messages(n).await
    }
    async fn append_message(
        &self,
        msg: alva_kernel_abi::AgentMessage,
        parent_uuid: Option<String>,
    ) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.append_message(msg, parent_uuid).await
    }
    async fn append(&self, event: alva_kernel_abi::SessionEvent) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.append(event).await
    }
    async fn append_records(
        &self,
        records: Vec<alva_kernel_abi::SessionRecord>,
    ) -> Result<alva_kernel_abi::AppendReceipt, alva_kernel_abi::SessionError> {
        self.inner.append_records(records).await
    }
    fn supports_atomic_records(&self) -> bool {
        self.inner.supports_atomic_records()
    }
    async fn flush(&self) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.flush().await
    }
}

#[async_trait::async_trait]
impl alva_kernel_abi::SessionStore for ChildCaptureSession {
    fn parent_session_id(&self) -> Option<&str> {
        self.inner.parent_session_id()
    }
    async fn create_child_session(
        &self,
        parent_run_id: &str,
        spawn_tool_call_id: &str,
    ) -> Result<Option<std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>>, alva_kernel_abi::SessionError> {
        let child: std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession> =
            std::sync::Arc::new(alva_agent_core::agent_session::ListenableInMemorySession::with_parent(
                self.inner.session_id(),
            ));
        self.children.lock().unwrap().push(child.clone());
        let link = alva_kernel_abi::SessionRecord::execution(
            "child_session_linked",
            alva_kernel_abi::EventEmitter::runtime(),
            alva_kernel_abi::ExecutionRecord::ChildSessionLinked {
                child_session_id: child.session_id().to_string(),
                parent_run_id: parent_run_id.to_string(),
                spawn_tool_call_id: spawn_tool_call_id.to_string(),
            },
        );
        self.inner.append_records(vec![link]).await?;
        Ok(Some(child))
    }
    async fn query(&self, filter: &alva_kernel_abi::EventQuery) -> Vec<alva_kernel_abi::EventMatch> {
        self.inner.query(filter).await
    }
    async fn records(&self, from_seq: u64) -> Vec<alva_kernel_abi::SessionRecord> {
        // 委托 typed 投影——默认实现经事件重建会把 ChildSessionLinked
        // 降级为通用形态。
        self.inner.records(from_seq).await
    }
    async fn count(&self, filter: &alva_kernel_abi::EventQuery) -> usize {
        self.inner.count(filter).await
    }
    async fn rollback_after(&self, uuid: &str) -> usize {
        self.inner.rollback_after(uuid).await
    }
    async fn save_snapshot(&self, data: &[u8]) {
        self.inner.save_snapshot(data).await
    }
    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.inner.load_snapshot().await
    }
    async fn restore(&self) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.restore().await
    }
    async fn close(&self) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.close().await
    }
    async fn clear(&self) -> Result<(), alva_kernel_abi::SessionError> {
        self.inner.clear().await
    }
    async fn compact_messages(&self, replacement: Vec<alva_kernel_abi::AgentMessage>) {
        self.inner.compact_messages(replacement).await
    }
}

impl alva_kernel_abi::agent_session::AgentSession for ChildCaptureSession {}


/// Flatten an AgentMessage list into comparable text.
fn agent_message_text(messages: &[alva_kernel_abi::AgentMessage]) -> String {
    messages
        .iter()
        .map(|message| match message {
            alva_kernel_abi::AgentMessage::Standard(message) => message
                .content
                .iter()
                .map(|block| match block {
                    alva_kernel_abi::ContentBlock::Text { text, .. } => text.clone(),
                    other => format!("{other:?}"),
                })
                .collect::<Vec<_>>()
                .join(" "),
            other => format!("{other:?}"),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn root_swap_during_a_running_child_preserves_child_identity() {
    let tmp = tempfile::tempdir().unwrap();
    let (model, child_entered, child_release) = GatedSpawnModel::new();
    let registry = Arc::new(alva_agent_core::ToolLockRegistry::new());
    let (root_session, captured_children) = ChildCaptureSession::new();
    let agent = Arc::new(
        alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .session(root_session)
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
    let (swap_started_tx, swap_started_rx) = tokio::sync::oneshot::channel::<()>();
    let mut swap_task = {
        let agent = agent.clone();
        tokio::spawn(async move {
            // R71: 握手——swap future 已开始执行。父轮此刻仍持有 state 锁
            // （child 尚未放行），swap 只能停在该锁上等待。
            let _ = swap_started_tx.send(());
            agent.swap_session(session_b).await
        })
    };
    tokio::time::timeout(Duration::from_secs(2), swap_started_rx)
        .await
        .expect("swap task must start")
        .expect("swap handshake must be received");
    // R71: 确定性负向 gate——child 未结束（父轮未释放状态）期间 swap
    // 不得落地；过早落地的坏实现会在此处让 timeout 返回 Ok。
    let premature = tokio::time::timeout(Duration::from_millis(200), &mut swap_task).await;
    assert!(
        premature.is_err(),
        "swap must not complete while the root turn is still running"
    );

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

    // R71: query the CHILD's own session — identity (parent id) and its
    // follow-up log, not just the parent-side link.
    let children = captured_children.lock().unwrap().clone();
    assert_eq!(children.len(), 1, "exactly one child must have been spawned");
    let child = children[0].clone();
    assert_eq!(
        child.parent_session_id(),
        Some(original_id.as_str()),
        "the child session must record the ORIGINAL root as its parent"
    );
    let child_messages = child.recent_messages(10).await.unwrap();
    let child_text = agent_message_text(&child_messages);
    assert!(
        child_text.contains("child done"),
        "the child session must carry the child's own answer: {child_text}"
    );

    // R71: the next root turn REALLY runs — and lands on the swapped-in
    // session, not the original one.
    let turn2 = {
        let agent = agent.clone();
        tokio::spawn(async move {
            collect_events(agent.prompt_text("second turn follow-up").await).await
        })
    };
    let events2 = tokio::time::timeout(Duration::from_secs(10), turn2)
        .await
        .expect("second root turn must finish")
        .expect("second turn task must not panic");
    assert!(
        events2
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "second root turn must finish cleanly: {events2:?}"
    );
    let b_messages = agent
        .session_handle()
        .await
        .recent_messages(10)
        .await
        .unwrap();
    let b_text = agent_message_text(&b_messages);
    assert!(
        b_text.contains("second turn follow-up"),
        "the post-swap turn must write to the swapped-in session: {b_text}"
    );
    let original_messages = original.recent_messages(10).await.unwrap();
    let original_text = agent_message_text(&original_messages);
    assert!(
        original_text.contains("spawn a worker"),
        "the original session must still carry the first turn: {original_text}"
    );
    assert!(
        !original_text.contains("second turn follow-up"),
        "the original session must NOT receive the post-swap turn"
    );
    assert!(
        !child_text.contains("second turn follow-up"),
        "the child session must NOT receive post-swap root traffic"
    );
}
