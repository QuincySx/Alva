// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, tokio::fs
// OUTPUT: CreateFileTool
// POS:    Creates or overwrites a file with auto-creation of parent directories.
//! create_file — create or overwrite a file

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
    content: String,
    create_dirs: Option<bool>,
}

pub struct CreateFileTool;

#[async_trait]
impl Tool for CreateFileTool {
    fn name(&self) -> &str {
        "create_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "create_file".to_string(),
            description: "Create a new file or overwrite an existing file with the given content. The path is relative to the workspace root.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let file_path = ctx.workspace.join(&params.path);
        let create_dirs = params.create_dirs.unwrap_or(true);

        if create_dirs {
            if let Some(parent) = file_path.parent() {
                tokio::fs::create_dir_all(parent)
                    .await
                    .map_err(|e| EngineError::ToolExecution(e.to_string()))?;
            }
        }

        tokio::fs::write(&file_path, &params.content)
            .await
            .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "create_file".to_string(),
            output: format!(
                "File created: {} ({} bytes)",
                file_path.display(),
                params.content.len()
            ),
            is_error: false,
            duration_ms,
        })
    }
}
