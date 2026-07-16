// INPUT:  alva_app_core, alva_test, alva_agent_security, alva_kernel_abi, tokio, tempfile, serde_json
// OUTPUT: sub-agent HITL gate integration tests
// POS:    Pins the child-middleware security invariant — a sub-agent's dangerous
//         tool calls must traverse the parent's approval gate (SecurityGuard on
//         the bus, wrapped into the child stack by agent_spawn).

use std::path::Path;
use std::sync::Arc;

use alva_agent_security::PermissionDecision;
use alva_app_core::extension::{PermissionPlugin, ShellPlugin, SubAgentPlugin};
use alva_app_core::{BaseAgent, PermissionMode};
use alva_test::assertions::collect_events;
use alva_test::fixtures::{make_assistant_message, make_tool_call_message};
use alva_test::mock_provider::MockLanguageModel;

/// Deterministic four-beat script: the child runs to completion inside the
/// parent's `agent` tool call, so the shared queue is consumed in strict
/// P1 → C1 → C2 → P2 order.
fn scripted_model(workspace: &Path) -> Arc<MockLanguageModel> {
    let marker = workspace.join("child-side-effect.txt");
    Arc::new(
        MockLanguageModel::new()
            // P1: parent spawns a child granted only execute_shell.
            .with_response(make_tool_call_message(
                "agent",
                serde_json::json!({
                    "task": "create the marker file",
                    "role": "worker",
                    "tools": ["execute_shell"],
                }),
            ))
            // C1: child immediately attempts a gated shell command.
            .with_response(make_tool_call_message(
                "execute_shell",
                serde_json::json!({ "command": format!("touch {}", marker.display()) }),
            ))
            // C2: child wraps up regardless of the tool outcome.
            .with_response(make_assistant_message("child done"))
            // P2: parent wraps up after the agent tool returns.
            .with_response(make_assistant_message("parent done")),
    )
}

async fn run_with_decision(decision: PermissionDecision) -> (tempfile::TempDir, usize) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let (approval_ext, mut approval_rx) = alva_app_core::extension::ApprovalPlugin::with_channel();
    let model = scripted_model(tmp.path());

    let agent = BaseAgent::builder()
        .workspace(tmp.path())
        .system_prompt("test harness")
        .plugin(Box::new(
            PermissionPlugin::new().with_initial(PermissionMode::Ask),
        ))
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(ShellPlugin))
        .plugin(Box::new(SubAgentPlugin::new(
            3,
            std::time::Duration::from_secs(60),
        )))
        .max_iterations(6)
        .build(model)
        .await
        .expect("agent build");

    // Resolve every approval request with the given decision, counting the
    // ones that reach the gate for execute_shell.
    let bus = agent.bus().clone();
    let counter = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let seen = counter.clone();
    tokio::spawn(async move {
        while let Some(req) = approval_rx.recv().await {
            if req.tool_name == "execute_shell" {
                seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            }
            if let Some(guard) = bus.get::<tokio::sync::Mutex<alva_agent_security::SecurityGuard>>()
            {
                let mut g = guard.lock().await;
                g.resolve_permission(&req.request_id, &req.tool_name, decision);
            }
        }
    });

    let rx = agent.prompt_text("spawn a worker to create the marker file");
    let _events = collect_events(rx).await;

    let approvals = counter.load(std::sync::atomic::Ordering::SeqCst);
    (tmp, approvals)
}

/// The child's shell call must surface as an approval request, and a
/// rejection must prevent the side effect. A regression to an ungated child
/// stack fails both assertions at once: no request is seen and the file
/// appears.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_shell_call_is_gated_and_reject_blocks_it() {
    let (tmp, approvals) = run_with_decision(PermissionDecision::RejectOnce).await;

    assert!(
        approvals >= 1,
        "child execute_shell never reached the approval gate — sub-agent HITL bypass regressed"
    );
    assert!(
        !tmp.path().join("child-side-effect.txt").exists(),
        "rejected child shell command still ran"
    );
}

/// Same wiring, approving decision: the gate must pass the call through —
/// proving the deny-test failure mode above is the gate, not a broken child.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn child_shell_call_executes_after_approval() {
    let (tmp, approvals) = run_with_decision(PermissionDecision::AllowOnce).await;

    assert!(
        approvals >= 1,
        "expected the child call to traverse the gate"
    );
    assert!(
        tmp.path().join("child-side-effect.txt").exists(),
        "approved child shell command did not run"
    );
}
