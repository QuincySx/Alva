// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskGetTool
// POS:    Retrieves full details of a tracked task by ID.
//! task_get — retrieve task details

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The task ID to look up.
    task_id: String,
}

#[derive(Tool)]
#[tool(
    name = "task_get",
    description = "Retrieve the full details of a task by its ID, including status, description, \
        and any output produced so far.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskGetTool;

impl TaskGetTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation this would look up TaskState from a shared store.
        // For now, return a placeholder indicating the task ID was requested.
        Ok(ToolOutput::text(format!(
            "Task {} — details not found. The task store is not yet connected.\n\
             Hint: Tasks are tracked in-memory during agent sessions.",
            params.task_id
        )))
    }
}
