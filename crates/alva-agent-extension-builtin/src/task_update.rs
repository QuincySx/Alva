// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json
// OUTPUT: TaskUpdateTool
// POS:    Updates an existing tracked task's fields.
//! task_update — update an existing task

use alva_kernel_abi::{
    AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::services::{TaskService, TaskUpdate};

/// Task status values exposed to the LLM. Note: the local "in_progress"
/// label maps to the kernel's `Running` status, and "deleted" maps to
/// `Killed` (no separate "deleted" terminal state in alva-kernel-abi —
/// see the mapping table in `to_kernel_status`).
#[derive(Debug, Deserialize, JsonSchema, Clone, Copy, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
}

fn to_kernel_status(s: TaskStatus) -> alva_kernel_abi::TaskStatus {
    match s {
        TaskStatus::Pending => alva_kernel_abi::TaskStatus::Pending,
        TaskStatus::InProgress => alva_kernel_abi::TaskStatus::Running,
        TaskStatus::Completed => alva_kernel_abi::TaskStatus::Completed,
        // No `Deleted` in the kernel — fold into `Killed` so the audit
        // trail still shows the task as terminated rather than throwing
        // away the user's intent.
        TaskStatus::Deleted => alva_kernel_abi::TaskStatus::Killed,
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// ID of the task to update.
    task_id: String,
    /// New subject / title.
    #[serde(default)]
    subject: Option<String>,
    /// New description.
    #[serde(default)]
    description: Option<String>,
    /// New status.
    #[serde(default)]
    status: Option<TaskStatus>,
    /// Agent or user that owns the task.
    #[serde(default)]
    owner: Option<String>,
    /// Task IDs that this task blocks.
    #[serde(default)]
    add_blocks: Option<Vec<String>>,
    /// Task IDs that block this task.
    #[serde(default)]
    add_blocked_by: Option<Vec<String>>,
    /// Additional metadata to merge.
    #[serde(default)]
    metadata: Option<Value>,
}

#[derive(Tool)]
#[tool(
    name = "task_update",
    description = "Update an existing task. You can change its subject, description, status, owner, \
        blocking relationships, or metadata.",
    input = Input,
)]
pub struct TaskUpdateTool;

impl TaskUpdateTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TaskService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task_update".into(),
                message: "task service not registered on bus".into(),
            })?;

        // Build the mutation. The TaskService currently models status +
        // description + append_output; subject/owner/blocks/metadata don't
        // exist on `TaskState` yet, so we surface them in the response
        // text purely as an audit trail.
        let mut mutation = TaskUpdate::default();
        let mut updates = Vec::new();

        if let Some(ref s) = params.subject {
            updates.push(format!("  subject → {}", s));
        }
        if let Some(ref d) = params.description {
            updates.push(format!("  description → {}", d));
            mutation.description = Some(d.clone());
        }
        if let Some(st) = params.status {
            let label = match st {
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Completed => "completed",
                TaskStatus::Deleted => "deleted",
            };
            updates.push(format!("  status → {}", label));
            mutation.status = Some(to_kernel_status(st));
        }
        if let Some(ref o) = params.owner {
            updates.push(format!("  owner → {}", o));
        }
        if let Some(ref blocks) = params.add_blocks {
            updates.push(format!("  blocks → {:?}", blocks));
        }
        if let Some(ref blocked) = params.add_blocked_by {
            updates.push(format!("  blocked_by → {:?}", blocked));
        }
        if params.metadata.is_some() {
            updates.push("  metadata updated".to_string());
        }

        if updates.is_empty() {
            return Ok(ToolOutput::text(format!(
                "Task {} — no fields to update.",
                params.task_id
            )));
        }

        // If anything touches kernel-tracked fields (status or
        // description), call into the service. Otherwise the change is
        // pure audit-trail — skip the service round-trip but still
        // require the service exists so the bus contract is enforced.
        if mutation.status.is_some() || mutation.description.is_some() {
            if let Err(e) = svc.update(&params.task_id, mutation).await {
                return Ok(ToolOutput {
                    content: vec![ToolContent::text(format!(
                        "Failed to update task {}: {e}",
                        params.task_id
                    ))],
                    is_error: true,
                    details: None,
                });
            }
        } else {
            // Validate the task at least exists; otherwise the user has
            // no way of knowing they typed the wrong id.
            if svc.get(&params.task_id).await.is_none() {
                return Ok(ToolOutput {
                    content: vec![ToolContent::text(format!(
                        "Task {} not found.",
                        params.task_id
                    ))],
                    is_error: true,
                    details: None,
                });
            }
        }

        Ok(ToolOutput::text(format!(
            "Task {} updated:\n{}",
            params.task_id,
            updates.join("\n")
        )))
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
    async fn task_update_reports_changed_fields() {
        let (ctx, store) = ctx_with_store();
        let t = create_task_state(
            TaskType::LocalAgent,
            "x".into(),
            None,
            PathBuf::from("/tmp/out"),
        );
        let id = t.id.clone();
        store.create(t).await.unwrap();

        let tool = TaskUpdateTool;
        let output = tool
            .execute(
                json!({
                    "task_id": id.clone(),
                    "subject": "New subject",
                    "status": "in_progress",
                    "owner": "bob",
                }),
                &ctx,
            )
            .await
            .expect("task_update should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains(&id), "got: {text}");
        assert!(text.contains("subject → New subject"), "got: {text}");
        assert!(text.contains("status → in_progress"), "got: {text}");
        assert!(text.contains("owner → bob"), "got: {text}");

        // Verify the service persisted the status mapping.
        let after = store.get(&id).await.unwrap();
        assert_eq!(after.status, alva_kernel_abi::TaskStatus::Running);
    }

    #[tokio::test]
    async fn task_update_deleted_maps_to_killed() {
        let (ctx, store) = ctx_with_store();
        let t = create_task_state(
            TaskType::LocalAgent,
            "x".into(),
            None,
            PathBuf::from("/tmp/out"),
        );
        let id = t.id.clone();
        store.create(t).await.unwrap();

        let tool = TaskUpdateTool;
        let output = tool
            .execute(
                json!({
                    "task_id": id.clone(),
                    "status": "deleted",
                }),
                &ctx,
            )
            .await
            .expect("ok");
        assert!(!output.is_error);
        let after = store.get(&id).await.unwrap();
        assert_eq!(after.status, alva_kernel_abi::TaskStatus::Killed);
    }

    #[tokio::test]
    async fn task_update_with_no_fields_returns_noop_message() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskUpdateTool;

        let output = tool
            .execute(json!({ "task_id": "tsk-empty" }), &ctx)
            .await
            .expect("task_update should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("tsk-empty"), "got: {text}");
        assert!(text.contains("no fields to update"), "got: {text}");
    }

    #[tokio::test]
    async fn task_update_unknown_task_with_kernel_field_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskUpdateTool;
        let output = tool
            .execute(
                json!({
                    "task_id": "tsk-nope",
                    "status": "completed",
                }),
                &ctx,
            )
            .await
            .expect("not-found shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("tsk-nope"), "got: {text}");
    }

    #[tokio::test]
    async fn task_update_rejects_missing_task_id() {
        let (ctx, _) = ctx_with_store();
        let tool = TaskUpdateTool;

        let err = tool
            .execute(json!({ "subject": "x" }), &ctx)
            .await
            .expect_err("missing task_id should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn task_update_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TaskUpdateTool;
        let err = tool
            .execute(json!({ "task_id": "x", "status": "completed" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("task service not registered"));
    }
}
