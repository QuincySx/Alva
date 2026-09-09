// INPUT:  production spawn path (BaseAgent + SubAgentPlugin + ToolLockRegistryPlugin),
//         ToolLockRegistry::acquire_bounded workspace resolution
// OUTPUT: R11 acceptance — child lock middleware resolves relative keys against the
//         child workspace identically to the parent's absolute keys.
// POS:    Parent holds a WRITE lock on `<workspace>/src/foo.rs` (absolute); the child
//         tool declares `src/foo.rs` (relative). With the child workspace wired, both
//         resolve to the same normalized lock key and the child must WAIT; read/read
//         and disjoint resources stay concurrent.

use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::time::Duration;

use alva_agent_core::ToolLockRegistry;
use alva_app_core::extension::{SubAgentPlugin, ToolLockRegistryPlugin};
use alva_kernel_abi::tool::execution::ToolExecutionContext;
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, ExecutionMode, LockMode, Message, ResourceKey,
    Scheduling, Tool, ToolOutput, ToolSchemaContext,
};
use alva_kernel_core::event::AgentEvent;
use alva_test::{
    assertions::collect_events,
    fixtures::{make_assistant_message, make_tool_call_message},
    mock_provider::MockLanguageModel,
};

/// Tool claiming a WRITE lock on one resource; records that it ran.
struct LockedTool {
    resource_key: String,
    ran: Arc<AtomicBool>,
}

#[async_trait::async_trait]
impl Tool for LockedTool {
    fn name(&self) -> &str {
        "locked_tool"
    }
    fn description(&self) -> &str {
        "records execution under a resource write lock"
    }
    fn parameters_schema(&self, _: &ToolSchemaContext) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    fn scheduling(&self, _: &serde_json::Value) -> Scheduling {
        Scheduling::default()
            .with_resource_keys(vec![ResourceKey::write(self.resource_key.clone())])
    }
    async fn execute(
        &self,
        _: serde_json::Value,
        _: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        self.ran.store(true, Ordering::SeqCst);
        Ok(ToolOutput::text("ran"))
    }
}

/// R11 acceptance (spawn path): an external holder keeps the ABSOLUTE lock
/// (`<workspace>/src/foo.rs`) while the spawned child's tool claims the
/// RELATIVE key (`src/foo.rs`). The child middleware must resolve the
/// relative key against the wired child workspace, so the child tool cannot
/// run until the holder releases; afterwards it runs normally.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn spawned_child_waits_on_absolute_holder_of_relative_resource() {
    let tmp = tempfile::tempdir().unwrap();
    let workspace = tmp.path().canonicalize().unwrap();
    let absolute_key = workspace.join("src/foo.rs").to_string_lossy().to_string();
    let ran = Arc::new(AtomicBool::new(false));

    // 100ms fuse: a blocked child surfaces as bounded lock errors, never a hang.
    let registry =
        Arc::new(ToolLockRegistry::new().with_acquire_timeout(Duration::from_millis(100)));

    // External holder keeps the absolute write lock for the first phase.
    let holder = registry
        .acquire(
            &[ResourceKey::write(absolute_key.clone())],
            ExecutionMode::SerialGlobal,
        )
        .await;

    let model = Arc::new(
        MockLanguageModel::new()
            .with_response(make_tool_call_message(
                "agent",
                serde_json::json!({"task": "use the granted tool", "role": "worker", "tools": ["locked_tool"]}),
            ))
            .with_response(make_tool_call_message("locked_tool", serde_json::json!({})))
            .with_response(make_assistant_message("child done"))
            .with_response(make_assistant_message("parent done")),
    );

    let agent = alva_app_core::BaseAgent::builder()
        .workspace(&workspace)
        .tool(Box::new(LockedTool {
            resource_key: "src/foo.rs".into(), // RELATIVE — child-side declaration
            ran: ran.clone(),
        }))
        .plugin(Box::new(ToolLockRegistryPlugin::with_registry(
            registry.clone(),
        )))
        .plugin(Box::new(SubAgentPlugin::new(3, Duration::from_secs(10))))
        .max_iterations(10)
        .build(model)
        .await
        .unwrap();

    let mut events = agent
        .prompt_text("spawn a worker that uses the locked tool")
        .await;
    tokio::time::timeout(Duration::from_millis(600), async {
        while let Ok(event) = events.try_recv() {
            if matches!(event, AgentEvent::AgentEnd { .. }) {
                panic!("child finished while the parent's absolute lock was still held");
            }
        }
    })
    .await
    .unwrap();
    assert!(
        !ran.load(Ordering::SeqCst),
        "child tool must not run while the absolute lock is held by the external holder"
    );

    // Release → the queued child tool runs on the next bounded acquire.
    drop(holder);
    let mut saw_end = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while tokio::time::Instant::now() < deadline {
        if let Ok(event) = events.try_recv() {
            if matches!(event, AgentEvent::AgentEnd { error: None }) {
                saw_end = true;
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(saw_end, "run must complete after release");
    assert!(
        ran.load(Ordering::SeqCst),
        "child tool must run after release"
    );
}

/// Registry-level pin of the resolution rule the middleware relies on:
/// with the workspace wired, relative `src/foo.rs` contends with absolute
/// `<workspace>/src/foo.rs` (write/write); read/read still shares.
#[tokio::test]
async fn registry_resolves_relative_keys_against_workspace_for_contention() {
    use std::path::Path;

    let registry =
        Arc::new(ToolLockRegistry::new().with_acquire_timeout(Duration::from_millis(100)));
    let workspace = Path::new("/ws");

    // Hold the ABSOLUTE write key.
    let holder = registry
        .acquire(
            &[ResourceKey::write("/ws/src/foo.rs")],
            ExecutionMode::Parallel,
        )
        .await;

    // Relative write must be blocked (same normalized key, write/write).
    let relative_write = [ResourceKey::write("src/foo.rs")];
    let blocked =
        registry.acquire_bounded(&relative_write, ExecutionMode::Parallel, Some(workspace));
    match tokio::time::timeout(Duration::from_millis(400), blocked).await {
        Err(_) => panic!("bounded acquire must not hang"),
        Ok(Err(_)) => {} // timed out: same lock, expected
        Ok(Ok(_)) => panic!("relative write must contend with absolute write"),
    }

    // A DISJOINT relative key is not blocked.
    let disjoint_key = [ResourceKey::write("src/bar.rs")];
    let disjoint =
        registry.acquire_bounded(&disjoint_key, ExecutionMode::Parallel, Some(workspace));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), disjoint)
            .await
            .is_ok(),
        "disjoint resource must acquire immediately"
    );

    drop(holder);

    // READ/read on the same resource still shares (multi-reader semantics).
    let read_lock = registry
        .acquire(
            &[ResourceKey::read("/ws/src/foo.rs")],
            ExecutionMode::Parallel,
        )
        .await;
    let read_key = [ResourceKey::read("src/foo.rs")];
    let shared_read = registry.acquire_bounded(&read_key, ExecutionMode::Parallel, Some(workspace));
    assert!(
        tokio::time::timeout(Duration::from_millis(400), shared_read)
            .await
            .is_ok(),
        "read/read on the same resolved resource must not block"
    );
    drop(read_lock);
}
