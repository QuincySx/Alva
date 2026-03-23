// INPUT:  agent_types, async_trait, serde, serde_json, tokio::fs
// OUTPUT: CreateFileTool
// POS:    Creates or overwrites a file with auto-creation of parent directories.
//! create_file — create or overwrite a file

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    content: String,
    create_dirs: Option<bool>,
}

pub struct CreateFileTool;

#[async_trait]
impl Tool for CreateFileTool {
    fn name(&self) -> &str {
        "create_file"
    }

    fn description(&self) -> &str {
        "Create a new file or overwrite an existing file with the given content. The path is relative to the workspace root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path", "content"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to workspace root"
                },
                "content": {
                    "type": "string",
                    "description": "File content to write"
                },
                "create_dirs": {
                    "type": "boolean",
                    "description": "Auto-create parent directories, default true"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;

        let file_path = ctx.workspace().join(&params.path);
        let create_dirs = params.create_dirs.unwrap_or(true);

        if create_dirs {
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;
            }
        }

        tokio::fs::write(&file_path, &params.content)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;

        Ok(ToolResult {
            content: format!(
                "File created: {} ({} bytes)",
                file_path.display(),
                params.content.len()
            ),
            is_error: false,
            details: None,
        })
    }
}
