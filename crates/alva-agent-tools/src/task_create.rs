// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json
// OUTPUT: TaskCreateTool
// POS:    Creates a new task for tracking work progress.
//! task_create — create a new tracked task

use alva_kernel_abi::{
    AgentError, TaskType, Tool, ToolExecutionContext, ToolOutput,
    create_task_state,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Short title / subject of the task.
    subject: String,
    /// Detailed description of the task.
    description: String,
    /// Optional key-value metadata to attach to the task.
    #[serde(default)]
    metadata: Option<HashMap<String, Value>>,
}

#[derive(Tool)]
#[tool(
    name = "task_create",
    description = "Create a new task for tracking work progress. Returns the task ID and confirmation.",
    input = Input,
)]
pub struct TaskCreateTool;

impl TaskCreateTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let output_dir = ctx
            .workspace()
            .map(|w| w.join(".tasks"))
            .unwrap_or_else(|| PathBuf::from("/tmp/.tasks"));

        let full_description = format!("{}: {}", params.subject, params.description);
        let state = create_task_state(
            TaskType::LocalAgent,
            full_description,
            None,
            output_dir.join("output.log"),
        );

        let task_id = state.id.clone();

        let mut result = format!(
            "Task created successfully.\n  ID: {}\n  Subject: {}\n  Description: {}",
            task_id, params.subject, params.description
        );

        if let Some(ref meta) = params.metadata {
            result.push_str(&format!("\n  Metadata: {:?}", meta));
        }

        Ok(ToolOutput::text(result))
    }
}
