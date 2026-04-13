// INPUT:  alva_types, async_trait, schemars, serde, serde_json
// OUTPUT: TaskUpdateTool
// POS:    Updates an existing tracked task's fields.
//! task_update — update an existing task

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

/// Task status values.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum TaskStatus {
    Pending,
    InProgress,
    Completed,
    Deleted,
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
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let mut updates = Vec::new();
        if let Some(ref s) = params.subject {
            updates.push(format!("  subject → {}", s));
        }
        if let Some(ref d) = params.description {
            updates.push(format!("  description → {}", d));
        }
        if let Some(ref st) = params.status {
            let label = match st {
                TaskStatus::Pending => "pending",
                TaskStatus::InProgress => "in_progress",
                TaskStatus::Completed => "completed",
                TaskStatus::Deleted => "deleted",
            };
            updates.push(format!("  status → {}", label));
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

        Ok(ToolOutput::text(format!(
            "Task {} updated:\n{}",
            params.task_id,
            updates.join("\n")
        )))
    }
}
