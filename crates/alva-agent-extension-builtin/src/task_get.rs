// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskGetTool
// POS:    Retrieves full details of a tracked task by ID.
//! task_get — retrieve task details

use alva_kernel_abi::{AgentError, TaskState, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::TaskService;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The task ID to look up.
    task_id: String,
}

#[derive(Tool)]
#[tool(
    name = "task_get",
    description = "Retrieve the full details of a task by its ID, including status, description, \
        and any output produced so far.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskGetTool;

impl TaskGetTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_get".into(),
                message: "task service not registered on bus".into(),
            })?;

        match svc.get(&params.task_id).await {
            Some(state) => Ok(ToolOutput::text(format_state(&state))),
            None => Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Task {} not found.",
                    params.task_id
                ))],
                is_error: true,
                details: None,
            }),
        }
    }
}

fn format_state(s: &TaskState) -> String {
    let status = match s.status {
        alva_kernel_abi::TaskStatus::Pending => "pending",
        alva_kernel_abi::TaskStatus::Running => "running",
        alva_kernel_abi::TaskStatus::Completed => "completed",
        alva_kernel_abi::TaskStatus::Failed => "failed",
        alva_kernel_abi::TaskStatus::Killed => "killed",
    };
    let mut out = format!(
        "Task {}\n  Status: {}\n  Description: {}\n  Started: {}",
        s.id, status, s.description, s.start_time
    );
    if let Some(end) = s.end_time {
        out.push_str(&format!("\n  Ended: {}", end));
    }
    out
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use alva_kernel_abi::{
        create_task_state, Bus, BusHandle, CancellationToken, TaskType, Tool, ToolExecutionContext,
    };
    use serde_json::json;

    use crate::services::{InMemoryTaskStore, TaskService};

    struct TestContext {
        cancel: CancellationToken,
        bus: Option<BusHandle>,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn workspace(&self) -> Option<&Path> {
            None
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn bus(&self) -> Option<&BusHandle> {
            self.bus.as_ref()
        }
    }

    fn ctx_with_store() -> (TestContext, Arc<InMemoryTaskStore>) {
        let store = Arc::new(InMemoryTaskStore::new());
        let bus = Bus::new();
        bus.writer().provide::<dyn TaskService>(store.clone());
        (
            TestContext {
                cancel: CancellationToken::new(),
                bus: Some(bus.handle()),
            },
            store,
        )
    }

    #[tokio::test]
    async fn task_get_returns_state_when_found() {
        let (ctx, store) = ctx_with_store();
        let state = create_task_state(
            TaskType::LocalAgent,
            "alpha task".into(),
            None,
            PathBuf::from("/tmp/out.log"),
        );
        let id = state.id.clone();
        store.create(state).await.unwrap();

        let tool = TaskGetTool;
        let output = tool
            .execute(json!({ "task_id": id.clone() }), &ctx)
            .await
            .expect("task_get should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains(&id), "id missing from output: {text}");
        assert!(text.contains("alpha task"), "description missing: {text}");
        assert!(text.contains("pending"), "status missing: {text}");
    }

    #[tokio::test]
    async fn task_get_missing_returns_error_output() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskGetTool;

        let output = tool
            .execute(json!({ "task_id": "tsk-nope" }), &ctx)
            .await
            .expect("task_get should not Err for not-found");

        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("tsk-nope"), "id missing from output: {text}");
        assert!(
            text.contains("not found"),
            "expected not found message: {text}"
        );
    }

    #[tokio::test]
    async fn task_get_rejects_missing_task_id() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskGetTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing task_id should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("task_id"),
            "unexpected error: {msg}"
        );
    }

    #[tokio::test]
    async fn task_get_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskGetTool;

        let err = tool
            .execute(json!({ "task_id": "x" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("task service not registered"));
    }

    #[test]
    fn task_get_is_read_only() {
        let tool = TaskGetTool;
        assert!(!tool.is_destructive(&json!({ "task_id": "x" })));
        assert_eq!(tool.name(), "task_get");
    }
}
