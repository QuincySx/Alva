// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: TaskOutputTool
// POS:    Retrieves the output/results of a task from its output file.
//! task_output — get task output content

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The task ID whose output to retrieve.
    task_id: String,
}

#[derive(Tool)]
#[tool(
    name = "task_output",
    description = "Get the output or results produced by a task. This reads from the task's output file.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskOutputTool;

impl TaskOutputTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation, this would look up the task's output_file
        // from the shared task store and read its contents.
        Ok(ToolOutput::text(format!(
            "No output available for task {}. The task store is not yet connected.",
            params.task_id
        )))
    }
}
