// INPUT:  alva_kernel_abi, async_trait, schemars, serde, crate::local_fs::LocalToolFs
// OUTPUT: EnterWorktreeTool
// POS:    Creates an isolated git worktree for safe parallel development.
//! enter_worktree — create an isolated git worktree

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name for the worktree (used for branch and directory). Auto-generated if omitted.
    #[serde(default)]
    name: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "enter_worktree",
    description = "Create an isolated git worktree for safe parallel development. \
        The worktree gets its own branch and working directory, allowing \
        changes without affecting the main workspace.",
    input = Input,
)]
pub struct EnterWorktreeTool;

impl EnterWorktreeTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "enter_worktree".into(),
            message: "workspace context required".into(),
        })?;

        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // Generate worktree name
        let wt_name = params.name.unwrap_or_else(|| {
            format!(
                "wt-{}",
                &alva_kernel_abi::generate_task_id(&alva_kernel_abi::TaskType::LocalAgent)[1..9]
            )
        });
        let branch_name = format!("worktree/{}", wt_name);
        let wt_path = workspace.parent().unwrap_or(workspace).join(&wt_name);

        // Create the worktree via git
        let cmd = format!(
            "git worktree add -b {} {} HEAD",
            branch_name,
            wt_path.display()
        );
        let cwd = workspace.to_str();

        match fs.exec(&cmd, cwd, 30_000).await {
            Ok(result) if result.success() => Ok(ToolOutput::text(format!(
                "Worktree created.\n  Path: {}\n  Branch: {}\n  \
                     Use exit_worktree to clean up when done.",
                wt_path.display(),
                branch_name
            ))),
            Ok(result) => Ok(ToolOutput::error(format!(
                "Failed to create worktree:\n{}{}",
                result.stdout, result.stderr
            ))),
            Err(e) => Err(AgentError::ToolError {
                tool_name: "enter_worktree".into(),
                message: format!("Failed to run git worktree: {}", e),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    use super::*;
    use alva_kernel_abi::{CancellationToken, ToolExecutionContext};
    use serde_json::json;
    use tempfile::TempDir;

    struct TestContext {
        workspace: PathBuf,
        cancel: CancellationToken,
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
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    /// Initialise a minimal git repo with a single commit so worktree add can succeed.
    /// Returns the temp dir (parent) and the repo dir inside it.
    fn init_repo() -> Option<(TempDir, PathBuf)> {
        // Skip the test silently if `git` is not on PATH.
        if Command::new("git").arg("--version").output().is_err() {
            return None;
        }

        let parent = TempDir::new().expect("tempdir");
        let repo = parent.path().join("repo");
        std::fs::create_dir(&repo).expect("create repo dir");

        let run = |args: &[&str]| {
            let status = Command::new("git")
                .args(args)
                .current_dir(&repo)
                .env("GIT_AUTHOR_NAME", "tester")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "tester")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                // Override any user-level config that would force commit/tag signing
                // (otherwise tests fail on machines with commit.gpgsign=true).
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git command");
            assert!(
                status.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&status.stderr)
            );
        };

        run(&["init", "-q", "-b", "main"]);
        std::fs::write(repo.join("README.md"), "hi").expect("write");
        run(&["add", "."]);
        run(&["commit", "-q", "-m", "init"]);

        Some((parent, repo))
    }

    #[tokio::test]
    async fn enter_worktree_creates_named_worktree() {
        let Some((_parent, repo)) = init_repo() else {
            eprintln!("git not available — skipping enter_worktree_creates_named_worktree");
            return;
        };

        let ctx = TestContext {
            workspace: repo.clone(),
            cancel: CancellationToken::new(),
        };
        let tool = EnterWorktreeTool;

        let output = tool
            .execute(json!({ "name": "feature-a" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(
            !output.is_error,
            "worktree creation failed: {}",
            output.model_text()
        );
        let text = output.model_text();
        assert!(
            text.contains("Worktree created"),
            "unexpected output: {text}"
        );
        assert!(
            text.contains("worktree/feature-a"),
            "branch name missing: {text}"
        );

        // Sibling directory should now exist
        let wt_path = repo.parent().unwrap().join("feature-a");
        assert!(
            wt_path.exists(),
            "expected worktree dir at {}",
            wt_path.display()
        );
    }

    #[tokio::test]
    async fn enter_worktree_errors_without_workspace() {
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
        let tool = EnterWorktreeTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("should error without workspace");
        assert!(
            err.to_string().contains("workspace context required"),
            "unexpected error: {err}"
        );
    }

    #[tokio::test]
    async fn enter_worktree_fails_in_non_git_dir() {
        let dir = TempDir::new().expect("tempdir");
        let ctx = TestContext {
            workspace: dir.path().to_path_buf(),
            cancel: CancellationToken::new(),
        };
        let tool = EnterWorktreeTool;

        let output = tool
            .execute(json!({ "name": "wt" }), &ctx)
            .await
            .expect("execute should return ToolOutput even on git failure");

        // git worktree in a non-repo prints an error and exits non-zero
        assert!(output.is_error, "expected error output in non-git dir");
        let text = output.model_text();
        assert!(
            text.contains("Failed to create worktree"),
            "expected failure message: {text}"
        );
    }
}
