// INPUT:  alva_types, async_trait, glob, serde, serde_json, crate::local_fs::walk_dir_filtered
// OUTPUT: FindFilesTool
// POS:    Search for files by glob pattern across the workspace, respecting .gitignore-like rules.
//! find_files — search file paths by glob pattern

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

/// Maximum results returned to prevent context overflow.
const MAX_RESULTS: usize = 1000;

#[derive(Debug, Deserialize)]
struct Input {
    pattern: String,
    path: Option<String>,
    #[serde(default)]
    max_results: Option<usize>,
}

pub struct FindFilesTool;

#[async_trait]
impl Tool for FindFilesTool {
    fn name(&self) -> &str {
        "find_files"
    }

    fn description(&self) -> &str {
        "Search for files by glob pattern (e.g. '*.rs', 'src/**/*.ts', '*test*'). \
         Returns matching file paths relative to the workspace root. \
         Respects .gitignore rules."
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
                    "description": "Maximum number of results to return. Default: 200"
                }
            }
        })
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

        let max_results = params.max_results.unwrap_or(MAX_RESULTS).min(MAX_RESULTS);

        // Parse glob pattern
        let glob = glob::Pattern::new(&params.pattern).map_err(|e| AgentError::ToolError {
            tool_name: self.name().into(),
            message: format!("Invalid glob pattern '{}': {}", params.pattern, e),
        })?;

        // Walk directory tree (hidden files excluded by default, .gitignore respected)
        let all_paths = crate::local_fs::walk_dir_filtered(search_root_str, None, false)?;

        // Match glob against relative path and file name
        let workspace_str = workspace.to_str().unwrap_or_default();
        let mut matches: Vec<String> = Vec::new();

        for full_path in &all_paths {
            if matches.len() >= max_results {
                break;
            }

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
                matches.push(rel_path.to_string());
            }
        }

        matches.sort();

        let total_found = matches.len();
        let content = if matches.is_empty() {
            format!("No files found matching '{}'", params.pattern)
        } else {
            matches.join("\n")
        };

        let mut output_content = content;
        if total_found >= max_results {
            output_content.push_str(&format!(
                "\n\n[Results capped at {}. Narrow your pattern for more specific results.]",
                max_results
            ));
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(output_content)],
            is_error: false,
            details: Some(json!({
                "pattern": params.pattern,
                "total_found": total_found,
                "capped": total_found >= max_results,
            })),
        })
    }
}
