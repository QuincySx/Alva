//! End-to-end tests that drive the task / team tools through their
//! `Tool::execute` JSON contract using a real `Bus` + `InMemoryTaskStore`
//! / `InMemoryTeamStore` registered as the backing service. Verifies the
//! create → list → update → stop and team_create → send_message →
//! inbox flows end-to-end with no shortcuts into the service trait.

#![cfg(all(feature = "task", feature = "team"))]

use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_agent_extension_builtin::send_message::SendMessageTool;
use alva_agent_extension_builtin::services::{
    InMemoryTaskStore, InMemoryTeamStore, TaskService, TeamService,
};
use alva_agent_extension_builtin::task_create::TaskCreateTool;
use alva_agent_extension_builtin::task_get::TaskGetTool;
use alva_agent_extension_builtin::task_list::TaskListTool;
use alva_agent_extension_builtin::task_stop::TaskStopTool;
use alva_agent_extension_builtin::task_update::TaskUpdateTool;
use alva_agent_extension_builtin::team_create::TeamCreateTool;
use alva_agent_extension_builtin::team_delete::TeamDeleteTool;
use alva_kernel_abi::{Bus, BusHandle, CancellationToken, Tool, ToolExecutionContext};
use serde_json::{json, Value};

struct TestCtx {
    cancel: CancellationToken,
    bus: Option<BusHandle>,
    workspace: Option<PathBuf>,
    session: String,
}

impl ToolExecutionContext for TestCtx {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
    fn session_id(&self) -> &str {
        &self.session
    }
    fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn bus(&self) -> Option<&BusHandle> {
        self.bus.as_ref()
    }
}

/// Extract a task id from the freeform text emitted by `task_create`.
/// The output line is `  ID: <id>` — pull whatever follows that prefix.
fn extract_task_id(text: &str) -> String {
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("ID:") {
            return rest.trim().to_string();
        }
    }
    panic!("no `ID:` line in task_create output: {text}");
}

async fn invoke(tool: &dyn Tool, args: Value, ctx: &dyn ToolExecutionContext) -> String {
    let out = tool.execute(args, ctx).await.expect("execute ok");
    assert!(!out.is_error, "tool returned is_error=true: {:?}", out);
    out.model_text()
}

#[tokio::test]
async fn task_lifecycle_end_to_end() {
    let store = Arc::new(InMemoryTaskStore::new());
    let bus = Bus::new();
    bus.writer().provide::<dyn TaskService>(store.clone());

    let ctx = TestCtx {
        cancel: CancellationToken::new(),
        bus: Some(bus.handle()),
        workspace: Some(PathBuf::from("/tmp/e2e-workspace")),
        session: "e2e-session".into(),
    };

    // 1. create
    let create_text = invoke(
        &TaskCreateTool,
        json!({
            "subject": "Refactor",
            "description": "Move auth to its own module",
        }),
        &ctx,
    )
    .await;
    let id = extract_task_id(&create_text);

    // 2. list returns the new task
    let list_text = invoke(&TaskListTool, json!({}), &ctx).await;
    assert!(list_text.contains(&id), "list missing id: {list_text}");

    // 3. get returns full state
    let get_text = invoke(&TaskGetTool, json!({ "task_id": id.clone() }), &ctx).await;
    assert!(get_text.contains("Refactor"), "got: {get_text}");
    assert!(get_text.contains("pending"), "got: {get_text}");

    // 4. update → in_progress (kernel: Running)
    invoke(
        &TaskUpdateTool,
        json!({ "task_id": id.clone(), "status": "in_progress" }),
        &ctx,
    )
    .await;
    let after_update = store.get(&id).await.unwrap();
    assert_eq!(after_update.status, alva_kernel_abi::TaskStatus::Running);

    // 5. stop → killed
    invoke(&TaskStopTool, json!({ "task_id": id.clone() }), &ctx).await;
    let after_stop = store.get(&id).await.unwrap();
    assert_eq!(after_stop.status, alva_kernel_abi::TaskStatus::Killed);

    // 6. list with running filter is now empty
    let list_running = invoke(&TaskListTool, json!({ "status": "running" }), &ctx).await;
    assert!(
        list_running.contains("No tasks found"),
        "expected empty: {list_running}"
    );
}

#[tokio::test]
async fn team_create_send_inbox_end_to_end() {
    let store = Arc::new(InMemoryTeamStore::new());
    let bus = Bus::new();
    bus.writer().provide::<dyn TeamService>(store.clone());

    let ctx = TestCtx {
        cancel: CancellationToken::new(),
        bus: Some(bus.handle()),
        workspace: None,
        session: "sender-session".into(),
    };

    // 1. team_create alice
    invoke(
        &TeamCreateTool,
        json!({
            "team_name": "alice",
            "description": "research helper",
            "agent_type": "research",
        }),
        &ctx,
    )
    .await;

    // 2. send_message to alice
    invoke(
        &SendMessageTool,
        json!({
            "to": "alice",
            "message": "please summarise the spec",
            "summary": "summarise spec",
        }),
        &ctx,
    )
    .await;

    // 3. alice's inbox sees the message with the sender = session_id()
    let inbox = store.inbox("alice").await;
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].body, "please summarise the spec");
    assert_eq!(inbox[0].from, "sender-session");
    assert_eq!(inbox[0].summary.as_deref(), Some("summarise spec"));

    // 4. team_delete alice
    invoke(&TeamDeleteTool, json!({ "team_name": "alice" }), &ctx).await;
    assert!(store.get("alice").await.is_none());
}

// ---------------------------------------------------------------------------
// Error-path e2e: verify tools surface service errors as `is_error=true`
// ToolOutput (LLM-readable) rather than panicking or returning AgentError.
// ---------------------------------------------------------------------------

/// Helper to call a tool and return the (output, is_error) pair without
/// the happy-path assertion that `invoke()` does.
async fn invoke_raw(
    tool: &dyn Tool,
    args: Value,
    ctx: &dyn ToolExecutionContext,
) -> (String, bool) {
    let out = tool.execute(args, ctx).await.expect("execute returns Ok");
    (out.model_text(), out.is_error)
}

#[tokio::test]
async fn task_get_unknown_id_surfaces_tool_output_error() {
    let store = Arc::new(InMemoryTaskStore::new());
    let bus = Bus::new();
    bus.writer().provide::<dyn TaskService>(store.clone());
    let ctx = TestCtx {
        cancel: CancellationToken::new(),
        bus: Some(bus.handle()),
        workspace: Some(PathBuf::from("/tmp/e2e-err")),
        session: "e2e".into(),
    };

    // Unknown task_id → tool surfaces is_error=true (NOT panic, NOT
    // AgentError) so LLM can react. task_stop on same id should also
    // surface cleanly.
    let (get_text, get_err) =
        invoke_raw(&TaskGetTool, json!({ "task_id": "ghost-id-9999" }), &ctx).await;
    assert!(get_err, "task_get on unknown id must set is_error");
    assert!(
        get_text.contains("ghost-id-9999") || get_text.contains("not found"),
        "expected NotFound-ish message: {get_text}"
    );

    let (stop_text, stop_err) =
        invoke_raw(&TaskStopTool, json!({ "task_id": "ghost-id-9999" }), &ctx).await;
    assert!(stop_err, "task_stop on unknown id must set is_error");
    assert!(
        stop_text.contains("ghost-id-9999") || stop_text.contains("not found"),
        "expected NotFound-ish message: {stop_text}"
    );
}

#[tokio::test]
async fn task_update_from_terminal_state_surfaces_error() {
    let store = Arc::new(InMemoryTaskStore::new());
    let bus = Bus::new();
    bus.writer().provide::<dyn TaskService>(store.clone());
    let ctx = TestCtx {
        cancel: CancellationToken::new(),
        bus: Some(bus.handle()),
        workspace: Some(PathBuf::from("/tmp/e2e-err2")),
        session: "e2e".into(),
    };

    // Create → stop (terminal) → try to update back to Running
    let create_text = invoke(
        &TaskCreateTool,
        json!({ "subject": "x", "description": "y" }),
        &ctx,
    )
    .await;
    let id = extract_task_id(&create_text);
    invoke(&TaskStopTool, json!({ "task_id": id.clone() }), &ctx).await;

    // Now the service returns TaskError::AlreadyTerminated; tool should
    // surface it as is_error rather than crashing.
    let (text, is_err) = invoke_raw(
        &TaskUpdateTool,
        json!({ "task_id": id.clone(), "status": "in_progress" }),
        &ctx,
    )
    .await;
    assert!(is_err, "update from terminal state must set is_error");
    assert!(
        text.contains(&id) || text.contains("terminated") || text.contains("already"),
        "expected AlreadyTerminated-ish msg: {text}"
    );
}

#[tokio::test]
async fn send_message_to_unknown_recipient_surfaces_error() {
    let store = Arc::new(InMemoryTeamStore::new());
    let bus = Bus::new();
    bus.writer().provide::<dyn TeamService>(store.clone());
    let ctx = TestCtx {
        cancel: CancellationToken::new(),
        bus: Some(bus.handle()),
        workspace: None,
        session: "sender".into(),
    };

    // No teammates registered → send to anyone returns NotFound.
    let (text, is_err) = invoke_raw(
        &SendMessageTool,
        json!({ "to": "phantom", "message": "hello?" }),
        &ctx,
    )
    .await;
    assert!(is_err, "send to unknown recipient must set is_error");
    assert!(
        text.contains("phantom") || text.contains("not found"),
        "expected NotFound-ish msg: {text}"
    );

    // After register + delete, send to the deleted name should also fail
    invoke(
        &TeamCreateTool,
        json!({ "team_name": "ephemeral", "description": "temp" }),
        &ctx,
    )
    .await;
    invoke(&TeamDeleteTool, json!({ "team_name": "ephemeral" }), &ctx).await;
    let (text2, is_err2) = invoke_raw(
        &SendMessageTool,
        json!({ "to": "ephemeral", "message": "post-delete" }),
        &ctx,
    )
    .await;
    assert!(is_err2, "send to deleted recipient must set is_error");
    assert!(
        text2.contains("ephemeral") || text2.contains("not found"),
        "expected NotFound after delete: {text2}"
    );
}
