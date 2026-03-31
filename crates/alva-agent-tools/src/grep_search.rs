// INPUT:  alva_types, async_trait, regex, serde, serde_json, crate::local_fs::walk_dir_filtered, crate::truncate
// OUTPUT: GrepSearchTool
// POS:    Searches for regex patterns across workspace files with glob filtering, context lines, and truncated output.
//! grep_search — regex search across files

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use regex::Regex;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::walk_dir_filtered;
use crate::truncate::{truncate_head, truncate_line, MAX_BYTES, MAX_LINES, MAX_LINE_LENGTH};

#[derive(Debug, Deserialize)]
struct Input {
    pattern: String,
    path: Option<String>,
    file_pattern: Option<String>,
    case_insensitive: Option<bool>,
    literal: Option<bool>,
    context: Option<usize>,
    max_results: Option<usize>,
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
                "literal": {
                    "type": "boolean",
                    "description": "Treat pattern as literal string instead of regex, default false"
                },
                "context": {
                    "type": "integer",
                    "description": "Number of context lines before and after each match, default 0"
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
        let workspace_str = workspace.to_str().unwrap_or_default();

        let search_root = params
            .path
            .as_ref()
            .map(|p| workspace.join(p))
            .unwrap_or_else(|| workspace.to_path_buf());

        let max_results = params.max_results.unwrap_or(100);
        let context_lines = params.context.unwrap_or(0);

        // Build regex — escape if literal mode
        let pattern_str = if params.literal.unwrap_or(false) {
            regex::escape(&params.pattern)
        } else {
            params.pattern.clone()
        };
        let pattern_str = if params.case_insensitive.unwrap_or(false) {
            format!("(?i){}", pattern_str)
        } else {
            pattern_str
        };

        let regex = Regex::new(&pattern_str)
            .map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: format!("Invalid regex: {}", e) })?;

        // Optional glob filter
        let glob_pattern = params.file_pattern.as_ref().map(|p| {
            glob::Pattern::new(p).unwrap_or_else(|_| glob::Pattern::new("*").unwrap())
        });

        // Walk directory with .gitignore support (synchronous)
        let search_root_str = search_root.to_str().unwrap_or_default();
        let file_paths = walk_dir_filtered(search_root_str, None, false)?;

        let mut match_count = 0;
        let mut output_lines: Vec<String> = Vec::new();

        for file_path in file_paths {
            if match_count >= max_results {
                break;
            }

            // Get relative path from workspace for glob matching and display
            let rel_path = file_path
                .strip_prefix(workspace_str)
                .unwrap_or(&file_path)
                .trim_start_matches('/');

            // Match glob against full relative path (not just filename)
            if let Some(ref glob) = glob_pattern {
                if !glob.matches(rel_path) {
                    continue;
                }
            }

            // Read file; skip binary / unreadable files
            let bytes = match std::fs::read(&file_path) {
                Ok(b) => b,
                Err(_) => continue,
            };
            let content = match std::str::from_utf8(&bytes) {
                Ok(s) => s,
                Err(_) => continue, // skip non-UTF8 (binary) files
            };

            let lines: Vec<&str> = content.lines().collect();
            let total_lines = lines.len();

            // Find all matching line indices in this file
            let mut match_indices: Vec<usize> = Vec::new();
            for (idx, line) in lines.iter().enumerate() {
                if regex.is_match(line) {
                    match_indices.push(idx);
                    match_count += 1;
                    if match_count >= max_results {
                        break;
                    }
                }
            }

            if match_indices.is_empty() {
                continue;
            }

            // Build the set of line indices to display (matches + context), dedup via sorted set
            let mut display_indices: Vec<usize> = Vec::new();
            for &m in &match_indices {
                let start = m.saturating_sub(context_lines);
                let end = std::cmp::min(m + context_lines, total_lines.saturating_sub(1));
                for i in start..=end {
                    display_indices.push(i);
                }
            }
            display_indices.sort_unstable();
            display_indices.dedup();

            // Convert matching indices to a set for fast lookup
            let match_set: std::collections::HashSet<usize> = match_indices.into_iter().collect();

            // Emit output lines
            let mut prev_idx: Option<usize> = None;
            for &idx in &display_indices {
                // Insert separator for non-contiguous ranges
                if let Some(prev) = prev_idx {
                    if idx > prev + 1 {
                        output_lines.push("--".to_string());
                    }
                }
                prev_idx = Some(idx);

                let line_num = idx + 1; // 1-based
                let truncated = truncate_line(lines[idx], MAX_LINE_LENGTH);
                if match_set.contains(&idx) {
                    // Match line: path:linenum: content
                    output_lines.push(format!("{}:{}:{}", rel_path, line_num, truncated));
                } else {
                    // Context line: path-linenum- content
                    output_lines.push(format!("{}-{}-{}", rel_path, line_num, truncated));
                }
            }
        }

        if match_count == 0 {
            return Ok(ToolOutput::text("No matches found."));
        }

        // Join all output, then apply head truncation
        let raw_output = output_lines.join("\n");
        let tr = truncate_head(&raw_output, MAX_LINES, MAX_BYTES);
        let final_output = if tr.truncated {
            format!(
                "{}\n\n[truncated: showing {}/{} lines]",
                tr.text, tr.shown_lines, tr.total_lines
            )
        } else {
            tr.text
        };

        Ok(ToolOutput::text(final_output))
    }
}
