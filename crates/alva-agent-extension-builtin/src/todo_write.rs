// INPUT:  alva_kernel_abi, async_trait, schemars, serde, crate::local_fs::LocalToolFs
// OUTPUT: TodoWriteTool
// POS:    Writes progress notes to a file (defaults to CLAUDE.md).
//! todo_write — write progress notes

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Content to write (progress notes, TODO items, etc.).
    content: String,
    /// File path to write to (default: CLAUDE.md).
    #[serde(default)]
    file_path: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "todo_write",
    description = "Write progress notes or TODO items to a tracking file. \
        Defaults to CLAUDE.md in the workspace root.",
    input = Input,
)]
pub struct TodoWriteTool;

impl TodoWriteTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "todo_write".into(),
            message: "workspace context required".into(),
        })?;

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let file_name = params.file_path.as_deref().unwrap_or("CLAUDE.md");
        let target = if std::path::Path::new(file_name).is_absolute() {
            std::path::PathBuf::from(file_name)
        } else {
            workspace.join(file_name)
        };
        let path_str = target.to_str().unwrap_or_default();

        // Read existing content (if any), then append
        let existing = match fs.read_file(path_str).await {
            Ok(data) => String::from_utf8_lossy(&data).to_string(),
            Err(_) => String::new(),
        };

        let mut new_content = existing;
        if !new_content.is_empty() && !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(&params.content);
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }

        fs.write_file(path_str, new_content.as_bytes())
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "todo_write".into(),
                message: format!("Failed to write: {}", e),
            })?;

        Ok(ToolOutput::text(format!(
            "Written to {} ({} bytes).",
            target.display(),
            params.content.len()
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};

    use super::*;
    use crate::MockToolFs;
    use alva_kernel_abi::{CancellationToken, ToolExecutionContext, ToolFs};
    use serde_json::json;

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
    async fn writes_to_default_claude_md_when_no_file_path() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new(),
        };
        let tool = TodoWriteTool;

        let output = tool
            .execute(json!({ "content": "- first todo" }), &ctx)
            .await
            .expect("execution should succeed");

        assert!(!output.is_error);
        let stored = ctx
            .fs
            .read_file("/workspace/CLAUDE.md")
            .await
            .expect("should write to default path");
        let text = String::from_utf8(stored).unwrap();
        assert_eq!(text, "- first todo\n");
    }

    #[tokio::test]
    async fn appends_to_existing_file_with_separating_newline() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new().with_file(
                "/workspace/notes.md",
                b"existing line without trailing newline",
            ),
        };
        let tool = TodoWriteTool;

        let output = tool
            .execute(
                json!({ "content": "appended", "file_path": "notes.md" }),
                &ctx,
            )
            .await
            .expect("execution should succeed");

        assert!(!output.is_error);
        let stored = ctx.fs.read_file("/workspace/notes.md").await.unwrap();
        let text = String::from_utf8(stored).unwrap();
        assert_eq!(text, "existing line without trailing newline\nappended\n");
    }

    #[tokio::test]
    async fn missing_workspace_returns_error() {
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
        let tool = TodoWriteTool;
        let err = tool
            .execute(json!({ "content": "x" }), &ctx)
            .await
            .expect_err("should error without workspace");
        assert!(err.to_string().contains("workspace"));
    }
}
