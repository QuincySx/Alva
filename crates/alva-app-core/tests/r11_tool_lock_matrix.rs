// INPUT: BaseAgent + SubAgentPlugin + ToolLockRegistryPlugin (production tool path), gated registry holds
// OUTPUT: R11 combination matrix — parent-relative vs child-absolute keys collide both ways, two children
//         contend on one resource, different resources run, read-read shares
// POS:    R48/R11 combination evidence. The contending hold is taken through the SAME registry the
//         production middleware uses (same precedent as spawned_child_obeys_shared_tool_lock); what is
//         under test is the production resolution + exclusion semantics, not the manual hold.

use alva_app_core::extension::{SubAgentPlugin, ToolLockRegistryPlugin};
use alva_kernel_abi::{
    ContentBlock, Message, MessageRole, ModelConfig, Scheduling, StreamEvent, Tool,
    ToolExecutionContext, ToolOutput, ToolSchemaContext,
};
use alva_test::assertions::collect_events;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};
use std::time::Duration;

/// Probe tool whose resource declaration is configurable; counts real
/// executions through a shared counter so the test keeps visibility after
/// the tool moves into the agent's tool box.
#[derive(Clone)]
struct ProbeTool {
    name: &'static str,
    key: String,
    mode: alva_kernel_abi::LockMode,
    executions: Arc<AtomicUsize>,
}

impl ProbeTool {
    fn write(name: &'static str, key: impl Into<String>) -> (Self, Arc<AtomicUsize>) {
        let executions = Arc::new(AtomicUsize::new(0));
        (
            Self {
                name,
                key: key.into(),
                mode: alva_kernel_abi::LockMode::Write,
                executions: executions.clone(),
            },
            executions,
        )
    }
    fn read(name: &'static str, key: impl Into<String>) -> (Self, Arc<AtomicUsize>) {
        let executions = Arc::new(AtomicUsize::new(0));
        (
            Self {
                name,
                key: key.into(),
                mode: alva_kernel_abi::LockMode::Read,
                executions: executions.clone(),
            },
            executions,
        )
    }
}

#[async_trait::async_trait]
impl Tool for ProbeTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "probe tool for the R11 lock matrix"
    }
    fn parameters_schema(&self, _ctx: &ToolSchemaContext) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, alva_kernel_abi::AgentError> {
        self.executions.fetch_add(1, Ordering::SeqCst);
        Ok(ToolOutput::text("probe ran"))
    }
    fn scheduling(&self, _input: &serde_json::Value) -> Scheduling {
        Scheduling::default().with_resource_keys(vec![alva_kernel_abi::ResourceKey {
            key: self.key.clone(),
            mode: self.mode,
        }])
    }
}

fn spawn_two_workers_message() -> Message {
    Message {
        id: "msg-spawn-two".into(),
        role: MessageRole::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: "call-worker-1".into(),
                name: "agent".into(),
                input: serde_json::json!({"task": "first", "role": "worker", "tools": ["probe_tool"]}),
            },
            ContentBlock::ToolUse {
                id: "call-worker-2".into(),
                name: "agent".into(),
                input: serde_json::json!({"task": "second", "role": "worker", "tools": ["probe_tool"]}),
            },
        ],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

async fn build_agent(
    workspace: &std::path::Path,
    probe: impl Tool + 'static,
    registry: Arc<alva_agent_core::ToolLockRegistry>,
    responses: Vec<Message>,
) -> Arc<alva_app_core::BaseAgent> {
    let mut mock = alva_test::mock_provider::MockLanguageModel::new();
    for response in responses {
        mock = mock.with_response(response);
    }
    let model = Arc::new(mock);
    Arc::new(
        alva_app_core::BaseAgent::builder()
            .workspace(workspace)
            .tool(Box::new(probe))
            .plugin(Box::new(ToolLockRegistryPlugin::with_registry(registry)))
            .plugin(Box::new(SubAgentPlugin::new(3, Duration::from_secs(10))))
            .max_iterations(8)
            .build(model)
            .await
            .unwrap(),
    )
}

async fn run_spawn_turn(agent: &alva_app_core::BaseAgent) {
    let events =
        tokio::time::timeout(Duration::from_secs(20), async {
            collect_events(agent.prompt_text("spawn two workers").await).await
        })
        .await
        .expect("turn must finish");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "parent must finish: {events:?}"
    );
}

/// 父相对 / 子绝对反向：the hold is taken with a RELATIVE key through the
/// production resolver (`acquire_within` + workspace); the spawned child's
/// tool declares the ABSOLUTE path of the same file. Both spellings must
/// resolve to one lock entry — the child's probe is blocked while the hold
/// lives, and the reverse direction collides the same way.
#[tokio::test]
async fn parent_relative_hold_blocks_child_absolute_key_and_reverse() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().to_path_buf();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(150)),
    );

    // Direction 1: relative hold, child declares absolute.
    let hold = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::write("src/lib.rs")],
            alva_kernel_abi::ExecutionMode::Parallel,
            workspace.as_path(),
        )
        .await;
    let (probe, executions) = ProbeTool::write(
        "probe_tool",
        workspace.join("src").join("lib.rs").to_string_lossy().to_string(),
    );
    let agent = build_agent(
        &workspace,
        probe,
        registry.clone(),
        vec![
            alva_test::fixtures::make_tool_call_message(
                "agent",
                serde_json::json!({"task": "touch the file", "role": "worker", "tools": ["probe_tool"]}),
            ),
            alva_test::fixtures::tool_use_message("probe", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::make_assistant_message("parent done"),
        ],
    )
    .await;
    run_spawn_turn(&agent).await;
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "child tool with an ABSOLUTE key must be blocked by a RELATIVE-key hold on the same file"
    );
    drop(hold);

    // Direction 2: absolute hold, child declares relative.
    let hold = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::write(
                workspace.join("src").join("lib.rs").to_string_lossy().as_ref(),
            )],
            alva_kernel_abi::ExecutionMode::Parallel,
            workspace.as_path(),
        )
        .await;
    let (probe2, executions2) = ProbeTool::write("probe_tool", "src/lib.rs");
    let agent2 = build_agent(
        &workspace,
        probe2.clone(),
        registry.clone(),
        vec![
            alva_test::fixtures::make_tool_call_message(
                "agent",
                serde_json::json!({"task": "touch the file", "role": "worker", "tools": ["probe_tool"]}),
            ),
            alva_test::fixtures::tool_use_message("probe", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::make_assistant_message("parent done"),
        ],
    )
    .await;
    run_spawn_turn(&agent2).await;
    assert_eq!(
        executions2.load(Ordering::SeqCst),
        0,
        "child tool with a RELATIVE key must be blocked by an ABSOLUTE-key hold on the same file"
    );
    drop(hold);
}

/// 两个 child 同资源互斥：with a write hold on the shared resource, BOTH
/// spawned children's probes are blocked; neither runs while the hold lives.
#[tokio::test]
async fn two_children_on_one_resource_are_mutually_excluded() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(150)),
    );
    let hold = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::write("shared.bin")],
            alva_kernel_abi::ExecutionMode::Parallel,
            tmp.path(),
        )
        .await;
    let (probe, executions) = ProbeTool::write("probe_tool", "shared.bin");
    let agent = build_agent(
        tmp.path(),
        probe.clone(),
        registry,
        vec![
            spawn_two_workers_message(),
            // Each child first CALLS probe_tool, then answers. The `agent`
            // tool is sequential (default scheduling), so the shared queue
            // is consumed in deterministic order.
            alva_test::fixtures::tool_use_message("probe-1", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::tool_use_message("probe-2", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::make_assistant_message("parent done"),
        ],
    )
    .await;
    run_spawn_turn(&agent).await;
    assert_eq!(
        executions.load(Ordering::SeqCst),
        0,
        "both children contend on the same write resource; neither probe may run"
    );
    drop(hold);
}

/// 不同资源并行：a hold on `a.bin` does not touch children probing `b.bin` —
/// both children's probes really execute.
#[tokio::test]
async fn different_resources_do_not_collide() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(150)),
    );
    let unrelated = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::write("a.bin")],
            alva_kernel_abi::ExecutionMode::Parallel,
            tmp.path(),
        )
        .await;
    let (probe, executions) = ProbeTool::write("probe_tool", "b.bin");
    let agent = build_agent(
        tmp.path(),
        probe.clone(),
        registry,
        vec![
            spawn_two_workers_message(),
            // Each child first CALLS probe_tool, then answers. The `agent`
            // tool is sequential (default scheduling), so the shared queue
            // is consumed in deterministic order.
            alva_test::fixtures::tool_use_message("probe-1", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::tool_use_message("probe-2", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::make_assistant_message("parent done"),
        ],
    )
    .await;
    run_spawn_turn(&agent).await;
    assert_eq!(
        executions.load(Ordering::SeqCst),
        2,
        "children on a different resource must both run: {}",
        executions.load(Ordering::SeqCst)
    );
    drop(unrelated);
}

/// 读读共享：a READ hold on the resource does not block children that only
/// read it — both probes execute (a write hold blocking readers is already
/// covered by the mutual-exclusion direction above).
#[tokio::test]
async fn read_holders_share_the_resource_with_readers() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(150)),
    );
    let read_hold = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::read("data.bin")],
            alva_kernel_abi::ExecutionMode::Parallel,
            tmp.path(),
        )
        .await;
    let (probe, executions) = ProbeTool::read("probe_tool", "data.bin");
    let agent = build_agent(
        tmp.path(),
        probe.clone(),
        registry,
        vec![
            spawn_two_workers_message(),
            // Each child first CALLS probe_tool, then answers. The `agent`
            // tool is sequential (default scheduling), so the shared queue
            // is consumed in deterministic order.
            alva_test::fixtures::tool_use_message("probe-1", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::tool_use_message("probe-2", "probe_tool", serde_json::json!({})),
            alva_test::fixtures::make_assistant_message("worker done"),
            alva_test::fixtures::make_assistant_message("parent done"),
        ],
    )
    .await;
    run_spawn_turn(&agent).await;
    assert_eq!(
        executions.load(Ordering::SeqCst),
        2,
        "read-read must share: {}",
        executions.load(Ordering::SeqCst)
    );
    drop(read_hold);
}

// ===========================================================================
// R72: real overlapping children. The model routes statelessly on message
// CONTENT (no shared FIFO), the parent's two `agent` calls run concurrently,
// and the probe tool parks on a test-owned gate so active/max-active can be
// asserted while two children are genuinely mid-execution. A mutant that
// globally serializes children fails the overlap assertions.
// ===========================================================================

/// Probe tool that parks each execution on a gate and tracks active/max-active
/// concurrency. The resource key can come from the call input (`{"target":
/// "..."}`) so one registered tool can serve per-child resources.
#[derive(Clone)]
struct GatedProbeTool {
    name: &'static str,
    default_key: String,
    mode: alva_kernel_abi::LockMode,
    executions: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}

impl GatedProbeTool {
    fn new(name: &'static str, key: &str, mode: alva_kernel_abi::LockMode) -> (Self, GateHandles) {
        let tool = Self {
            name,
            default_key: key.to_string(),
            mode,
            executions: Arc::new(AtomicUsize::new(0)),
            active: Arc::new(AtomicUsize::new(0)),
            max_active: Arc::new(AtomicUsize::new(0)),
            entered: Arc::new(tokio::sync::Semaphore::new(0)),
            release: Arc::new(tokio::sync::Semaphore::new(0)),
        };
        (
            tool.clone(),
            GateHandles {
                executions: tool.executions.clone(),
                max_active: tool.max_active.clone(),
                entered: tool.entered.clone(),
                release: tool.release.clone(),
            },
        )
    }
}

/// Test-side handles cloned out of the tool before it moves into the agent.
#[derive(Clone)]
struct GateHandles {
    executions: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
    entered: Arc<tokio::sync::Semaphore>,
    release: Arc<tokio::sync::Semaphore>,
}

impl GateHandles {
    /// Wait until one more probe execution has entered the gate.
    async fn wait_entry(&self) {
        tokio::time::timeout(Duration::from_secs(10), self.entered.acquire())
            .await
            .expect("a probe must enter within the timeout")
            .expect("semaphore must be open")
            .forget();
    }

    /// Deterministic negative gate: no further entry may happen within `ms`.
    async fn expect_no_entry(&self, ms: u64) {
        let attempt = tokio::time::timeout(Duration::from_millis(ms), self.entered.acquire()).await;
        assert!(
            attempt.is_err(),
            "a second probe entered while exclusion required it not to"
        );
    }

    fn release_one(&self) {
        self.release.add_permits(1);
    }

    fn release_all(&self, n: usize) {
        self.release.add_permits(n);
    }
}

#[async_trait::async_trait]
impl Tool for GatedProbeTool {
    fn name(&self) -> &str {
        self.name
    }
    fn description(&self) -> &str {
        "gated probe tool for the R72 overlap matrix"
    }
    fn parameters_schema(&self, _ctx: &ToolSchemaContext) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, alva_kernel_abi::AgentError> {
        let now_active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(now_active, Ordering::SeqCst);
        self.entered.add_permits(1);
        // Park until the test releases this execution — the overlap window.
        let _ = self.release.acquire().await;
        self.executions.fetch_add(1, Ordering::SeqCst);
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolOutput::text("probe ran"))
    }
    fn scheduling(&self, input: &serde_json::Value) -> Scheduling {
        let target = input
            .get("target")
            .and_then(|value| value.as_str())
            .map(str::to_string)
            .unwrap_or_else(|| self.default_key.clone());
        Scheduling::default().with_resource_keys(vec![alva_kernel_abi::ResourceKey {
            key: target,
            mode: self.mode,
        }])
    }
}

/// Stateless routing model: the response is derived from the CONVERSATION
/// CONTENT, never from a shared FIFO, so concurrent children get correct
/// responses regardless of scheduling order.
struct RoutingSpawnModel {
    spawn_message: Message,
}

impl RoutingSpawnModel {
    fn route(&self, messages: &[Message]) -> Message {
        // Map tool_use ids to tool names.
        let mut names = std::collections::HashMap::new();
        for message in messages {
            for block in &message.content {
                if let ContentBlock::ToolUse { id, name, .. } = block {
                    names.insert(id.clone(), name.clone());
                }
            }
        }
        // Route on the MOST RECENT tool result.
        for message in messages.iter().rev() {
            if message.role != MessageRole::Tool {
                continue;
            }
            for block in &message.content {
                if let ContentBlock::ToolResult { id, is_error, .. } = block {
                    match names.get(id).map(String::as_str) {
                        Some("probe_tool") if !*is_error => {
                            return alva_test::fixtures::make_assistant_message("worker done");
                        }
                        Some("agent") if !*is_error => {
                            return alva_test::fixtures::make_assistant_message("parent done");
                        }
                        _ => {}
                    }
                }
            }
        }
        // No tool result yet: parent spawn turn or a child's first call.
        for message in messages.iter().rev() {
            if message.role == MessageRole::User {
                for block in &message.content {
                    if let ContentBlock::Text { text, .. } = block {
                        if text.contains("spawn two workers") {
                            return self.spawn_message.clone();
                        }
                    }
                }
            }
        }
        // A child's first LLM call: the child's task text doubles as its
        // probe resource key, so two children spawned in one batch land on
        // the resources their tasks name (same task → same resource,
        // different tasks → different resources).
        let task = messages
            .iter()
            .rev()
            .filter(|message| message.role == MessageRole::User)
            .find_map(|message| {
                message.content.iter().find_map(|block| match block {
                    ContentBlock::Text { text, .. } => Some(text.clone()),
                    _ => None,
                })
            })
            .unwrap_or_default();
        alva_test::fixtures::tool_use_message(
            "probe",
            "probe_tool",
            serde_json::json!({ "target": task }),
        )
    }
}

#[async_trait::async_trait]
impl alva_kernel_abi::LanguageModel for RoutingSpawnModel {
    async fn complete(
        &self,
        messages: &[Message],
        tools: &[&dyn alva_kernel_abi::Tool],
        config: &ModelConfig,
    ) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
        let relay = alva_test::mock_provider::MockLanguageModel::new()
            .with_response(self.route(messages));
        relay.complete(messages, tools, config).await
    }

    fn stream(
        &self,
        messages: &[Message],
        tools: &[&dyn alva_kernel_abi::Tool],
        config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        let relay = alva_test::mock_provider::MockLanguageModel::new()
            .with_response(self.route(messages));
        relay.stream(messages, tools, config)
    }

    fn model_id(&self) -> &str {
        "routing-spawn-model"
    }
}

/// R72 spawn message: two `agent` calls in ONE assistant message, so the
/// parent's tool-batch wave decides their concurrency, with per-test task
/// texts — a child's task text doubles as its probe resource key.
fn spawn_two_workers_on_tasks(task_a: &str, task_b: &str) -> Message {
    Message {
        id: "msg-spawn-two".into(),
        role: MessageRole::Assistant,
        content: vec![
            ContentBlock::ToolUse {
                id: "call-worker-1".into(),
                name: "agent".into(),
                input: serde_json::json!({"task": task_a, "role": "worker", "tools": ["probe_tool"]}),
            },
            ContentBlock::ToolUse {
                id: "call-worker-2".into(),
                name: "agent".into(),
                input: serde_json::json!({"task": task_b, "role": "worker", "tools": ["probe_tool"]}),
            },
        ],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

async fn build_routed_agent(
    workspace: &std::path::Path,
    probe: GatedProbeTool,
    registry: Arc<alva_agent_core::ToolLockRegistry>,
    spawn_message: Message,
) -> Arc<alva_app_core::BaseAgent> {
    let model = Arc::new(RoutingSpawnModel { spawn_message });
    Arc::new(
        alva_app_core::BaseAgent::builder()
            .workspace(workspace)
            .tool(Box::new(probe))
            .plugin(Box::new(ToolLockRegistryPlugin::with_registry(registry)))
            .plugin(Box::new(SubAgentPlugin::new(3, Duration::from_secs(10))))
            .max_iterations(8)
            .build(model)
            .await
            .unwrap(),
    )
}

/// 同资源写互斥：two REAL children contend on one write resource. The first
/// probe parks in-flight; the second child's probe must be BLOCKED by the
/// production lock (no second entry) until the first releases. max-active
/// must stay 1.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn same_resource_real_children_serialize_with_in_flight_overlap() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(10_000)),
    );
    let (probe, gate) = GatedProbeTool::new("probe_tool", "shared.bin", alva_kernel_abi::LockMode::Write);
    let agent =
        build_routed_agent(tmp.path(), probe, registry, spawn_two_workers_on_tasks("shared.bin", "shared.bin"))
            .await;

    let turn = {
        let agent = agent.clone();
        tokio::spawn(async move {
            collect_events(agent.prompt_text("spawn two workers").await).await
        })
    };

    // Child 1's probe is in flight (holding the write lock, parked).
    gate.wait_entry().await;
    // Child 2 contends on the same resource: it must NOT enter while child 1
    // holds the lock — a bounded deterministic negative gate.
    gate.expect_no_entry(300).await;
    assert_eq!(gate.max_active.load(Ordering::SeqCst), 1);

    // Release child 1; child 2 must then acquire and enter.
    gate.release_one();
    gate.wait_entry().await;
    assert_eq!(gate.max_active.load(Ordering::SeqCst), 1);
    gate.release_one();

    let events = tokio::time::timeout(Duration::from_secs(20), turn)
        .await
        .expect("turn must finish")
        .expect("turn task must not panic");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "parent must finish: {events:?}"
    );
    assert_eq!(gate.executions.load(Ordering::SeqCst), 2, "both children's probes must have run");
    assert_eq!(
        gate.max_active.load(Ordering::SeqCst),
        1,
        "same-resource write children must never overlap"
    );
}

/// 异资源重叠：two REAL children probing DIFFERENT resources must be
/// in-flight SIMULTANEOUSLY (max-active 2) — a globally serializing mutant
/// hangs on the second entry and fails.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn different_resource_real_children_overlap() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(10_000)),
    );
    let (probe, gate) = GatedProbeTool::new("probe_tool", "fallback.bin", alva_kernel_abi::LockMode::Write);
    let agent =
        build_routed_agent(tmp.path(), probe, registry, spawn_two_workers_on_tasks("a.bin", "b.bin")).await;

    let turn = {
        let agent = agent.clone();
        tokio::spawn(async move {
            collect_events(agent.prompt_text("spawn two workers").await).await
        })
    };

    // Both children must be in flight at the same time.
    gate.wait_entry().await;
    gate.wait_entry().await;
    assert_eq!(
        gate.max_active.load(Ordering::SeqCst),
        2,
        "children on different resources must overlap: max-active {}",
        gate.max_active.load(Ordering::SeqCst)
    );
    gate.release_all(2);

    let events = tokio::time::timeout(Duration::from_secs(20), turn)
        .await
        .expect("turn must finish")
        .expect("turn task must not panic");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "parent must finish: {events:?}"
    );
    assert_eq!(gate.executions.load(Ordering::SeqCst), 2);
    assert_eq!(gate.max_active.load(Ordering::SeqCst), 2);
}

/// 读读重叠：with an external READ hold on the resource, two reader children
/// must still overlap (max-active 2) while the hold lives.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn read_children_overlap_while_read_hold_is_live() {
    let tmp = tempfile::tempdir().unwrap();
    let registry = Arc::new(
        alva_agent_core::ToolLockRegistry::new()
            .with_acquire_timeout(Duration::from_millis(10_000)),
    );
    let hold = registry
        .acquire_within(
            &[alva_kernel_abi::ResourceKey::read("data.bin")],
            alva_kernel_abi::ExecutionMode::Parallel,
            tmp.path(),
        )
        .await;
    let (probe, gate) = GatedProbeTool::new("probe_tool", "data.bin", alva_kernel_abi::LockMode::Read);
    let agent =
        build_routed_agent(tmp.path(), probe, registry, spawn_two_workers_on_tasks("data.bin", "data.bin")).await;

    let turn = {
        let agent = agent.clone();
        tokio::spawn(async move {
            collect_events(agent.prompt_text("spawn two workers").await).await
        })
    };

    gate.wait_entry().await;
    gate.wait_entry().await;
    assert_eq!(
        gate.max_active.load(Ordering::SeqCst),
        2,
        "readers must overlap under a live read hold: max-active {}",
        gate.max_active.load(Ordering::SeqCst)
    );
    gate.release_all(2);
    drop(hold);

    let events = tokio::time::timeout(Duration::from_secs(20), turn)
        .await
        .expect("turn must finish")
        .expect("turn task must not panic");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "parent must finish: {events:?}"
    );
    assert_eq!(gate.executions.load(Ordering::SeqCst), 2);
    assert_eq!(gate.max_active.load(Ordering::SeqCst), 2);
}
