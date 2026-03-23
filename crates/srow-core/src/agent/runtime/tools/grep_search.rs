// INPUT:  agent_types, async_trait, regex, serde, serde_json, walkdir
// OUTPUT: GrepSearchTool
// POS:    Searches for regex patterns across workspace files with glob filtering and line-level results.
//! grep_search — regex search across files

use agent_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};
use walkdir::WalkDir;

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

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: e.to_string() })?;

        let search_root = params
            .path
            .map(|p| ctx.workspace().join(p))
            .unwrap_or_else(|| ctx.workspace().to_path_buf());

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

        // Walk directory and search (blocking, run in spawn_blocking)
        let matches = tokio::task::spawn_blocking(move || {
            let mut results: Vec<GrepMatch> = Vec::new();

            for entry in WalkDir::new(&search_root)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_type().is_file())
            {
                if results.len() >= max_results {
                    break;
                }

                let path = entry.path();

                // Apply glob filter
                if let Some(ref glob) = glob_pattern {
                    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                        if !glob.matches(name) {
                            continue;
                        }
                    }
                }

                // Skip binary files (heuristic: try reading as UTF-8)
                if let Ok(content) = std::fs::read_to_string(path) {
                    for (line_num, line) in content.lines().enumerate() {
                        if results.len() >= max_results {
                            break;
                        }
                        if regex.is_match(line) {
                            results.push(GrepMatch {
                                file: path.display().to_string(),
                                line: line_num + 1,
                                content: line.to_string(),
                            });
                        }
                    }
                }
            }

            results
        })
        .await
        .map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: e.to_string() })?;

        Ok(ToolResult {
            content: serde_json::to_string_pretty(&matches)
                .unwrap_or_else(|_| "[]".to_string()),
            is_error: false,
            details: None,
        })
    }
}
