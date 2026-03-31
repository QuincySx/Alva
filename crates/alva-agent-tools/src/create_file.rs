// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: CreateFileTool
// POS:    Creates or overwrites a file with auto-creation of parent directories.
//! create_file — create or overwrite a file

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

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

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "create_file".into(),
            message: "local filesystem context required".into(),
        })?;
        let file_path = workspace.join(&params.path);
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // write_file handles parent directory creation internally
        let _ = params.create_dirs; // honoured by ToolFs::write_file unconditionally
        fs.write_file(file_path.to_str().unwrap_or_default(), params.content.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;

        Ok(ToolOutput::text(format!(
            "File created: {} ({} bytes)",
            file_path.display(),
            params.content.len()
        )))
    }
}
