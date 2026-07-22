// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, crate::PlatformToolFs
// OUTPUT: ListFilesTool
// POS:    Lists directory contents with recursive traversal and hidden file filtering via ToolFs.
//! list_files — list directory contents

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;

use crate::PlatformToolFs;

/// Maximum entries returned to prevent context overflow.
const MAX_ENTRIES: usize = 500;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Directory path relative to workspace root, defaults to workspace root.
    #[serde(default)]
    path: Option<String>,
    /// List recursively, default false.
    #[serde(default)]
    recursive: Option<bool>,
    /// Max recursion depth, default 3.
    #[serde(default)]
    max_depth: Option<usize>,
    /// Show hidden files (starting with .), default false.
    #[serde(default)]
    show_hidden: Option<bool>,
}

/// Recursively collect entries (files and directories) via ToolFs,
/// returning relative paths with a trailing `/` for directories.
///
/// Uses `Box::pin` to allow async recursion.
fn list_entries<'a>(
    fs: &'a dyn alva_kernel_abi::ToolFs,
    root: &'a str,
    prefix: &'a str,
    depth: usize,
    max_depth: usize,
    show_hidden: bool,
) -> futures::future::BoxFuture<'a, Result<Vec<String>, AgentError>> {
    Box::pin(async move {
        let mut results = Vec::new();
        let entries = fs.list_dir(root).await?;
        for entry in entries {
            if !show_hidden && entry.name.starts_with('.') {
                continue;
            }
            let rel = if prefix.is_empty() {
                entry.name.clone()
            } else {
                format!("{}/{}", prefix, entry.name)
            };
            if entry.is_dir {
                results.push(format!("{}/", rel));
                if depth < max_depth {
                    let child_path = format!("{}/{}", root.trim_end_matches('/'), entry.name);
                    let sub =
                        list_entries(fs, &child_path, &rel, depth + 1, max_depth, show_hidden)
                            .await?;
                    results.extend(sub);
                }
            } else {
                results.push(rel);
            }
        }
        Ok(results)
    })
}

#[derive(Tool)]
#[tool(
    name = "list_files",
    description = "List files and directories in the given path. Returns a tree-like listing of the directory contents.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct ListFilesTool;

impl ListFilesTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "list_files".into(),
            message: "local filesystem context required".into(),
        })?;
        let target = params
            .path
            .map(|p| workspace.join(p))
            .unwrap_or_else(|| workspace.to_path_buf());

        let recursive = params.recursive.unwrap_or(false);
        let max_depth = if recursive {
            params.max_depth.unwrap_or(3)
        } else {
            1
        };
        let show_hidden = params.show_hidden.unwrap_or(false);

        let fallback = PlatformToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let target_str = target.to_str().unwrap_or_default();
        // A missing / non-directory path is a normal, model-recoverable
        // situation (e.g. listing `pages/` in a wiki that has none yet), not a
        // reason to abort the whole run. Return it as an error *tool result*
        // with a concrete recovery hint instead of propagating `?`, which the
        // worker's agent loop turns into a fatal "sandboxed agent loop failed".
        let mut entries = match list_entries(fs, target_str, "", 1, max_depth, show_hidden).await {
            Ok(entries) => entries,
            Err(error) => {
                return Ok(ToolOutput::error(format!(
                    "Cannot list directory '{target_str}': {error}. It may not \
                     exist in this workspace. List the workspace root (call \
                     list_files with no path) to see what actually exists, then \
                     retry with a valid path."
                )));
            }
        };
        entries.sort();

        let total = entries.len();
        let truncated = total > MAX_ENTRIES;
        if truncated {
            entries.truncate(MAX_ENTRIES);
        }
        let mut content = entries.join("\n");
        if truncated {
            content.push_str(&format!(
                "\n\n[Showing {} of {} entries. Use a more specific path.]",
                MAX_ENTRIES, total
            ));
        }
        Ok(ToolOutput {
            content: vec![ToolContent::text(content)],
            is_error: false,
            details: Some(json!({
                "total_entries": total,
                "shown": total.min(MAX_ENTRIES),
                "truncated": truncated,
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
    async fn lists_immediate_children_non_recursive() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new()
                .with_file("/workspace/a.txt", b"a")
                .with_file("/workspace/b.txt", b"b")
                .with_file("/workspace/sub/c.txt", b"c"),
        };
        let tool = ListFilesTool;

        let output = tool
            .execute(json!({}), &ctx)
            .await
            .expect("execution should succeed");

        assert!(!output.is_error, "got: {}", output.model_text());
        let text = output.model_text();
        assert!(text.contains("a.txt"), "missing a.txt: {text}");
        assert!(text.contains("b.txt"), "missing b.txt: {text}");
        // sub is a directory — appears with trailing /
        assert!(text.contains("sub/"), "missing sub/: {text}");
        // Non-recursive: should NOT list "sub/c.txt"
        assert!(!text.contains("c.txt"), "should not recurse: {text}");
    }

    /// Listing a directory that does not exist (e.g. `pages/` in a wiki that
    /// was never built) must return a recoverable *error tool result* the model
    /// can act on, never a propagated `Err` — the worker loop turns a tool
    /// `Err` into a fatal "sandboxed agent loop failed" and kills the run. The
    /// in-repo MockToolFs returns Ok(empty) for a missing dir, so it cannot
    /// reproduce the real WASI ENOENT; this uses a tiny fs that fails list_dir
    /// exactly like WasiFs does on a non-existent directory.
    #[tokio::test]
    async fn missing_directory_is_a_recoverable_error_not_fatal() {
        struct FailingListDirFs;
        #[async_trait::async_trait]
        impl ToolFs for FailingListDirFs {
            async fn exec(
                &self,
                _: &str,
                _: Option<&str>,
                _: u64,
            ) -> Result<alva_kernel_abi::ToolFsExecResult, AgentError> {
                unreachable!("list_files never execs")
            }
            async fn read_file(&self, _: &str) -> Result<Vec<u8>, AgentError> {
                unreachable!("list_files never reads")
            }
            async fn write_file(&self, _: &str, _: &[u8]) -> Result<(), AgentError> {
                unreachable!("list_files never writes")
            }
            async fn list_dir(
                &self,
                path: &str,
            ) -> Result<Vec<alva_kernel_abi::ToolFsDirEntry>, AgentError> {
                Err(AgentError::ToolError {
                    tool_name: "wasi_fs::read directory".into(),
                    message: format!(
                        "cannot read directory '{path}': No such file or directory (os error 44)"
                    ),
                })
            }
            async fn exists(&self, _: &str) -> Result<bool, AgentError> {
                Ok(false)
            }
        }

        struct FailingCtx {
            workspace: PathBuf,
            cancel: CancellationToken,
            fs: FailingListDirFs,
        }
        impl ToolExecutionContext for FailingCtx {
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

        let ctx = FailingCtx {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: FailingListDirFs,
        };
        let output = ListFilesTool
            .execute(json!({ "path": "pages" }), &ctx)
            .await
            .expect("must return Ok(error tool result), never a fatal Err");
        assert!(output.is_error, "missing dir must be flagged is_error");
        assert!(
            output.model_text().contains("list_files"),
            "error must guide recovery (list root): {}",
            output.model_text()
        );
    }

    #[tokio::test]
    async fn recursive_lists_nested_files() {
        let ctx = TestContext {
            workspace: PathBuf::from("/workspace"),
            cancel: CancellationToken::new(),
            fs: MockToolFs::new()
                .with_file("/workspace/top.txt", b"t")
                .with_file("/workspace/nested/inner.txt", b"i"),
        };
        let tool = ListFilesTool;

        let output = tool
            .execute(json!({ "recursive": true, "max_depth": 3 }), &ctx)
            .await
            .expect("execution should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("top.txt"));
        assert!(
            text.contains("nested/inner.txt"),
            "missing nested file: {text}"
        );
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
        let tool = ListFilesTool;
        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("should error without workspace");
        assert!(err.to_string().contains("filesystem") || err.to_string().contains("workspace"));
    }
}
