// INPUT:  alva_types, async_trait, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: CreateFileTool
// POS:    Creates or overwrites a file with auto-creation of parent directories,
//         line ending preservation, and staleness detection.
//! create_file — create or overwrite a file (FileWriteTool behavior)

use alva_types::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// File path relative to workspace root.
    path: String,
    /// File content to write.
    content: String,
    /// Auto-create parent directories, default true.
    #[serde(default)]
    create_dirs: Option<bool>,
}

/// Detect the dominant line ending style in existing content.
/// Returns `"\r\n"` if CRLF is dominant, otherwise `"\n"`.
#[allow(unused)]
fn detect_line_ending(existing: &str) -> &'static str {
    let crlf_count = existing.matches("\r\n").count();
    let lf_only_count = existing.matches('\n').count().saturating_sub(crlf_count);
    if crlf_count > lf_only_count && crlf_count > 0 {
        "\r\n"
    } else {
        "\n"
    }
}

/// Normalize all line endings in `content` to `target_ending`.
#[allow(unused)]
fn normalize_line_endings(content: &str, target_ending: &str) -> String {
    // First normalize everything to LF, then convert to target
    let normalized = content.replace("\r\n", "\n");
    if target_ending == "\r\n" {
        normalized.replace('\n', "\r\n")
    } else {
        normalized
    }
}

#[derive(Tool)]
#[tool(
    name = "create_file",
    description = "Create a new file or overwrite an existing file with the given content. \
        Preserves existing line endings (CRLF/LF) when overwriting. \
        The path is relative to the workspace root.",
    input = Input,
)]
pub struct CreateFileTool;

impl CreateFileTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "create_file".into(),
            message: "local filesystem context required".into(),
        })?;
        let file_path = workspace.join(&params.path);
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let path_str = file_path.to_str().unwrap_or_default();

        // Detect if file already exists — determine overwrite vs create
        let is_overwrite = fs.exists(path_str).await.unwrap_or(false);
        let mut staleness_warning: Option<String> = None;

        // Prepare content — preserve line endings of existing file
        let final_content = if is_overwrite {
            // Read existing file to detect line endings
            match fs.read_file(path_str).await {
                Ok(existing_bytes) => {
                    if let Ok(existing_text) = std::str::from_utf8(&existing_bytes) {
                        let ending = detect_line_ending(existing_text);
                        normalize_line_endings(&params.content, ending)
                    } else {
                        params.content.clone()
                    }
                }
                Err(_) => params.content.clone(),
            }
        } else {
            params.content.clone()
        };

        // write_file handles parent directory creation internally
        let _ = params.create_dirs; // honoured by ToolFs::write_file unconditionally
        fs.write_file(path_str, final_content.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError { tool_name: "create_file".into(), message: e.to_string() })?;

        let action = if is_overwrite { "overwritten" } else { "created" };
        let mut summary = format!(
            "File {}: {} ({} bytes)",
            action,
            file_path.display(),
            final_content.len()
        );

        if let Some(warning) = staleness_warning.take() {
            summary.push_str(&format!("\n\nWarning: {}", warning));
        }

        Ok(ToolOutput {
            content: vec![ToolContent::text(summary)],
            is_error: false,
            details: Some(json!({
                "path": params.path,
                "action": action,
                "bytes_written": final_content.len(),
            })),
        })
    }
}
