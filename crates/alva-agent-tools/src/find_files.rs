// INPUT:  alva_types, async_trait, glob, serde, serde_json, crate::local_fs::walk_dir_filtered
// OUTPUT: FindFilesTool
// POS:    Search for files by glob pattern across the workspace, respecting .gitignore-like rules.
//         Results sorted by modification time (most recent first) with configurable limits
//         and relative path output.
//! find_files — search file paths by glob pattern

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// Default maximum results returned.
const DEFAULT_MAX_RESULTS: usize = 100;
/// Hard cap on maximum results to prevent context overflow.
const HARD_MAX_RESULTS: usize = 1000;

#[derive(Debug, Deserialize)]
struct Input {
    pattern: String,
    path: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
    /// Sort results by modification time (most recent first). Default: true.
    #[serde(default)]
    sort_by_mtime: Option<bool>,
}

/// File path with modification time metadata.
struct FileEntry {
    rel_path: String,
    #[allow(unused)]
    mtime: Option<std::time::SystemTime>,
}

pub struct FindFilesTool;

#[async_trait]
impl Tool for FindFilesTool {
    fn name(&self) -> &str {
        "find_files"
    }

    fn description(&self) -> &str {
        "Search for files by glob pattern (e.g. '*.rs', 'src/**/*.ts', '*test*'). \
         Returns matching file paths relative to the workspace root, \
         sorted by modification time (most recent first). \
         Respects .gitignore rules. Default limit: 100 files."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["pattern"],
            "properties": {
                "pattern": {
                    "type": "string",
                    "description": "Glob pattern to match file paths (e.g. '*.rs', 'src/**/*.ts', '*controller*')"
                },
                "path": {
                    "type": "string",
                    "description": "Directory to search in, relative to workspace root. Default: workspace root"
                },
                "max_results": {
                    "type": "integer",
                    "description": "Maximum number of results to return (default: 100, max: 1000)"
                },
                "sort_by_mtime": {
                    "type": "boolean",
                    "description": "Sort results by modification time, most recent first (default: true)"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input).map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: e.to_string(),
        })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "workspace required".into(),
        })?;

        let search_root = params
            .path
            .map(|p| workspace.join(p))
            .unwrap_or_else(|| workspace.to_path_buf());
        let search_root_str = search_root.to_str().unwrap_or_default();

        let max_results = params.max_results.unwrap_or(DEFAULT_MAX_RESULTS).min(HARD_MAX_RESULTS);
        let sort_by_mtime = params.sort_by_mtime.unwrap_or(true);

        // Parse glob pattern
        let glob = glob::Pattern::new(&params.pattern).map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Invalid glob pattern '{}': {}", params.pattern, e),
        })?;

        // Walk directory tree (hidden files excluded by default, .gitignore respected)
        let all_paths = crate::local_fs::walk_dir_filtered(search_root_str, None, false)?;

        // Match glob against relative path and file name, collecting entries
        let workspace_str = workspace.to_str().unwrap_or_default();
        let mut entries: Vec<FileEntry> = Vec::new();

        for full_path in &all_paths {
            // Get relative path from workspace root
            let rel_path = full_path
                .strip_prefix(workspace_str)
                .unwrap_or(full_path)
                .trim_start_matches('/');

            // Match against full relative path OR just the file name
            let file_name = std::path::Path::new(rel_path)
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("");

            if glob.matches(rel_path) || glob.matches(file_name) {
                // Get modification time
                let mtime = if sort_by_mtime {
                    std::fs::metadata(full_path)
                        .ok()
                        .and_then(|m| m.modified().ok())
                } else {
                    None
                };

                entries.push(FileEntry {
                    rel_path: rel_path.to_string(),
                    mtime,
                });
            }
        }

        // Sort by modification time (most recent first) or alphabetically
        if sort_by_mtime {
            entries.sort_by(|a, b| {
                // Most recent first — reverse order
                match (&b.mtime, &a.mtime) {
                    (Some(b_time), Some(a_time)) => b_time.cmp(a_time),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => a.rel_path.cmp(&b.rel_path),
                }
            });
        } else {
            entries.sort_by(|a, b| a.rel_path.cmp(&b.rel_path));
        }

        let total_found = entries.len();
        let capped = total_found > max_results;
        let display_entries: Vec<&str> = entries.iter()
            .take(max_results)
            .map(|e| e.rel_path.as_str())
            .collect();

        let content = if display_entries.is_empty() {
            format!("No files found matching '{}'", params.pattern)
        } else {
            display_entries.join("\n")
        };

        let mut output_content = content;
        if capped {
            output_content.push_str(&format!(
                "\n\n[Showing {} of {} results. Narrow your pattern for more specific results.]",
                max_results, total_found
            ));
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(output_content)],
            is_error: false,
            details: Some(json!({
                "pattern": params.pattern,
                "total_found": total_found,
                "shown": display_entries.len(),
                "capped": capped,
                "sorted_by": if sort_by_mtime { "mtime" } else { "name" },
            })),
        })
    }
}
