// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: TodoWriteTool
// POS:    Writes progress notes to a file (defaults to CLAUDE.md).
//! todo_write — write progress notes

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    content: String,
    #[serde(default)]
    file_path: Option<String>,
}

pub struct TodoWriteTool;

#[async_trait]
impl Tool for TodoWriteTool {
    fn name(&self) -> &str {
        "todo_write"
    }

    fn description(&self) -> &str {
        "Write progress notes or TODO items to a tracking file. \
         Defaults to CLAUDE.md in the workspace root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["content"],
            "properties": {
                "content": {
                    "type": "string",
                    "description": "Content to write (progress notes, TODO items, etc.)"
                },
                "file_path": {
                    "type": "string",
                    "description": "File path to write to (default: CLAUDE.md)"
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

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "workspace context required".into(),
        })?;

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let file_name = params.file_path.as_deref().unwrap_or("CLAUDE.md");
        let target = if std::path::Path::new(file_name).is_absolute() {
            std::path::PathBuf::from(file_name)
        } else {
            workspace.join(file_name)
        };
        let path_str = target.to_str().unwrap_or_default();

        // Read existing content (if any), then append
        let existing = match fs.read_file(path_str).await {
            Ok(data) => String::from_utf8_lossy(&data).to_string(),
            Err(_) => String::new(),
        };

        let mut new_content = existing;
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(&params.content);
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }

        fs.write_file(path_str, new_content.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("Failed to write: {}", e),
            })?;

        Ok(ToolOutput::text(format!(
            "Written to {} ({} bytes).",
            target.display(),
            params.content.len()
        )))
    }
}
