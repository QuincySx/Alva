// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TaskListTool
// POS:    Lists all tracked tasks with optional status filter.
//! task_list — list tracked tasks

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// Task status filter for the list query.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum StatusFilter {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Filter tasks by status.
    #[serde(default)]
    status: Option<StatusFilter>,
}

#[derive(Tool)]
#[tool(
    name = "task_list",
    description = "List all tracked tasks. Optionally filter by status (pending, running, completed, failed, killed).",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct TaskListTool;

impl TaskListTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation this would iterate over a shared task store.
        let filter_msg = match params.status {
            Some(StatusFilter::Pending) => " (filter: status=pending)".to_string(),
            Some(StatusFilter::Running) => " (filter: status=running)".to_string(),
            Some(StatusFilter::Completed) => " (filter: status=completed)".to_string(),
            Some(StatusFilter::Failed) => " (filter: status=failed)".to_string(),
            Some(StatusFilter::Killed) => " (filter: status=killed)".to_string(),
            None => String::new(),
        };

        Ok(ToolOutput::text(format!(
            "No tasks found{}. The task store is not yet connected.",
            filter_msg
        )))
    }
}
