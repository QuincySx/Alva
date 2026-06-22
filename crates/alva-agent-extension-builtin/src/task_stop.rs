// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskStopTool
// POS:    Stops/cancels a running task.
//! task_stop — stop or cancel a running task

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::TaskService;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The task ID to stop.
    task_id: String,
}

#[derive(Tool)]
#[tool(
    name = "task_stop",
    description = "Stop or cancel a running task. The task will be marked as killed.",
    input = Input,
    destructive,
)]
pub struct TaskStopTool;

impl TaskStopTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_stop".into(),
                message: "task service not registered on bus".into(),
            })?;

        match svc.stop(&params.task_id).await {
            Ok(_) => Ok(ToolOutput::text(format!(
                "Task {} stop requested.",
                params.task_id
            ))),
            Err(e) => Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Failed to stop task {}: {e}",
                    params.task_id
                ))],
                is_error: true,
                details: None,
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use alva_kernel_abi::{
        create_task_state, Bus, BusHandle, CancellationToken, TaskStatus, TaskType, Tool,
        ToolExecutionContext,
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
    async fn task_stop_acknowledges_request_and_kills() {
        let (ctx, store) = ctx_with_store();
        let t = create_task_state(
            TaskType::LocalAgent,
            "x".into(),
            None,
            PathBuf::from("/tmp/out"),
        );
        let id = t.id.clone();
        store.create(t).await.unwrap();
        let tool = TaskStopTool;

        let output = tool
            .execute(json!({ "task_id": id.clone() }), &ctx)
            .await
            .expect("task_stop should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains(&id), "got: {text}");
        assert!(text.contains("stop requested"), "got: {text}");

        let after = store.get(&id).await.unwrap();
        assert_eq!(after.status, TaskStatus::Killed);
    }

    #[tokio::test]
    async fn task_stop_unknown_task_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskStopTool;
        let output = tool
            .execute(json!({ "task_id": "tsk-nope" }), &ctx)
            .await
            .expect("not-found shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("tsk-nope"), "got: {text}");
    }

    #[tokio::test]
    async fn task_stop_rejects_missing_task_id() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskStopTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing task_id should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn task_stop_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskStopTool;
        let err = tool
            .execute(json!({ "task_id": "x" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("task service not registered"));
    }

    #[test]
    fn task_stop_is_destructive() {
        let tool = TaskStopTool;
        assert!(tool.is_destructive(&json!({ "task_id": "x" })));
        assert_eq!(tool.name(), "task_stop");
    }
}
