// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: NotebookEditTool
// POS:    Edits Jupyter notebook cells (replace, insert, or delete).
//! notebook_edit — edit Jupyter notebook cells

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

/// Cell type for notebook cells.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum CellType {
    Code,
    Markdown,
}

/// Edit operation kind.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum EditMode {
    Replace,
    Insert,
    Delete,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Path to the .ipynb notebook file.
    notebook_path: String,
    /// ID of the cell to edit (or position for insert).
    cell_id: String,
    /// New source content for the cell (required for replace/insert).
    #[serde(default)]
    new_source: Option<String>,
    /// Cell type (default: code).
    #[serde(default)]
    cell_type: Option<CellType>,
    /// Edit operation: replace content, insert a new cell, or delete.
    edit_mode: EditMode,
}

#[derive(Tool)]
#[tool(
    name = "notebook_edit",
    description = "Edit a Jupyter notebook cell. Supports replacing cell content, inserting a new \
        cell, or deleting an existing cell.",
    input = Input,
)]
pub struct NotebookEditTool;

impl NotebookEditTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "notebook_edit".into(),
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
            tool_name: "notebook_edit".into(),
            message: format!("Failed to read notebook: {}", e),
        })?;

        let mut notebook: Value = serde_json::from_slice(&data)
            .map_err(|e| AgentError::ToolError {
                tool_name: "notebook_edit".into(),
                message: format!("Invalid notebook JSON: {}", e),
            })?;

        let cells = notebook.get_mut("cells")
            .and_then(|c| c.as_array_mut())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "notebook_edit".into(),
                message: "Notebook has no 'cells' array".into(),
            })?;

        let cell_type_str = match params.cell_type {
            Some(CellType::Code) | None => "code",
            Some(CellType::Markdown) => "markdown",
        };

        let edit_mode_str = match params.edit_mode {
            EditMode::Replace => "replace",
            EditMode::Insert => "insert",
            EditMode::Delete => "delete",
        };

        match params.edit_mode {
            EditMode::Replace => {
                let new_source = params.new_source.ok_or_else(|| AgentError::ToolError {
                    tool_name: "notebook_edit".into(),
                    message: "new_source is required for replace mode".into(),
                })?;

                // Find cell by id
                let cell = cells.iter_mut().find(|c| {
                    c.get("id").and_then(|v| v.as_str()) == Some(&params.cell_id)
                }).ok_or_else(|| AgentError::ToolError {
                    tool_name: "notebook_edit".into(),
                    message: format!("Cell '{}' not found", params.cell_id),
                })?;

                // Update source
                let source_lines: Vec<Value> = new_source
                    .lines()
                    .map(|l| Value::String(format!("{}\n", l)))
                    .collect();
                cell["source"] = Value::Array(source_lines);
                cell["cell_type"] = Value::String(cell_type_str.to_string());
            }
            EditMode::Insert => {
                let new_source = params.new_source.ok_or_else(|| AgentError::ToolError {
                    tool_name: "notebook_edit".into(),
                    message: "new_source is required for insert mode".into(),
                })?;

                let source_lines: Vec<Value> = new_source
                    .lines()
                    .map(|l| Value::String(format!("{}\n", l)))
                    .collect();

                let new_cell = json!({
                    "id": params.cell_id,
                    "cell_type": cell_type_str,
                    "source": source_lines,
                    "metadata": {},
                    "outputs": [],
                    "execution_count": null
                });
                cells.push(new_cell);
            }
            EditMode::Delete => {
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
        }

        // Write back
        let output = serde_json::to_vec_pretty(&notebook)
            .map_err(|e| AgentError::ToolError {
                tool_name: "notebook_edit".into(),
                message: format!("Failed to serialize notebook: {}", e),
            })?;

        fs.write_file(path_str, &output).await.map_err(|e| AgentError::ToolError {
            tool_name: "notebook_edit".into(),
            message: format!("Failed to write notebook: {}", e),
        })?;

        Ok(ToolOutput::text(format!(
            "Notebook cell '{}' {}d in {}.",
            params.cell_id,
            edit_mode_str,
            params.notebook_path
        )))
    }
}
