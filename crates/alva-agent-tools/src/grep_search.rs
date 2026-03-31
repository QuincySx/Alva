// INPUT:  alva_types, async_trait, regex, serde, serde_json, crate::local_fs::{LocalToolFs, walk_dir}
// OUTPUT: GrepSearchTool
// POS:    Searches for regex patterns across workspace files with glob filtering and line-level results.
//! grep_search — regex search across files

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::{walk_dir, LocalToolFs};

#[derive(Debug, Deserialize)]
struct Input {
    pattern: String,
    path: Option<String>,
    file_pattern: Option<String>,
    case_insensitive: Option<bool>,
    max_results: Option<usize>,
}

#[derive(Debug, serde::Serialize)]
struct GrepMatch {
    file: String,
    line: usize,
    content: String,
}

pub struct GrepSearchTool;

#[async_trait]
impl Tool for GrepSearchTool {
    fn name(&self) -> &str {
        "grep_search"
    }

    fn description(&self) -> &str {
        "Search for a regex pattern across files in the workspace. Returns matching lines with file paths and line numbers."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Regular expression pattern to search for"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to workspace root"
                },
                "file_pattern": {
                    "type": "string",
                    "description": "Glob pattern to filter files, e.g. '*.rs'"
                },
                "case_insensitive": {
                    "type": "boolean",
                    "description": "Case insensitive search, default false"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of matches to return, default 100"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: e.to_string() })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "grep_search".into(),
            message: "local filesystem context required".into(),
        })?;
        let search_root = params
            .path
            .map(|p| workspace.join(p))
            .unwrap_or_else(|| workspace.to_path_buf());

        let max_results = params.max_results.unwrap_or(100);

        // Build regex
        let pattern_str = if params.case_insensitive.unwrap_or(false) {
            format!("(?i){}", params.pattern)
        } else {
            params.pattern.clone()
        };

        let regex = Regex::new(&pattern_str)
            .map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: format!("Invalid regex: {}", e) })?;

        // Optional glob filter
        let glob_pattern = params.file_pattern.as_ref().map(|p| {
            glob::Pattern::new(p).unwrap_or_else(|_| glob::Pattern::new("*").unwrap())
        });

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // Walk directory to get all file paths (hidden files excluded)
        let search_root_str = search_root.to_str().unwrap_or_default();
        let file_paths = walk_dir(fs, search_root_str, None, false).await?;

        let mut results: Vec<GrepMatch> = Vec::new();

        for file_path in file_paths {
            if results.len() >= max_results {
                break;
            }

            // Apply glob filter on file name
            if let Some(ref glob) = glob_pattern {
                if let Some(name) = std::path::Path::new(&file_path).file_name().and_then(|n| n.to_str()) {
                    if !glob.matches(name) {
                        continue;
                    }
                }
            }

            // Read file via ToolFs; skip binary / unreadable files
            let bytes = match fs.read_file(&file_path).await {
                Ok(b) => b,
                Err(_) => continue,
            };
            let content = String::from_utf8_lossy(&bytes);

            for (line_num, line) in content.lines().enumerate() {
                if results.len() >= max_results {
                    break;
                }
                if regex.is_match(line) {
                    results.push(GrepMatch {
                        file: file_path.clone(),
                        line: line_num + 1,
                        content: line.to_string(),
                    });
                }
            }
        }

        Ok(ToolOutput::text(serde_json::to_string_pretty(&results)
            .unwrap_or_else(|_| "[]".to_string())))
    }
}
