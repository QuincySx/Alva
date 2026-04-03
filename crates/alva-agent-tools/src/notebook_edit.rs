// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: NotebookEditTool
// POS:    Edits Jupyter notebook cells (replace, insert, or delete).
//! notebook_edit — edit Jupyter notebook cells

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    notebook_path: String,
    cell_id: String,
    #[serde(default)]
    new_source: Option<String>,
    #[serde(default)]
    cell_type: Option<String>,
    edit_mode: String,
}

pub struct NotebookEditTool;

#[async_trait]
impl Tool for NotebookEditTool {
    fn name(&self) -> &str {
        "notebook_edit"
    }

    fn description(&self) -> &str {
        "Edit a Jupyter notebook cell. Supports replacing cell content, inserting a new \
         cell, or deleting an existing cell."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["notebook_path", "cell_id", "edit_mode"],
            "properties": {
                "notebook_path": {
                    "type": "string",
                    "description": "Path to the .ipynb notebook file"
                },
                "cell_id": {
                    "type": "string",
                    "description": "ID of the cell to edit (or position for insert)"
                },
                "new_source": {
                    "type": "string",
                    "description": "New source content for the cell (required for replace/insert)"
                },
                "cell_type": {
                    "type": "string",
                    "enum": ["code", "markdown"],
                    "description": "Cell type (default: code)"
                },
                "edit_mode": {
                    "type": "string",
                    "enum": ["replace", "insert", "delete"],
                    "description": "Edit operation: replace content, insert a new cell, or delete"
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

        // Resolve notebook path
        let nb_path = if std::path::Path::new(&params.notebook_path).is_absolute() {
            std::path::PathBuf::from(&params.notebook_path)
        } else {
            workspace.join(&params.notebook_path)
        };
        let path_str = nb_path.to_str().unwrap_or_default();

        // Read notebook
        let data = fs.read_file(path_str).await.map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Failed to read notebook: {}", e),
        })?;

        let mut notebook: Value = serde_json::from_slice(&data)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("Invalid notebook JSON: {}", e),
            })?;

        let cells = notebook.get_mut("cells")
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: self.name().into(),
                message: "Notebook has no 'cells' array".into(),
            })?;

        let cell_type = params.cell_type.as_deref().unwrap_or("code");

        match params.edit_mode.as_str() {
            "replace" => {
                let new_source = params.new_source.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "new_source is required for replace mode".into(),
                })?;

                // Find cell by id
                let cell = cells.iter_mut().find(|c| {
                    c.get("id").and_then(|v| v.as_str()) == Some(&params.cell_id)
                }).ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: format!("Cell '{}' not found", params.cell_id),
                })?;

                // Update source
                let source_lines: Vec<Value> = new_source
                    .lines()
                    .map(|l| Value::String(format!("{}\n", l)))
                    .collect();
                cell["source"] = Value::Array(source_lines);
                cell["cell_type"] = Value::String(cell_type.to_string());
            }
            "insert" => {
                let new_source = params.new_source.ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "new_source is required for insert mode".into(),
                })?;

                let source_lines: Vec<Value> = new_source
                    .lines()
                    .map(|l| Value::String(format!("{}\n", l)))
                    .collect();

                let new_cell = json!({
                    "id": params.cell_id,
                    "cell_type": cell_type,
                    "source": source_lines,
                    "metadata": {},
                    "outputs": [],
                    "execution_count": null
                });
                cells.push(new_cell);
            }
            "delete" => {
                let initial_len = cells.len();
                cells.retain(|c| {
                    c.get("id").and_then(|v| v.as_str()) != Some(&params.cell_id)
                });
                if cells.len() == initial_len {
                    return Ok(ToolOutput::error(format!(
                        "Cell '{}' not found in notebook",
                        params.cell_id
                    )));
                }
            }
            other => {
                return Ok(ToolOutput::error(format!(
                    "Invalid edit_mode '{}'. Must be replace, insert, or delete.",
                    other
                )));
            }
        }

        // Write back
        let output = serde_json::to_vec_pretty(&notebook)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("Failed to serialize notebook: {}", e),
            })?;

        fs.write_file(path_str, &output).await.map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Failed to write notebook: {}", e),
        })?;

        Ok(ToolOutput::text(format!(
            "Notebook cell '{}' {}d in {}.",
            params.cell_id,
            params.edit_mode,
            params.notebook_path
        )))
    }
}
