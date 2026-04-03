// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TaskCreateTool
// POS:    Creates a new task for tracking work progress.
//! task_create — create a new tracked task

use alva_types::{
    AgentError, TaskType, Tool, ToolExecutionContext, ToolOutput,
    create_task_state,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Input {
    subject: String,
    description: String,
    #[serde(default)]
    metadata: Option<HashMap<String, Value>>,
}

pub struct TaskCreateTool;

#[async_trait]
impl Tool for TaskCreateTool {
    fn name(&self) -> &str {
        "task_create"
    }

    fn description(&self) -> &str {
        "Create a new task for tracking work progress. Returns the task ID and confirmation."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["subject", "description"],
            "properties": {
                "subject": {
                    "type": "string",
                    "description": "Short title / subject of the task"
                },
                "description": {
                    "type": "string",
                    "description": "Detailed description of the task"
                },
                "metadata": {
                    "type": "object",
                    "description": "Optional key-value metadata to attach to the task"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

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
