// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TaskListTool
// POS:    Lists all tracked tasks with optional status filter.
//! task_list — list tracked tasks

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    #[serde(default)]
    status: Option<String>,
}

pub struct TaskListTool;

#[async_trait]
impl Tool for TaskListTool {
    fn name(&self) -> &str {
        "task_list"
    }

    fn description(&self) -> &str {
        "List all tracked tasks. Optionally filter by status (pending, running, completed, failed, killed)."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "status": {
                    "type": "string",
                    "enum": ["pending", "running", "completed", "failed", "killed"],
                    "description": "Filter tasks by status"
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

        // In a full implementation this would iterate over a shared task store.
        let filter_msg = match params.status {
            Some(ref s) => format!(" (filter: status={})", s),
            None => String::new(),
        };

        Ok(ToolOutput::text(format!(
            "No tasks found{}. The task store is not yet connected.",
            filter_msg
        )))
    }
}
