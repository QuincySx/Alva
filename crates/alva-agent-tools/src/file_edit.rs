// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: FileEditTool
// POS:    Performs string-replace-based file editing with unique match enforcement,
//         replace_all mode, quote normalization, and staleness detection.
//! file_edit — string-replace based file editing (like Claude Code's Edit tool)

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::sync::Mutex;

use crate::local_fs::LocalToolFs;
use crate::truncate::safe_truncate;

fn truncated_preview(text: &str) -> &str {
    safe_truncate(text, 50)
}

fn unique_match_count(content: &str, needle: &str) -> usize {
    content.matches(needle).count()
}

/// Normalize smart quotes / curly quotes to their ASCII equivalents.
///
/// This handles common smart quote characters that editors or copy-paste may introduce:
/// - \u{2018}, \u{2019} (left/right single quote) -> '
/// - \u{201C}, \u{201D} (left/right double quote) -> "
/// - \u{2013} (en dash) -> -
/// - \u{2014} (em dash) -> --
fn normalize_quotes(text: &str) -> String {
    text.replace('\u{2018}', "'")
        .replace('\u{2019}', "'")
        .replace('\u{201C}', "\"")
        .replace('\u{201D}', "\"")
        .replace('\u{2013}', "-")
        .replace('\u{2014}', "--")
}

#[derive(Debug, Deserialize)]
struct SingleEdit {
    old_str: String,
    new_str: String,
}

#[derive(Debug, Deserialize)]
struct Input {
    path: String,
    #[serde(default)]
    old_str: Option<String>,
    #[serde(default)]
    new_str: Option<String>,
    #[serde(default)]
    edits: Option<Vec<SingleEdit>>,
    /// Replace all occurrences of old_str (default false).
    /// When true, old_str does not need to be unique.
    #[serde(default)]
    replace_all: Option<bool>,
}

// ---------------------------------------------------------------------------
// File read-state tracker (staleness detection)
// ---------------------------------------------------------------------------

/// Simple hash of file contents for staleness detection.
pub fn content_hash(data: &[u8]) -> u64 {
    // FNV-1a hash — fast, good enough for change detection
    let mut hash: u64 = 0xcbf29ce484222325;
    for &byte in data {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(0x100000001b3);
    }
    hash
}

/// Global file-read state: maps absolute path → content hash at last read.
///
/// Updated by `ReadFileTool` (via `record_file_read`) and checked by `FileEditTool`.
fn read_state() -> &'static Mutex<HashMap<String, u64>> {
    static STATE: std::sync::OnceLock<Mutex<HashMap<String, u64>>> = std::sync::OnceLock::new();
    STATE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Record that a file was read with the given content hash.
/// Called by ReadFileTool after reading a file.
pub fn record_file_read(path: &str, hash: u64) {
    if let Ok(mut state) = read_state().lock() {
        state.insert(path.to_string(), hash);
    }
}

/// Check if a file has been modified since the last recorded read.
/// Returns `Some(warning_message)` if stale, `None` if fresh or never read.
fn check_staleness(path: &str, current_content: &[u8]) -> Option<String> {
    let state = read_state().lock().ok()?;
    let recorded_hash = state.get(path)?;
    let current_hash = content_hash(current_content);
    if current_hash != *recorded_hash {
        Some(format!(
            "Warning: file '{}' has been modified since it was last read. \
             The edit may be based on stale content. Consider reading the file again first.",
            path
        ))
    } else {
        None
    }
}

pub struct FileEditTool;

#[async_trait]
impl Tool for FileEditTool {
    fn name(&self) -> &str {
        "file_edit"
    }

    fn description(&self) -> &str {
        "Edit a file by replacing exact string matches. Supports single edit (old_str+new_str), \
         batch edits (edits[] array), and replace_all mode. Each old_str must be unique unless \
         replace_all is true. Smart quotes are automatically normalized. Path is relative to workspace root."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": {
                    "type": "string",
                    "description": "File path relative to workspace root"
                },
                "old_str": {
                    "type": "string",
                    "description": "Exact string to find (single edit mode)"
                },
                "new_str": {
                    "type": "string",
                    "description": "Replacement string (single edit mode)"
                },
                "edits": {
                    "type": "array",
                    "description": "Array of {old_str, new_str} for batch editing (alternative to single edit)",
                    "items": {
                        "type": "object",
                        "required": ["old_str", "new_str"],
                        "properties": {
                            "old_str": { "type": "string" },
                            "new_str": { "type": "string" }
                        }
                    }
                },
                "replace_all": {
                    "type": "boolean",
                    "description": "Replace all occurrences of old_str (default false). When true, old_str does not need to be unique."
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: e.to_string() })?;

        let replace_all = params.replace_all.unwrap_or(false);

        // Normalize to Vec<SingleEdit>
        let edits = if let Some(edits) = params.edits {
            if edits.is_empty() {
                return Ok(ToolOutput::error("edits array is empty"));
            }
            edits
        } else if let (Some(old_str), Some(new_str)) = (params.old_str, params.new_str) {
            vec![SingleEdit { old_str, new_str }]
        } else {
            return Ok(ToolOutput::error("Provide either old_str+new_str or edits[]"));
        };

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "file_edit".into(),
            message: "local filesystem context required".into(),
        })?;
        let file_path = workspace.join(&params.path);
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let raw = fs
            .read_file(file_path.to_str().unwrap_or_default())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot read file: {}", e) })?;
        let content = String::from_utf8_lossy(&raw).into_owned();

        // Staleness detection: warn if file changed since last read
        let staleness_warning = check_staleness(
            file_path.to_str().unwrap_or_default(),
            &raw,
        );

        // Try matching with quote normalization if direct match fails
        let normalized_content = normalize_quotes(&content);

        // Validate ALL edits against the ORIGINAL content before applying
        for edit in &edits {
            if edit.old_str.is_empty() {
                return Ok(ToolOutput::error("old_str must not be empty"));
            }

            let count = unique_match_count(&content, &edit.old_str);

            // If not found directly, try with normalized quotes
            if count == 0 {
                let normalized_old = normalize_quotes(&edit.old_str);
                let norm_count = unique_match_count(&normalized_content, &normalized_old);
                if norm_count == 0 {
                    return Ok(ToolOutput::error(format!(
                        "old_str not found: {:?}",
                        truncated_preview(&edit.old_str)
                    )));
                }
                // Found via normalization — will handle below
            }

            if !replace_all && count > 1 {
                return Ok(ToolOutput::error(format!(
                    "old_str found {} times (must be unique, or use replace_all): {:?}",
                    count,
                    truncated_preview(&edit.old_str)
                )));
            }
        }

        // Apply all edits and generate combined diff
        let mut current = content.clone();
        let mut combined_diff = String::new();
        let mut details_edits = Vec::new();

        for (index, edit) in edits.iter().enumerate() {
            let direct_count = unique_match_count(&current, &edit.old_str);

            // Determine the actual search string to use
            let (search_in, search_for) = if direct_count > 0 {
                (current.clone(), edit.old_str.clone())
            } else {
                // Try normalized matching
                let normalized_old = normalize_quotes(&edit.old_str);
                let normalized_current = normalize_quotes(&current);
                if unique_match_count(&normalized_current, &normalized_old) == 0 {
                    return Ok(ToolOutput::error(format!(
                        "edit {} was invalidated by an earlier edit: old_str not found: {:?}",
                        index + 1,
                        truncated_preview(&edit.old_str)
                    )));
                }
                (normalized_current, normalized_old)
            };

            if replace_all {
                // Replace all occurrences
                let count = unique_match_count(&search_in, &search_for);
                if count == 0 {
                    return Ok(ToolOutput::error(format!(
                        "edit {} was invalidated by an earlier edit: old_str not found: {:?}",
                        index + 1,
                        truncated_preview(&edit.old_str)
                    )));
                }

                // Find line number of first change
                let match_index = search_in.find(&search_for).expect("validated match");
                let line_num = search_in[..match_index].lines().count() + 1;

                let old_lines: Vec<&str> = edit.old_str.lines().collect();
                let new_lines: Vec<&str> = edit.new_str.lines().collect();
                combined_diff.push_str(&format!("--- {}\n+++ {}\n@@ -{} ({} occurrences replaced) @@\n",
                    params.path, params.path,
                    line_num, count,
                ));
                for line in &old_lines {
                    combined_diff.push_str(&format!("-{}\n", line));
                }
                for line in &new_lines {
                    combined_diff.push_str(&format!("+{}\n", line));
                }

                details_edits.push(json!({
                    "first_changed_line": line_num,
                    "occurrences_replaced": count,
                    "old_lines": old_lines.len(),
                    "new_lines": new_lines.len(),
                }));

                // If we had to use normalized matching, we need to replace in normalized space
                // but actually write back the proper replacements
                if direct_count > 0 {
                    current = current.replace(&edit.old_str, &edit.new_str);
                } else {
                    // Replace in normalized content, then that becomes our new current
                    let normalized_current = normalize_quotes(&current);
                    let normalized_old = normalize_quotes(&edit.old_str);
                    current = normalized_current.replace(&normalized_old, &edit.new_str);
                }
            } else {
                // Single replacement (existing behavior)
                let current_count = unique_match_count(&search_in, &search_for);
                if current_count == 0 {
                    return Ok(ToolOutput::error(format!(
                        "edit {} was invalidated by an earlier edit: old_str not found: {:?}",
                        index + 1,
                        truncated_preview(&edit.old_str)
                    )));
                }
                if current_count > 1 {
                    return Ok(ToolOutput::error(format!(
                        "edit {} was invalidated by an earlier edit: old_str found {} times (must be unique): {:?}",
                        index + 1,
                        current_count,
                        truncated_preview(&edit.old_str)
                    )));
                }

                // Find line number of change in current content
                let match_index = search_in.find(&search_for).expect("validated unique match");
                let line_num = search_in[..match_index].lines().count() + 1;

                // Generate unified diff for this edit
                let old_lines: Vec<&str> = edit.old_str.lines().collect();
                let new_lines: Vec<&str> = edit.new_str.lines().collect();
                combined_diff.push_str(&format!("--- {}\n+++ {}\n@@ -{},{} +{},{} @@\n",
                    params.path, params.path,
                    line_num, old_lines.len(),
                    line_num, new_lines.len(),
                ));
                for line in &old_lines {
                    combined_diff.push_str(&format!("-{}\n", line));
                }
                for line in &new_lines {
                    combined_diff.push_str(&format!("+{}\n", line));
                }

                details_edits.push(json!({
                    "first_changed_line": line_num,
                    "old_lines": old_lines.len(),
                    "new_lines": new_lines.len(),
                }));

                if direct_count > 0 {
                    current = current.replacen(&edit.old_str, &edit.new_str, 1);
                } else {
                    let normalized_current = normalize_quotes(&current);
                    let normalized_old = normalize_quotes(&edit.old_str);
                    current = normalized_current.replacen(&normalized_old, &edit.new_str, 1);
                }
            }
        }

        fs.write_file(file_path.to_str().unwrap_or_default(), current.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "file_edit".into(), message: format!("Cannot write file: {}", e) })?;

        // Update read state with new content hash
        record_file_read(
            file_path.to_str().unwrap_or_default(),
            content_hash(current.as_bytes()),
        );

        let edit_count = details_edits.len();
        let summary = if edit_count == 1 {
            let line = details_edits[0]["first_changed_line"].as_u64().unwrap_or(0);
            if replace_all {
                let replaced = details_edits[0]["occurrences_replaced"].as_u64().unwrap_or(1);
                format!("File edited: {} (line {}, {} occurrences replaced)", params.path, line, replaced)
            } else {
                format!("File edited: {} (line {})", params.path, line)
            }
        } else {
            format!("File edited: {} ({} edits applied)", params.path, edit_count)
        };

        // Prepend staleness warning if detected
        let mut output_text = String::new();
        if let Some(ref warning) = staleness_warning {
            output_text.push_str(&warning);
            output_text.push_str("\n\n");
        }
        output_text.push_str(&summary);
        output_text.push_str("\n\n");
        output_text.push_str(&combined_diff);

        Ok(ToolOutput {
            content: vec![ToolContent::text(output_text)],
            is_error: false,
            details: Some(json!({
                "path": params.path,
                "edits": details_edits,
                "stale": staleness_warning.is_some(),
            })),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::MockToolFs;
    use alva_types::{CancellationToken, ToolExecutionContext, ToolFs};

    struct TestContext {
        workspace: PathBuf,
        cancel: CancellationToken,
        fs: MockToolFs,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }

        fn session_id(&self) -> &str {
            "test-session"
        }

        fn workspace(&self) -> Option<&Path> {
            Some(&self.workspace)
        }

        fn tool_fs(&self) -> Option<&dyn alva_types::ToolFs> {
            Some(&self.fs)
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn batch_edits_fail_when_earlier_edit_invalidates_later_match() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new().with_file("/workspace/test.txt", b"alpha\nbeta\n"),
        };
        let tool = FileEditTool;

        let output = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "edits": [
                        { "old_str": "alpha", "new_str": "beta" },
                        { "old_str": "beta", "new_str": "gamma" }
                    ]
                }),
                &ctx,
            )
            .await
            .expect("tool execution should succeed with an error output");

        assert!(output.is_error, "batch edit should be rejected");
        assert!(
            output.model_text().contains("earlier edit"),
            "unexpected output: {}",
            output.model_text()
        );

        let current = String::from_utf8(
            ctx.fs
                .read_file("/workspace/test.txt")
                .await
                .expect("file should still exist"),
        )
        .expect("mock file should be utf-8");
        assert_eq!(current, "alpha\nbeta\n");
    }

    #[tokio::test]
    async fn replace_all_replaces_multiple_occurrences() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new().with_file("/workspace/test.txt", b"foo bar foo baz foo"),
        };
        let tool = FileEditTool;

        let output = tool
            .execute(
                json!({
                    "path": "test.txt",
                    "old_str": "foo",
                    "new_str": "qux",
                    "replace_all": true,
                }),
                &ctx,
            )
            .await
            .expect("tool execution should succeed");

        assert!(!output.is_error, "replace_all should succeed");

        let current = String::from_utf8(
            ctx.fs
                .read_file("/workspace/test.txt")
                .await
                .expect("file should exist"),
        )
        .expect("utf-8");
        assert_eq!(current, "qux bar qux baz qux");
    }

    #[test]
    fn truncated_preview_multibyte_no_panic() {
        // 20 CJK chars = 60 bytes in UTF-8, exceeds the 50-byte limit.
        let text = "你好世界你好世界你好世界你好世界你好世界";
        let preview = truncated_preview(text);
        // Must not panic and must be valid UTF-8 with len <= 50.
        assert!(preview.len() <= 50);
        assert!(preview.is_char_boundary(preview.len()));
        // 50 / 3 = 16 full chars = 48 bytes.
        assert_eq!(preview, "你好世界你好世界你好世界你好世界");
    }

    #[test]
    fn truncated_preview_short_string() {
        let text = "hello";
        assert_eq!(truncated_preview(text), "hello");
    }

    #[test]
    fn normalize_quotes_converts_smart_quotes() {
        let input = "\u{201C}hello\u{201D} and \u{2018}world\u{2019}";
        let expected = "\"hello\" and 'world'";
        assert_eq!(normalize_quotes(input), expected);
    }

    #[test]
    fn normalize_quotes_handles_dashes() {
        let input = "foo\u{2013}bar\u{2014}baz";
        assert_eq!(normalize_quotes(input), "foo-bar--baz");
    }

    #[test]
    fn content_hash_deterministic() {
        let data = b"hello world";
        let h1 = content_hash(data);
        let h2 = content_hash(data);
        assert_eq!(h1, h2);
    }

    #[test]
    fn content_hash_different_for_different_content() {
        let h1 = content_hash(b"hello");
        let h2 = content_hash(b"world");
        assert_ne!(h1, h2);
    }

    #[test]
    fn staleness_detection_no_prior_read() {
        // No prior read → should return None (not stale)
        let result = check_staleness("/nonexistent/file.txt", b"content");
        assert!(result.is_none());
    }

    #[test]
    fn staleness_detection_fresh() {
        let path = "/tmp/test-staleness-fresh.txt";
        let content = b"hello world";
        record_file_read(path, content_hash(content));
        let result = check_staleness(path, content);
        assert!(result.is_none(), "file should not be stale");
    }

    #[test]
    fn staleness_detection_stale() {
        let path = "/tmp/test-staleness-stale.txt";
        let original = b"hello world";
        let modified = b"hello world modified";
        record_file_read(path, content_hash(original));
        let result = check_staleness(path, modified);
        assert!(result.is_some(), "file should be detected as stale");
        assert!(result.unwrap().contains("modified since"));
    }

    #[tokio::test]
    async fn edit_with_staleness_warning() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new().with_file("/workspace/stale.txt", b"original content"),
        };

        // Record a read with different content (simulating external modification)
        record_file_read("/workspace/stale.txt", content_hash(b"different content"));

        let tool = FileEditTool;
        let output = tool
            .execute(
                json!({
                    "path": "stale.txt",
                    "old_str": "original",
                    "new_str": "updated",
                }),
                &ctx,
            )
            .await
            .expect("should succeed with warning");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("Warning"), "should contain staleness warning: {}", text);
        assert!(text.contains("modified since"), "should mention modification: {}", text);
    }
}
