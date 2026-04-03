// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TaskStopTool
// POS:    Stops/cancels a running task.
//! task_stop — stop or cancel a running task

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    task_id: String,
}

pub struct TaskStopTool;

#[async_trait]
impl Tool for TaskStopTool {
    fn name(&self) -> &str {
        "task_stop"
    }

    fn description(&self) -> &str {
        "Stop or cancel a running task. The task will be marked as killed."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["task_id"],
            "properties": {
                "task_id": {
                    "type": "string",
                    "description": "The task ID to stop"
                }
            }
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
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

        // In a full implementation, this would:
        // 1. Look up the task in the shared store
        // 2. Cancel its CancellationToken
        // 3. Update status to Killed
        Ok(ToolOutput::text(format!(
            "Task {} stop requested. The task store is not yet connected.",
            params.task_id
        )))
    }
}
