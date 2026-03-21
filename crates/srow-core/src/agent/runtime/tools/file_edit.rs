// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, tokio::fs
// OUTPUT: FileEditTool
// POS:    Performs string-replace-based file editing requiring unique match of old_str.
//! file_edit — string-replace based file editing (like Claude Code's Edit tool)

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "file_edit".to_string(),
            description: "Edit a file by replacing an exact string match (old_str) with a new string (new_str). The old_str must be unique in the file. Path is relative to workspace root.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let file_path = ctx.workspace.join(&params.path);

        let content = tokio::fs::read_to_string(&file_path)
            .await
            .map_err(|e| EngineError::ToolExecution(format!("Cannot read file: {}", e)))?;

        // Verify old_str is unique
        let count = content.matches(&params.old_str).count();
        if count == 0 {
            return Ok(ToolResult {
                tool_call_id: String::new(),
                tool_name: "file_edit".to_string(),
                output: "Error: old_str not found in file".to_string(),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }
        if count > 1 {
            return Ok(ToolResult {
                tool_call_id: String::new(),
                tool_name: "file_edit".to_string(),
                output: format!(
                    "Error: old_str found {} times in file (must be unique)",
                    count
                ),
                is_error: true,
                duration_ms: start.elapsed().as_millis() as u64,
            });
        }

        let new_content = content.replacen(&params.old_str, &params.new_str, 1);
        tokio::fs::write(&file_path, &new_content)
            .await
            .map_err(|e| EngineError::ToolExecution(format!("Cannot write file: {}", e)))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "file_edit".to_string(),
            output: format!("File edited: {}", file_path.display()),
            is_error: false,
            duration_ms,
        })
    }
}
