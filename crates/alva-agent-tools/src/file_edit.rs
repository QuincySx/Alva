// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: FileEditTool
// POS:    Performs string-replace-based file editing requiring unique match of old_str.
//! file_edit — string-replace based file editing (like Claude Code's Edit tool)

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    old_str: String,
    new_str: String,
}

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing an exact string match (old_str) with a new string (new_str). The old_str must be unique in the file. Path is relative to workspace root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path", "old_str", "new_str"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to workspace root"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact string to find and replace (must be unique in file)"
                },
                "new_str": {
                    "type": "string",
                    "description": "Replacement string"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: e.to_string() })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "file_edit".into(),
            message: "local filesystem context required".into(),
        })?;
        let file_path = workspace.join(&params.path);
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let raw = fs
            .read_file(file_path.to_str().unwrap_or_default())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot read file: {}", e) })?;
        let content = String::from_utf8_lossy(&raw).into_owned();

        // Verify old_str is unique
        let count = content.matches(&params.old_str).count();
        if count == 0 {
            return Ok(ToolOutput::error("Error: old_str not found in file"));
        }
        if count > 1 {
            return Ok(ToolOutput::error(format!(
                "Error: old_str found {} times in file (must be unique)",
                count
            )));
        }

        let new_content = content.replacen(&params.old_str, &params.new_str, 1);
        fs.write_file(file_path.to_str().unwrap_or_default(), new_content.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot write file: {}", e) })?;

        Ok(ToolOutput::text(format!("File edited: {}", file_path.display())))
    }
}
