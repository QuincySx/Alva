// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TaskGetTool
// POS:    Retrieves full details of a tracked task by ID.
//! task_get — retrieve task details

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    task_id: String,
}

pub struct TaskGetTool;

#[async_trait]
impl Tool for TaskGetTool {
    fn name(&self) -> &str {
        "task_get"
    }

    fn description(&self) -> &str {
        "Retrieve the full details of a task by its ID, including status, description, \
         and any output produced so far."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to look up"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
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

        // In a full implementation this would look up TaskState from a shared store.
        // For now, return a placeholder indicating the task ID was requested.
        Ok(ToolOutput::text(format!(
            "Task {} — details not found. The task store is not yet connected.\n\
             Hint: Tasks are tracked in-memory during agent sessions.",
            params.task_id
        )))
    }
}
