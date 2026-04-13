// INPUT:  alva_types, async_trait, regex, schemars, serde, serde_json, crate::local_fs::walk_dir_filtered, crate::truncate
// OUTPUT: GrepSearchTool
// POS:    Searches for regex patterns across workspace files with glob filtering,
//         multiple output modes, context lines, pagination, multiline, type filters,
//         and auto-excluded VCS directories.
//! grep_search — regex search across files

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use regex::Regex;
use schemars::JsonSchema;
use serde::Deserialize;

use crate::local_fs::walk_dir_filtered;
use crate::truncate::{truncate_head, truncate_line, MAX_BYTES, MAX_LINES, MAX_LINE_LENGTH};

/// Output mode for grep results.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum OutputMode {
    /// Show matching lines with context (default for content).
    Content,
    /// Show only file paths that contain matches (default).
    FilesWithMatches,
    /// Show match counts per file.
    Count,
}

impl Default for OutputMode {
    fn default() -> Self {
        OutputMode::FilesWithMatches
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Regular expression pattern to search for.
    pattern: String,
    /// Directory to search in, relative to workspace root.
    #[serde(default)]
    path: Option<String>,
    /// Glob pattern to filter files, e.g. '*.rs', '*.{ts,tsx}'.
    #[serde(default, alias = "file_pattern")]
    glob: Option<String>,
    /// Case insensitive search (default false).
    #[serde(default, alias = "case_insensitive", rename = "-i")]
    case_insensitive_flag: Option<bool>,
    /// Treat pattern as literal string instead of regex, default false.
    #[serde(default)]
    literal: Option<bool>,
    /// Number of context lines before and after each match (legacy alias for -C).
    #[serde(default)]
    context: Option<usize>,
    /// Number of lines to show before each match.
    #[serde(default, rename = "-B")]
    context_before: Option<usize>,
    /// Number of lines to show after each match.
    #[serde(default, rename = "-A")]
    context_after: Option<usize>,
    /// Number of context lines before and after each match.
    #[serde(default, rename = "-C")]
    context_symmetric: Option<usize>,
    /// Show line numbers in output (default true for content mode).
    #[serde(default, rename = "-n")]
    line_numbers: Option<bool>,
    /// Maximum number of matches to return, default 100.
    #[serde(default)]
    max_results: Option<usize>,
    /// Output mode: "content", "files_with_matches" (default), "count".
    #[serde(default)]
    output_mode: Option<OutputMode>,
    /// File type to search (e.g., 'js', 'py', 'rust', 'go', 'java').
    #[serde(default, rename = "type")]
    file_type: Option<String>,
    /// Limit output to first N entries. Default 250.
    #[serde(default)]
    head_limit: Option<usize>,
    /// Skip first N entries before applying head_limit. Default 0.
    #[serde(default)]
    offset: Option<usize>,
    /// Enable multiline mode where . matches newlines. Default false.
    #[serde(default)]
    multiline: Option<bool>,
}

/// VCS / build directories to always exclude.
const AUTO_EXCLUDE_DIRS: &[&str] = &[".git", ".svn", ".hg", "node_modules", "target", "__pycache__", ".tox"];

/// Map short type names to glob patterns.
fn type_to_glob(file_type: &str) -> Vec<String> {
    match file_type {
        "js" => vec!["*.js".into(), "*.mjs".into(), "*.cjs".into()],
        "ts" => vec!["*.ts".into(), "*.tsx".into()],
        "py" => vec!["*.py".into()],
        "rust" | "rs" => vec!["*.rs".into()],
        "go" => vec!["*.go".into()],
        "java" => vec!["*.java".into()],
        "c" => vec!["*.c".into(), "*.h".into()],
        "cpp" => vec!["*.cpp".into(), "*.cxx".into(), "*.cc".into(), "*.hpp".into(), "*.hxx".into()],
        "rb" => vec!["*.rb".into()],
        "swift" => vec!["*.swift".into()],
        "kt" | "kotlin" => vec!["*.kt".into(), "*.kts".into()],
        "json" => vec!["*.json".into()],
        "yaml" | "yml" => vec!["*.yaml".into(), "*.yml".into()],
        "toml" => vec!["*.toml".into()],
        "md" | "markdown" => vec!["*.md".into(), "*.markdown".into()],
        "html" => vec!["*.html".into(), "*.htm".into()],
        "css" => vec!["*.css".into()],
        "sh" | "bash" => vec!["*.sh".into(), "*.bash".into()],
        "sql" => vec!["*.sql".into()],
        "xml" => vec!["*.xml".into()],
        other => vec![format!("*.{}", other)],
    }
}

/// Check if a path component matches any auto-excluded directory.
fn is_excluded_path(rel_path: &str) -> bool {
    for component in rel_path.split('/') {
        if AUTO_EXCLUDE_DIRS.contains(&component) {
            return true;
        }
    }
    false
}

#[derive(Tool)]
#[tool(
    name = "grep_search",
    description = "Search for a regex pattern across files in the workspace. Supports multiple output modes, \
        context lines, file type filtering, pagination, and multiline matching.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct GrepSearchTool;

impl GrepSearchTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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

        let output_mode = params.output_mode.unwrap_or_default();
        let max_results = params.max_results.unwrap_or(100);
        let head_limit = params.head_limit.unwrap_or(250);
        let offset = params.offset.unwrap_or(0);

        // Resolve context lines: -C > context, then -B/-A override individually
        let sym_context = params.context_symmetric.or(params.context).unwrap_or(0);
        let context_before = params.context_before.unwrap_or(sym_context);
        let context_after = params.context_after.unwrap_or(sym_context);
        let show_line_numbers = params.line_numbers.unwrap_or(output_mode == OutputMode::Content);
        let case_insensitive = params.case_insensitive_flag.unwrap_or(false);
        let multiline = params.multiline.unwrap_or(false);

        // Build regex — escape if literal mode
        let pattern_str = if params.literal.unwrap_or(false) {
            regex::escape(&params.pattern)
        } else {
            params.pattern.clone()
        };
        let mut flags = String::new();
        if case_insensitive {
            flags.push_str("(?i)");
        }
        if multiline {
            flags.push_str("(?s)");
        }
        let full_pattern = format!("{}{}", flags, pattern_str);

        let regex = Regex::new(&full_pattern)
            .map_err(|e| AgentError::ToolError { tool_name: "grep_search".into(), message: format!("Invalid regex: {}", e) })?;

        // Optional glob filter
        let glob_pattern = params.glob.as_ref().map(|p| {
            glob::Pattern::new(p).unwrap_or_else(|_| glob::Pattern::new("*").unwrap())
        });

        // Type-based glob patterns
        let type_globs: Option<Vec<glob::Pattern>> = params.file_type.as_ref().map(|t| {
            type_to_glob(t)
                .iter()
                .filter_map(|g| glob::Pattern::new(g).ok())
                .collect()
        });

        // Walk directory with .gitignore support (synchronous)
        let search_root_str = search_root.to_str().unwrap_or_default();
        let file_paths = walk_dir_filtered(search_root_str, None, false)?;

        let mut match_count = 0;
        let mut output_entries: Vec<String> = Vec::new();
        let mut entry_count: usize = 0;

        for file_path in file_paths {
            if match_count >= max_results {
                break;
            }

            // Get relative path from workspace for glob matching and display
            let rel_path = file_path
                .strip_prefix(workspace_str)
                .unwrap_or(&file_path)
                .trim_start_matches('/');

            // Auto-exclude VCS directories
            if is_excluded_path(rel_path) {
                continue;
            }

            // Match glob against full relative path (not just filename)
            if let Some(ref glob) = glob_pattern {
                if !glob.matches(rel_path) {
                    continue;
                }
            }

            // Match type filter
            if let Some(ref type_pats) = type_globs {
                let file_name = std::path::Path::new(rel_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or("");
                if !type_pats.iter().any(|p| p.matches(file_name)) {
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

            if multiline {
                // For multiline, search the whole content and map byte offsets to line numbers
                for m in regex.find_iter(content) {
                    let start_byte = m.start();
                    let line_idx = content[..start_byte].matches('\n').count();
                    if !match_indices.contains(&line_idx) {
                        match_indices.push(line_idx);
                        match_count += 1;
                        if match_count >= max_results {
                            break;
                        }
                    }
                }
            } else {
                for (idx, line) in lines.iter().enumerate() {
                    if regex.is_match(line) {
                        match_indices.push(idx);
                        match_count += 1;
                        if match_count >= max_results {
                            break;
                        }
                    }
                }
            }

            if match_indices.is_empty() {
                continue;
            }

            // Apply output mode
            match output_mode {
                OutputMode::FilesWithMatches => {
                    entry_count += 1;
                    if entry_count > offset && output_entries.len() < head_limit {
                        output_entries.push(rel_path.to_string());
                    }
                }
                OutputMode::Count => {
                    entry_count += 1;
                    if entry_count > offset && output_entries.len() < head_limit {
                        output_entries.push(format!("{}:{}", rel_path, match_indices.len()));
                    }
                }
                OutputMode::Content => {
                    // Build the set of line indices to display (matches + context), dedup via sorted set
                    let mut display_indices: Vec<usize> = Vec::new();
                    for &m in &match_indices {
                        let start = m.saturating_sub(context_before);
                        let end = std::cmp::min(m + context_after, total_lines.saturating_sub(1));
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
                        entry_count += 1;
                        if entry_count <= offset {
                            prev_idx = Some(idx);
                            continue;
                        }
                        if output_entries.len() >= head_limit {
                            break;
                        }

                        // Insert separator for non-contiguous ranges
                        if let Some(prev) = prev_idx {
                            if idx > prev + 1 {
                                output_entries.push("--".to_string());
                            }
                        }
                        prev_idx = Some(idx);

                        let line_num = idx + 1; // 1-based
                        let truncated = truncate_line(lines[idx], MAX_LINE_LENGTH);
                        if show_line_numbers {
                            if match_set.contains(&idx) {
                                output_entries.push(format!("{}:{}:{}", rel_path, line_num, truncated));
                            } else {
                                output_entries.push(format!("{}-{}-{}", rel_path, line_num, truncated));
                            }
                        } else if match_set.contains(&idx) {
                            output_entries.push(format!("{}:{}", rel_path, truncated));
                        } else {
                            output_entries.push(format!("{}-{}", rel_path, truncated));
                        }
                    }
                }
            }
        }

        if match_count == 0 {
            return Ok(ToolOutput::text("No matches found."));
        }

        // Join all output, then apply head truncation
        let raw_output = output_entries.join("\n");
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
