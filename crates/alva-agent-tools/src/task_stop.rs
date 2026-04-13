// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskStopTool
// POS:    Stops/cancels a running task.
//! task_stop — stop or cancel a running task

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

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
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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
