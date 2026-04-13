// INPUT:  alva_types, async_trait, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ListFilesTool
// POS:    Lists directory contents with recursive traversal and hidden file filtering via ToolFs.
//! list_files — list directory contents

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::local_fs::LocalToolFs;

/// Maximum entries returned to prevent context overflow.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Directory path relative to workspace root, defaults to workspace root.
    #[serde(default)]
    path: Option<String>,
    /// List recursively, default false.
    #[serde(default)]
    recursive: Option<bool>,
    /// Max recursion depth, default 3.
    #[serde(default)]
    max_depth: Option<usize>,
    /// Show hidden files (starting with .), default false.
    #[serde(default)]
    show_hidden: Option<bool>,
}

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

#[derive(Tool)]
#[tool(
    name = "list_files",
    description = "List files and directories in the given path. Returns a tree-like listing of the directory contents.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct ListFilesTool;

impl ListFilesTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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

        let total = entries.len();
        let truncated = total > MAX_ENTRIES;
        if truncated {
            entries.truncate(MAX_ENTRIES);
        }
        let mut content = entries.join("\n");
        if truncated {
            content.push_str(&format!(
                "\n\n[Showing {} of {} entries. Use a more specific path.]",
                MAX_ENTRIES, total
            ));
        }
        Ok(ToolOutput {
            content: vec![ToolContent::text(content)],
            is_error: false,
            details: Some(json!({
                "total_entries": total,
                "shown": total.min(MAX_ENTRIES),
                "truncated": truncated,
            })),
        })
    }
}
