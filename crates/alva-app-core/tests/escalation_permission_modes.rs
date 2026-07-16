// INPUT:  alva_app_core, alva_agent_extension_builtin, alva_agent_security, alva_kernel_abi, alva_test, tokio, tempfile
// OUTPUT: five PermissionMode goldens plus scripted escalation repair-loop integration coverage
// POS:    Native, bind-free contract tests for request_escalation permission routing and worker feedback.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use alva_agent_extension_builtin::request_escalation::{
    EscalationExecutor, EscalationRequest, RequestEscalationTool,
};
use alva_agent_security::{ApprovalRequest, PermissionDecision, SecurityGuard};
use alva_app_core::extension::{ApprovalPlugin, CorePlugin, PermissionPlugin};
use alva_app_core::{BaseAgent, PermissionMode};
use alva_kernel_abi::{AgentError, ToolExecutionContext, ToolFsExecResult};
use alva_test::assertions::{collect_events, tool_result_for};
use alva_test::fixtures::{make_assistant_message, tool_use_message};
use alva_test::mock_provider::MockLanguageModel;
use async_trait::async_trait;
use tokio::sync::mpsc;

struct RecordingExecutor {
    requests: Mutex<Vec<EscalationRequest>>,
    results: Mutex<VecDeque<ToolFsExecResult>>,
}

impl RecordingExecutor {
    fn successful() -> Arc<Self> {
        Arc::new(Self {
            requests: Mutex::new(Vec::new()),
            results: Mutex::new(VecDeque::from([ToolFsExecResult {
                stdout: "tests passed".into(),
                stderr: String::new(),
                exit_code: 0,
            }])),
        })
    }

    fn call_count(&self) -> usize {
        self.requests.lock().unwrap().len()
    }
}

#[async_trait]
impl EscalationExecutor for RecordingExecutor {
    async fn execute(
        &self,
        request: &EscalationRequest,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolFsExecResult, AgentError> {
        self.requests.lock().unwrap().push(request.clone());
        self.results
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "request_escalation".into(),
                message: "test executor has no result queued".into(),
            })
    }
}

async fn build_agent(
    workspace: &std::path::Path,
    mode: PermissionMode,
    model: MockLanguageModel,
    executor: Arc<dyn EscalationExecutor>,
    with_core_tools: bool,
) -> (BaseAgent, mpsc::UnboundedReceiver<ApprovalRequest>) {
    let (approval_plugin, approval_rx) = ApprovalPlugin::with_channel();
    let mut builder = BaseAgent::builder()
        .workspace(workspace)
        .system_prompt("scripted escalation test")
        .plugin(Box::new(PermissionPlugin::new().with_initial(mode)))
        .plugin(Box::new(approval_plugin))
        .tool(Box::new(RequestEscalationTool::new(executor)))
        .max_iterations(10);
    if with_core_tools {
        builder = builder.plugin(Box::new(CorePlugin));
    }
    let agent = builder.build(Arc::new(model)).await.expect("agent build");
    assert!(agent.set_permission_mode(mode));
    (agent, approval_rx)
}

fn one_escalation_model(command: &str, cwd: &std::path::Path) -> MockLanguageModel {
    MockLanguageModel::new()
        .with_response(tool_use_message(
            "escalate",
            "request_escalation",
            serde_json::json!({"command": command, "cwd": cwd}),
        ))
        .with_response(make_assistant_message("worker finished"))
}

async fn resolve(agent: &BaseAgent, request: &ApprovalRequest, decision: PermissionDecision) {
    let guard = agent
        .bus()
        .get::<tokio::sync::Mutex<SecurityGuard>>()
        .expect("security guard on bus");
    assert!(guard.lock().await.resolve_permission(
        &request.request_id,
        &request.tool_name,
        decision
    ));
}

async fn next_approval(
    approval_rx: &mut mpsc::UnboundedReceiver<ApprovalRequest>,
) -> ApprovalRequest {
    tokio::time::timeout(Duration::from_secs(2), approval_rx.recv())
        .await
        .expect("approval request was not emitted")
        .expect("approval channel closed")
}

#[tokio::test]
async fn golden_ask_waits_then_approval_executes_and_rejection_returns_reason() {
    for (decision, should_execute) in [
        (PermissionDecision::AllowOnce, true),
        (PermissionDecision::RejectOnce, false),
    ] {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_path = workspace.path().canonicalize().unwrap();
        let executor = RecordingExecutor::successful();
        let (agent, mut approval_rx) = build_agent(
            &workspace_path,
            PermissionMode::Ask,
            one_escalation_model("cargo test", &workspace_path),
            executor.clone(),
            false,
        )
        .await;

        let event_rx = agent.prompt_text("run tests outside the worker");
        let request = next_approval(&mut approval_rx).await;
        assert_eq!(request.tool_name, "request_escalation");
        assert_eq!(
            executor.call_count(),
            0,
            "Ask must suspend before execution"
        );
        resolve(&agent, &request, decision).await;

        let events = collect_events(event_rx).await;
        let result = tool_result_for(&events, "request_escalation");
        assert_eq!(executor.call_count(), usize::from(should_execute));
        if should_execute {
            assert!(!result.is_error, "approved request should execute");
            assert!(result.model_text().contains("tests passed"));
        } else {
            assert!(result.is_error, "rejected request must fail the tool task");
            assert!(
                result.model_text().contains("denied by user"),
                "rejection reason must be fed back to the worker: {}",
                result.model_text()
            );
        }
    }
}

#[tokio::test]
async fn golden_accept_shell_runs_safe_or_unknown_and_blocks_destructive() {
    for (command, should_execute) in [("cargo test", true), ("rm -rf build-output", false)] {
        let workspace = tempfile::tempdir().unwrap();
        let workspace_path = workspace.path().canonicalize().unwrap();
        let executor = RecordingExecutor::successful();
        let (agent, mut approval_rx) = build_agent(
            &workspace_path,
            PermissionMode::AcceptShell,
            one_escalation_model(command, &workspace_path),
            executor.clone(),
            false,
        )
        .await;

        let events = collect_events(agent.prompt_text("run host command")).await;
        let result = tool_result_for(&events, "request_escalation");
        assert_eq!(executor.call_count(), usize::from(should_execute));
        assert!(approval_rx.try_recv().is_err(), "Auto must not open HITL");
        if should_execute {
            assert!(!result.is_error, "cargo test should be auto-approved");
        } else {
            assert!(result.is_error, "destructive command must be blocked");
            assert!(result.model_text().contains("destructive command"));
        }
    }
}

#[tokio::test]
async fn golden_bypass_executes_without_prompting() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_path = workspace.path().canonicalize().unwrap();
    let executor = RecordingExecutor::successful();
    let (agent, mut approval_rx) = build_agent(
        &workspace_path,
        PermissionMode::Bypass,
        one_escalation_model("cargo test", &workspace_path),
        executor.clone(),
        false,
    )
    .await;

    let events = collect_events(agent.prompt_text("run host command")).await;
    assert!(!tool_result_for(&events, "request_escalation").is_error);
    assert_eq!(executor.call_count(), 1);
    assert!(approval_rx.try_recv().is_err());
}

#[tokio::test]
async fn golden_accept_edits_still_waits_for_shell_approval() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_path = workspace.path().canonicalize().unwrap();
    let executor = RecordingExecutor::successful();
    let (agent, mut approval_rx) = build_agent(
        &workspace_path,
        PermissionMode::AcceptEdits,
        one_escalation_model("cargo test", &workspace_path),
        executor.clone(),
        false,
    )
    .await;

    let event_rx = agent.prompt_text("run host command");
    let request = next_approval(&mut approval_rx).await;
    assert_eq!(executor.call_count(), 0);
    resolve(&agent, &request, PermissionDecision::RejectOnce).await;
    let result = tool_result_for(&collect_events(event_rx).await, "request_escalation");
    assert!(result.is_error);
    assert!(result.model_text().contains("denied by user"));
    assert_eq!(executor.call_count(), 0);
}

#[tokio::test]
async fn golden_plan_never_executes_escalation() {
    let workspace = tempfile::tempdir().unwrap();
    let workspace_path = workspace.path().canonicalize().unwrap();
    let executor = RecordingExecutor::successful();
    let (agent, mut approval_rx) = build_agent(
        &workspace_path,
        PermissionMode::Plan,
        one_escalation_model("cargo test", &workspace_path),
        executor.clone(),
        false,
    )
    .await;

    let events = collect_events(agent.prompt_text("run host command")).await;
    let result = tool_result_for(&events, "request_escalation");
    assert!(result.is_error);
    assert!(result.model_text().to_lowercase().contains("plan mode"));
    assert_eq!(executor.call_count(), 0);
    assert!(approval_rx.try_recv().is_err());
}

struct WorkspaceTestExecutor {
    requests: Mutex<Vec<EscalationRequest>>,
}

#[async_trait]
impl EscalationExecutor for WorkspaceTestExecutor {
    async fn execute(
        &self,
        request: &EscalationRequest,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolFsExecResult, AgentError> {
        self.requests.lock().unwrap().push(request.clone());
        let workspace = ctx.workspace().expect("workspace");
        let source = std::fs::read_to_string(workspace.join("answer.txt"))
            .map_err(|error| AgentError::Other(error.to_string()))?;
        if source == "FIXED" {
            Ok(ToolFsExecResult {
                stdout: "test_answer ... ok".into(),
                stderr: String::new(),
                exit_code: 0,
            })
        } else {
            Ok(ToolFsExecResult {
                stdout: "running 1 test".into(),
                stderr: "test_answer FAILED: expected FIXED, got BROKEN".into(),
                exit_code: 1,
            })
        }
    }
}

#[tokio::test]
async fn scripted_worker_edits_runs_tests_reads_failure_and_repairs() {
    let workspace = tempfile::tempdir().unwrap();
    let executor = Arc::new(WorkspaceTestExecutor {
        requests: Mutex::new(Vec::new()),
    });
    let model = MockLanguageModel::new()
        .with_response(tool_use_message(
            "write-broken",
            "create_file",
            serde_json::json!({"path": "answer.txt", "content": "BROKEN"}),
        ))
        .with_response(tool_use_message(
            "test-fails",
            "request_escalation",
            serde_json::json!({"command": "cargo test", "cwd": "."}),
        ))
        .with_response(tool_use_message(
            "repair",
            "file_edit",
            serde_json::json!({
                "path": "answer.txt",
                "old_str": "BROKEN",
                "new_str": "FIXED"
            }),
        ))
        .with_response(tool_use_message(
            "test-passes",
            "request_escalation",
            serde_json::json!({"command": "cargo test", "cwd": "."}),
        ))
        .with_response(make_assistant_message(
            "fixed after reading the failed test",
        ));
    let recorded_model = model.clone();
    let (agent, mut approval_rx) = build_agent(
        workspace.path(),
        PermissionMode::Bypass,
        model,
        executor.clone(),
        true,
    )
    .await;

    let events = collect_events(agent.prompt_text("repair answer.txt and verify it")).await;

    assert_eq!(
        std::fs::read_to_string(workspace.path().join("answer.txt")).unwrap(),
        "FIXED"
    );
    assert_eq!(executor.requests.lock().unwrap().len(), 2);
    let escalation_results = events
        .iter()
        .filter_map(|event| match event {
            alva_app_core::AgentEvent::ToolExecutionEnd { tool_call, result }
                if tool_call.name == "request_escalation" =>
            {
                Some(result.clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();
    assert_eq!(escalation_results.len(), 2);
    assert!(escalation_results[0].is_error);
    assert!(escalation_results[0]
        .model_text()
        .contains("expected FIXED, got BROKEN"));
    assert!(!escalation_results[1].is_error);
    assert!(escalation_results[1]
        .model_text()
        .contains("test_answer ... ok"));
    let recorded_calls = serde_json::to_string(&recorded_model.calls()).unwrap();
    assert!(
        recorded_calls.contains("expected FIXED, got BROKEN"),
        "the failed host output must reach the worker's next model turn"
    );
    assert!(approval_rx.try_recv().is_err());
}
