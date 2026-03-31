// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ListFilesTool
// POS:    Lists directory contents with recursive traversal and hidden file filtering via ToolFs.
//! list_files — list directory contents

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    path: Option<String>,
    recursive: Option<bool>,
    max_depth: Option<usize>,
    show_hidden: Option<bool>,
}

pub struct ListFilesTool;

/// Recursively collect entries (files and directories) via ToolFs,
/// returning relative paths with a trailing `/` for directories.
///
/// Uses `Box::pin` to allow async recursion.
fn list_entries<'a>(
    fs: &'a dyn alva_types::ToolFs,
    root: &'a str,
    prefix: &'a str,
    depth: usize,
    max_depth: usize,
    show_hidden: bool,
) -> futures::future::BoxFuture<'a, Result<Vec<String>, AgentError>> {
    Box::pin(async move {
        let mut results = Vec::new();
        let entries = fs.list_dir(root).await?;
        for entry in entries {
            if !show_hidden && entry.name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", prefix, entry.name)
            };
            if entry.is_dir {
                results.push(format!("{}/", rel));
                if depth < max_depth {
                    let child_path = format!("{}/{}", root.trim_end_matches('/'), entry.name);
                    let sub = list_entries(fs, &child_path, &rel, depth + 1, max_depth, show_hidden).await?;
                    results.extend(sub);
                }
            } else {
                results.push(rel);
            }
        }
        Ok(results)
    })
}

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn description(&self) -> &str {
        "List files and directories in the given path. Returns a tree-like listing of the directory contents."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "path": {
                    "type": "string",
                    "description": "Directory path relative to workspace root, defaults to workspace root"
                },
                "recursive": {
                    "type": "boolean",
                    "description": "List recursively, default false"
                },
                "max_depth": {
                    "type": "integer",
                    "description": "Max recursion depth, default 3"
                },
                "show_hidden": {
                    "type": "boolean",
                    "description": "Show hidden files (starting with .), default false"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "list_files".into(), message: e.to_string() })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "list_files".into(),
            message: "local filesystem context required".into(),
        })?;
        let target = params
            .path
            .map(|p| workspace.join(p))
            .unwrap_or_else(|| workspace.to_path_buf());

        let recursive = params.recursive.unwrap_or(false);
        let max_depth = if recursive {
            params.max_depth.unwrap_or(3)
        } else {
            1
        };
        let show_hidden = params.show_hidden.unwrap_or(false);

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let target_str = target.to_str().unwrap_or_default();
        let mut entries = list_entries(fs, target_str, "", 1, max_depth, show_hidden).await?;
        entries.sort();

        Ok(ToolOutput::text(entries.join("\n")))
    }
}
