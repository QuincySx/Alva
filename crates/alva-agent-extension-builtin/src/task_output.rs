// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskOutputTool
// POS:    Retrieves the output/results of a task from its output file.
//! task_output — get task output content

use alva_kernel_abi::{
    AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::TaskService;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The task ID whose output to retrieve.
    task_id: String,
}

#[derive(Tool)]
#[tool(
    name = "task_output",
    description = "Get the output or results produced by a task. This reads from the task's output file.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskOutputTool;

impl TaskOutputTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_output".into(),
                message: "task service not registered on bus".into(),
            })?;

        match svc.read_output(&params.task_id).await {
            Ok(out) if out.is_empty() => Ok(ToolOutput::text(format!(
                "Task {} has no output yet.",
                params.task_id
            ))),
            Ok(out) => Ok(ToolOutput::text(format!(
                "Task {} output:\n{}",
                params.task_id, out
            ))),
            Err(e) => Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Failed to read task {} output: {e}",
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
        Bus, BusHandle, CancellationToken, TaskType, Tool, ToolExecutionContext,
        create_task_state,
    };
    use serde_json::json;

    use crate::services::{InMemoryTaskStore, TaskService, TaskUpdate};

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
        bus.writer()
            .provide::<dyn TaskService>(store.clone());
        (
            TestContext {
                cancel: CancellationToken::new(),
                bus: Some(bus.handle()),
            },
            store,
        )
    }

    #[tokio::test]
    async fn task_output_returns_appended_text() {
        let (ctx, store) = ctx_with_store();
        let t = create_task_state(
            TaskType::LocalAgent,
            "x".into(),
            None,
            PathBuf::from("/tmp/out"),
        );
        let id = t.id.clone();
        store.create(t).await.unwrap();
        store
            .update(
                &id,
                TaskUpdate {
                    append_output: Some("hello world".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();

        let tool = TaskOutputTool;
        let output = tool
            .execute(json!({ "task_id": id.clone() }), &ctx)
            .await
            .expect("task_output should succeed");
        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains(&id), "got: {text}");
        assert!(text.contains("hello world"), "got: {text}");
    }

    #[tokio::test]
    async fn task_output_empty_when_no_output() {
        let (ctx, store) = ctx_with_store();
        let t = create_task_state(
            TaskType::LocalAgent,
            "x".into(),
            None,
            PathBuf::from("/tmp/out"),
        );
        let id = t.id.clone();
        store.create(t).await.unwrap();

        let tool = TaskOutputTool;
        let output = tool
            .execute(json!({ "task_id": id.clone() }), &ctx)
            .await
            .expect("ok");
        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("no output"), "got: {text}");
    }

    #[tokio::test]
    async fn task_output_missing_task_returns_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskOutputTool;
        let output = tool
            .execute(json!({ "task_id": "tsk-nope" }), &ctx)
            .await
            .expect("not-found shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("tsk-nope"), "got: {text}");
    }

    #[tokio::test]
    async fn task_output_rejects_missing_task_id() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskOutputTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing task_id should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("task_id"),
            "got: {msg}"
        );
    }

    #[tokio::test]
    async fn task_output_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskOutputTool;
        let err = tool
            .execute(json!({ "task_id": "x" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("task service not registered"));
    }

    #[test]
    fn task_output_is_read_only() {
        let tool = TaskOutputTool;
        assert!(!tool.is_destructive(&json!({ "task_id": "x" })));
        assert_eq!(tool.name(), "task_output");
    }
}
