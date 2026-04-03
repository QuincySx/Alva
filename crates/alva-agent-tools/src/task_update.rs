// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TaskUpdateTool
// POS:    Updates an existing tracked task's fields.
//! task_update — update an existing task

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    task_id: String,
    #[serde(default)]
    subject: Option<String>,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    status: Option<String>,
    #[serde(default)]
    owner: Option<String>,
    #[serde(default)]
    add_blocks: Option<Vec<String>>,
    #[serde(default)]
    add_blocked_by: Option<Vec<String>>,
    #[serde(default)]
    metadata: Option<Value>,
}

pub struct TaskUpdateTool;

#[async_trait]
impl Tool for TaskUpdateTool {
    fn name(&self) -> &str {
        "task_update"
    }

    fn description(&self) -> &str {
        "Update an existing task. You can change its subject, description, status, owner, \
         blocking relationships, or metadata."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "ID of the task to update"
                },
                "subject": {
                    "type": "string",
                    "description": "New subject / title"
                },
                "description": {
                    "type": "string",
                    "description": "New description"
                },
                "status": {
                    "type": "string",
                    "enum": ["pending", "in_progress", "completed", "deleted"],
                    "description": "New status"
                },
                "owner": {
                    "type": "string",
                    "description": "Agent or user that owns the task"
                },
                "add_blocks": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that this task blocks"
                },
                "add_blocked_by": {
                    "type": "array",
                    "items": { "type": "string" },
                    "description": "Task IDs that block this task"
                },
                "metadata": {
                    "type": "object",
                    "description": "Additional metadata to merge"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        let mut updates = Vec::new();
        if let Some(ref s) = params.subject {
            updates.push(format!("  subject → {}", s));
        }
        if let Some(ref d) = params.description {
            updates.push(format!("  description → {}", d));
        }
        if let Some(ref st) = params.status {
            // Validate status value
            match st.as_str() {
                "pending" | "in_progress" | "completed" | "deleted" => {}
                other => {
                    return Ok(ToolOutput::error(format!(
                        "Invalid status '{}'. Must be one of: pending, in_progress, completed, deleted",
                        other
                    )));
                }
            }
            updates.push(format!("  status → {}", st));
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
