// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json
// OUTPUT: TaskCreateTool
// POS:    Creates a new task for tracking work progress.
//! task_create — create a new tracked task

use alva_kernel_abi::{
    AgentError, TaskType, Tool, ToolExecutionContext, ToolOutput,
    create_task_state,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

use crate::services::TaskService;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Short title / subject of the task.
    subject: String,
    /// Detailed description of the task.
    description: String,
    /// Optional key-value metadata to attach to the task.
    #[serde(default)]
    metadata: Option<HashMap<String, Value>>,
}

#[derive(Tool)]
#[tool(
    name = "task_create",
    description = "Create a new task for tracking work progress. Returns the task ID and confirmation.",
    input = Input,
)]
pub struct TaskCreateTool;

impl TaskCreateTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_create".into(),
                message: "task service not registered on bus".into(),
            })?;

        let output_dir = ctx
            .workspace()
            .map(|w| w.join(".tasks"))
            .unwrap_or_else(|| PathBuf::from("/tmp/.tasks"));

        let full_description = format!("{}: {}", params.subject, params.description);
        let state = create_task_state(
            TaskType::LocalAgent,
            full_description,
            None,
            output_dir.join("output.log"),
        );

        let task_id = state.id.clone();

        if let Err(e) = svc.create(state).await {
            return Ok(ToolOutput {
                content: vec![alva_kernel_abi::ToolContent::text(format!(
                    "Failed to create task: {e}"
                ))],
                is_error: true,
                details: None,
            });
        }

        let mut result = format!(
            "Task created successfully.\n  ID: {}\n  Subject: {}\n  Description: {}",
            task_id, params.subject, params.description
        );

        if let Some(ref meta) = params.metadata {
            result.push_str(&format!("\n  Metadata: {:?}", meta));
        }

        Ok(ToolOutput::text(result))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;

    use super::*;
    use alva_kernel_abi::{Bus, BusHandle, CancellationToken, Tool, ToolExecutionContext};
    use serde_json::json;

    use crate::services::{InMemoryTaskStore, TaskService};

    struct TestContext {
        workspace: Option<PathBuf>,
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
            self.workspace.as_deref()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn bus(&self) -> Option<&BusHandle> {
            self.bus.as_ref()
        }
    }

    /// Build a test context with a fresh `InMemoryTaskStore` provided on the
    /// bus, returning both the context and the store handle so assertions
    /// can verify the service was actually called.
    fn ctx_with_store(
        workspace: Option<PathBuf>,
    ) -> (TestContext, Arc<InMemoryTaskStore>) {
        let store = Arc::new(InMemoryTaskStore::new());
        let bus = Bus::new();
        bus.writer()
            .provide::<dyn TaskService>(store.clone());
        let ctx = TestContext {
            workspace,
            cancel: CancellationToken::new(),
            bus: Some(bus.handle()),
        };
        (ctx, store)
    }

    #[tokio::test]
    async fn task_create_returns_id_and_fields() {
        let (ctx, store) = ctx_with_store(Some(PathBuf::from("/workspace")));
        let tool = TaskCreateTool;

        let output = tool
            .execute(
                json!({
                    "subject": "Refactor login flow",
                    "description": "Move auth logic into a dedicated module",
                }),
                &ctx,
            )
            .await
            .expect("task_create should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("Task created successfully"), "got: {text}");
        assert!(text.contains("Refactor login flow"), "got: {text}");
        assert!(text.contains("Move auth logic"), "got: {text}");
        assert!(text.contains("ID:"), "expected an ID line: {text}");

        // Verify the service actually stored the task: list should now
        // contain exactly one task whose description prefixes with the
        // subject we sent.
        let listed = store.list(None).await;
        assert_eq!(listed.len(), 1);
        assert!(listed[0].description.starts_with("Refactor login flow:"));
    }

    #[tokio::test]
    async fn task_create_includes_metadata_when_present() {
        let (ctx, _store) = ctx_with_store(Some(PathBuf::from("/workspace")));
        let tool = TaskCreateTool;

        let output = tool
            .execute(
                json!({
                    "subject": "Test",
                    "description": "Desc",
                    "metadata": { "priority": "high", "owner": "alice" }
                }),
                &ctx,
            )
            .await
            .expect("task_create should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("Metadata"), "metadata block missing: {text}");
    }

    #[tokio::test]
    async fn task_create_rejects_missing_required_field() {
        // `description` is required; missing it should produce an Err from execute().
        let (ctx, _store) = ctx_with_store(None);
        let tool = TaskCreateTool;

        let err = tool
            .execute(json!({ "subject": "only subject" }), &ctx)
            .await
            .expect_err("missing description should error");

        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("description"),
            "expected invalid-input error, got: {msg}"
        );
    }

    #[tokio::test]
    async fn task_create_errors_when_service_not_on_bus() {
        // No bus / no service provided → ToolError.
        let ctx = TestContext {
            workspace: Some(PathBuf::from("/workspace")),
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskCreateTool;

        let err = tool
            .execute(
                json!({
                    "subject": "x",
                    "description": "y",
                }),
                &ctx,
            )
            .await
            .expect_err("missing service should error");
        let msg = format!("{err}");
        assert!(
            msg.contains("task service not registered"),
            "got: {msg}"
        );
    }
}
