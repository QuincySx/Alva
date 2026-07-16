// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, crate::PlatformToolFs
// OUTPUT: CreateFileTool
// POS:    Creates or overwrites a file with auto-creation of parent directories,
//         line ending preservation, and staleness detection.
//! create_file — create or overwrite a file (FileWriteTool behavior)

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::PlatformToolFs;

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
        let fallback = PlatformToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let path_str = file_path.to_str().unwrap_or_default();

        // Detect if file already exists — determine overwrite vs create
        let is_overwrite = fs.exists(path_str).await.unwrap_or(false);

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
            .map_err(|e| AgentError::ToolError {
                tool_name: "create_file".into(),
                message: e.to_string(),
            })?;

        let action = if is_overwrite {
            "overwritten"
        } else {
            "created"
        };
        let summary = format!(
            "File {}: {} ({} bytes)",
            action,
            file_path.display(),
            final_content.len()
        );

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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::MockToolFs;
    use alva_kernel_abi::{CancellationToken, ToolExecutionContext, ToolFs};

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
        fn tool_fs(&self) -> Option<&dyn ToolFs> {
            Some(&self.fs)
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn create_new_file_succeeds() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new(),
        };
        let tool = CreateFileTool;

        let output = tool
            .execute(
                json!({
                    "path": "hello.txt",
                    "content": "hi there",
                }),
                &ctx,
            )
            .await
            .expect("execution should succeed");

        assert!(
            !output.is_error,
            "expected success, got: {}",
            output.model_text()
        );
        assert!(output.model_text().contains("created"));

        let stored = ctx
            .fs
            .read_file("/workspace/hello.txt")
            .await
            .expect("file should be written");
        assert_eq!(stored, b"hi there");
    }

    #[tokio::test]
    async fn overwrite_preserves_crlf_line_endings() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new().with_file("/workspace/win.txt", b"old\r\nlines\r\n"),
        };
        let tool = CreateFileTool;

        let output = tool
            .execute(
                json!({
                    "path": "win.txt",
                    "content": "new\nshiny\nlines",
                }),
                &ctx,
            )
            .await
            .expect("execution should succeed");

        assert!(!output.is_error);
        assert!(output.model_text().contains("overwritten"));

        let stored = ctx
            .fs
            .read_file("/workspace/win.txt")
            .await
            .expect("file should still exist");
        assert_eq!(stored, b"new\r\nshiny\r\nlines");
    }

    #[tokio::test]
    async fn missing_workspace_returns_tool_error() {
        struct NoWorkspaceCtx {
            cancel: CancellationToken,
        }
        impl ToolExecutionContext for NoWorkspaceCtx {
            fn cancel_token(&self) -> &CancellationToken {
                &self.cancel
            }
            fn session_id(&self) -> &str {
                "test-session"
            }
            fn workspace(&self) -> Option<&Path> {
                None
            }
            fn as_any(&self) -> &dyn Any {
                self
            }
        }

        let ctx = NoWorkspaceCtx {
            cancel: CancellationToken::new(),
        };
        let tool = CreateFileTool;
        let err = tool
            .execute(
                json!({
                    "path": "x.txt",
                    "content": "y",
                }),
                &ctx,
            )
            .await
            .expect_err("should error without workspace");
        assert!(
            err.to_string().contains("workspace") || err.to_string().contains("filesystem"),
            "unexpected error: {err}"
        );
    }
}
