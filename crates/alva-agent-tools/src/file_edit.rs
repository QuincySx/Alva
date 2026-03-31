// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: FileEditTool
// POS:    Performs string-replace-based file editing requiring unique match of old_str.
//! file_edit — string-replace based file editing (like Claude Code's Edit tool)

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct SingleEdit {
    old_str: String,
    new_str: String,
}

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    #[serde(default)]
    old_str: Option<String>,
    #[serde(default)]
    new_str: Option<String>,
    #[serde(default)]
    edits: Option<Vec<SingleEdit>>,
}

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact string matches. Supports single edit (old_str+new_str) or batch edits (edits[] array). Each old_str must be unique in the file. Path is relative to workspace root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to workspace root"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact string to find (single edit mode)"
                },
                "new_str": {
                    "type": "string",
                    "description": "Replacement string (single edit mode)"
                },
                "edits": {
                    "type": "array",
                    "description": "Array of {old_str, new_str} for batch editing (alternative to single edit)",
                    "items": {
                        "type": "object",
                        "required": ["old_str", "new_str"],
                        "properties": {
                            "old_str": { "type": "string" },
                            "new_str": { "type": "string" }
                        }
                    }
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: e.to_string() })?;

        // Normalize to Vec<SingleEdit>
        let edits = if let Some(edits) = params.edits {
            if edits.is_empty() {
                return Ok(ToolOutput::error("edits array is empty"));
            }
            edits
        } else if let (Some(old_str), Some(new_str)) = (params.old_str, params.new_str) {
            vec![SingleEdit { old_str, new_str }]
        } else {
            return Ok(ToolOutput::error("Provide either old_str+new_str or edits[]"));
        };

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

        // Validate ALL edits against the ORIGINAL content before applying
        for edit in &edits {
            let count = content.matches(&edit.old_str).count();
            if count == 0 {
                return Ok(ToolOutput::error(format!("old_str not found: {:?}",
                    &edit.old_str[..edit.old_str.len().min(50)])));
            }
            if count > 1 {
                return Ok(ToolOutput::error(format!("old_str found {} times (must be unique): {:?}",
                    count, &edit.old_str[..edit.old_str.len().min(50)])));
            }
        }

        // Apply all edits and generate combined diff
        let mut current = content.clone();
        let mut combined_diff = String::new();
        let mut details_edits = Vec::new();

        for edit in &edits {
            // Find line number of change in current content
            let line_num = current[..current.find(&edit.old_str).unwrap()]
                .lines()
                .count() + 1;

            // Generate unified diff for this edit
            let old_lines: Vec<&str> = edit.old_str.lines().collect();
            let new_lines: Vec<&str> = edit.new_str.lines().collect();
            combined_diff.push_str(&format!("--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n",
                params.path, params.path,
                line_num, old_lines.len(),
                line_num, new_lines.len(),
            ));
            for line in &old_lines {
                combined_diff.push_str(&format!("-{}\n", line));
            }
            for line in &new_lines {
                combined_diff.push_str(&format!("+{}\n", line));
            }

            details_edits.push(json!({
                "first_changed_line": line_num,
                "old_lines": old_lines.len(),
                "new_lines": new_lines.len(),
            }));

            current = current.replacen(&edit.old_str, &edit.new_str, 1);
        }

        fs.write_file(file_path.to_str().unwrap_or_default(), current.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot write file: {}", e) })?;

        let edit_count = details_edits.len();
        let summary = if edit_count == 1 {
            let line = details_edits[0]["first_changed_line"].as_u64().unwrap_or(0);
            format!("File edited: {} (line {})", params.path, line)
        } else {
            format!("File edited: {} ({} edits applied)", params.path, edit_count)
        };

        Ok(ToolOutput {
            content: vec![ToolContent::text(format!("{}\n\n{}", summary, combined_diff))],
            is_error: false,
            details: Some(json!({
                "path": params.path,
                "edits": details_edits,
            })),
        })
    }
}
