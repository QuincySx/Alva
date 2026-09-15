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
