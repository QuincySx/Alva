// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, walkdir
// OUTPUT: ListFilesTool
// POS:    Lists directory contents with recursive traversal and hidden file filtering.
//! list_files — list directory contents

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;
use walkdir::WalkDir;

#[derive(Debug, Deserialize)]
struct Input {
    path: Option<String>,
    recursive: Option<bool>,
    max_depth: Option<usize>,
    show_hidden: Option<bool>,
}

pub struct ListFilesTool;

#[async_trait]
impl Tool for ListFilesTool {
    fn name(&self) -> &str {
        "list_files"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_files".to_string(),
            description: "List files and directories in the given path. Returns a tree-like listing of the directory contents.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();
        let target = params
            .path
            .map(|p| ctx.workspace.join(p))
            .unwrap_or_else(|| ctx.workspace.clone());

        let recursive = params.recursive.unwrap_or(false);
        let max_depth = if recursive {
            params.max_depth.unwrap_or(3)
        } else {
            1
        };
        let show_hidden = params.show_hidden.unwrap_or(false);

        let entries = tokio::task::spawn_blocking(move || {
            let mut files: Vec<String> = Vec::new();

            for entry in WalkDir::new(&target)
                .max_depth(max_depth)
                .into_iter()
                .filter_map(|e| e.ok())
            {
                let path = entry.path();

                // Skip hidden files unless requested
                if !show_hidden {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if name.starts_with('.') && path != target {
                            continue;
                        }
                    }
                }

                let display = if path == target {
                    ".".to_string()
                } else {
                    path.strip_prefix(&target)
                        .unwrap_or(path)
                        .display()
                        .to_string()
                };

                let suffix = if entry.file_type().is_dir() {
                    "/"
                } else {
                    ""
                };

                if display != "." {
                    files.push(format!("{}{}", display, suffix));
                }
            }

            files.sort();
            files
        })
        .await
        .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "list_files".to_string(),
            output: entries.join("\n"),
            is_error: false,
            duration_ms,
        })
    }
}
