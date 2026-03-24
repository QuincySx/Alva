// INPUT:  alva_types, async_trait, serde, serde_json, tokio::fs
// OUTPUT: FileEditTool
// POS:    Performs string-replace-based file editing requiring unique match of old_str.
//! file_edit — string-replace based file editing (like Claude Code's Edit tool)

use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

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

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: e.to_string() })?;

        let local = ctx.local().ok_or_else(|| AgentError::ToolError {
            tool_name: "file_edit".into(),
            message: "local filesystem context required".into(),
        })?;
        let file_path = local.workspace().join(&params.path);

        let content = tokio::fs::read_to_string(&file_path)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot read file: {}", e) })?;

        // Verify old_str is unique
        let count = content.matches(&params.old_str).count();
        if count == 0 {
            return Ok(ToolResult {
                content: "Error: old_str not found in file".to_string(),
                is_error: true,
                details: None,
            });
        }
        if count > 1 {
            return Ok(ToolResult {
                content: format!(
                    "Error: old_str found {} times in file (must be unique)",
                    count
                ),
                is_error: true,
                details: None,
            });
        }

        let new_content = content.replacen(&params.old_str, &params.new_str, 1);
        tokio::fs::write(&file_path, &new_content)
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot write file: {}", e) })?;

        Ok(ToolResult {
            content: format!("File edited: {}", file_path.display()),
            is_error: false,
            details: None,
        })
    }
}
