// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskListTool
// POS:    Lists all tracked tasks with optional status filter.
//! task_list — list tracked tasks

use alva_kernel_abi::{AgentError, TaskStatus, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::TaskService;

/// Task status filter for the list query.
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum StatusFilter {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl StatusFilter {
    fn to_kernel(self) -> TaskStatus {
        match self {
            Self::Pending => TaskStatus::Pending,
            Self::Running => TaskStatus::Running,
            Self::Completed => TaskStatus::Completed,
            Self::Failed => TaskStatus::Failed,
            Self::Killed => TaskStatus::Killed,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Killed => "killed",
        }
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Filter tasks by status.
    #[serde(default)]
    status: Option<StatusFilter>,
}

#[derive(Tool)]
#[tool(
    name = "task_list",
    description = "List all tracked tasks. Optionally filter by status (pending, running, completed, failed, killed).",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskListTool;

impl TaskListTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_list".into(),
                message: "task service not registered on bus".into(),
            })?;

        let filter = params.status.map(|s| s.to_kernel());
        let filter_msg = match params.status {
            Some(s) => format!(" (filter: status={})", s.label()),
            None => String::new(),
        };

        let tasks = svc.list(filter).await;

        if tasks.is_empty() {
            return Ok(ToolOutput::text(format!("No tasks found{}.", filter_msg)));
        }

        let mut out = format!("Tasks ({}){}", tasks.len(), filter_msg);
        for t in &tasks {
            let st = match t.status {
                TaskStatus::Pending => "pending",
                TaskStatus::Running => "running",
                TaskStatus::Completed => "completed",
                TaskStatus::Failed => "failed",
                TaskStatus::Killed => "killed",
            };
            out.push_str(&format!("\n  {} [{}] {}", t.id, st, t.description));
        }
        Ok(ToolOutput::text(out))
    }
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
    async fn task_list_no_filter_returns_empty_placeholder() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskListTool;

        let output = tool
            .execute(json!({}), &ctx)
            .await
            .expect("task_list should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("No tasks found"), "got: {text}");
        assert!(
            !text.contains("filter:"),
            "should have no filter clause: {text}"
        );
    }

    #[tokio::test]
    async fn task_list_with_status_filter_mentions_filter() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskListTool;

        let output = tool
            .execute(json!({ "status": "running" }), &ctx)
            .await
            .expect("task_list should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("filter: status=running"), "got: {text}");
    }

    #[tokio::test]
    async fn task_list_returns_seeded_tasks() {
        let (ctx, store) = ctx_with_store();
        for desc in ["alpha", "beta"] {
            let t = create_task_state(
                TaskType::LocalAgent,
                desc.into(),
                None,
                PathBuf::from("/tmp/x"),
            );
            store.create(t).await.unwrap();
        }
        let tool = TaskListTool;
        let output = tool
            .execute(json!({}), &ctx)
            .await
            .expect("task_list should succeed");
        let text = output.model_text();
        assert!(text.contains("alpha"), "got: {text}");
        assert!(text.contains("beta"), "got: {text}");
    }

    #[tokio::test]
    async fn task_list_filter_narrows_results() {
        let (ctx, store) = ctx_with_store();
        let mut r = create_task_state(
            TaskType::LocalAgent,
            "running-one".into(),
            None,
            PathBuf::from("/tmp/x"),
        );
        r.status = TaskStatus::Running;
        store.create(r).await.unwrap();

        let mut d = create_task_state(
            TaskType::LocalAgent,
            "done-one".into(),
            None,
            PathBuf::from("/tmp/x"),
        );
        d.status = TaskStatus::Completed;
        store.create(d).await.unwrap();

        let tool = TaskListTool;
        let output = tool
            .execute(json!({ "status": "running" }), &ctx)
            .await
            .expect("ok");
        let text = output.model_text();
        assert!(text.contains("running-one"), "got: {text}");
        assert!(!text.contains("done-one"), "got: {text}");
    }

    #[tokio::test]
    async fn task_list_rejects_invalid_status_value() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskListTool;

        let err = tool
            .execute(json!({ "status": "bogus_status" }), &ctx)
            .await
            .expect_err("invalid enum value should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn task_list_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskListTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("task service not registered"));
    }

    #[test]
    fn task_list_is_read_only() {
        let tool = TaskListTool;
        assert!(!tool.is_destructive(&json!({})));
        assert_eq!(tool.name(), "task_list");
    }
}
