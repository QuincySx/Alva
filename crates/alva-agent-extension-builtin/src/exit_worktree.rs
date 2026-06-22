// INPUT:  alva_kernel_abi, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExitWorktreeTool
// POS:    Exits and optionally cleans up a git worktree.
//! exit_worktree — exit and cleanup a git worktree

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    action: String,
    #[serde(default)]
    discard_changes: Option<bool>,
}

pub struct ExitWorktreeTool;

#[async_trait]
impl Tool for ExitWorktreeTool {
    fn name(&self) -> &str {
        "exit_worktree"
    }

    fn description(&self) -> &str {
        "Exit and optionally remove the current git worktree. \
         Use 'keep' to preserve the worktree or 'remove' to delete it."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["action"],
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["keep", "remove"],
                    "description": "Whether to keep or remove the worktree"
                },
                "discard_changes": {
                    "type": "boolean",
                    "description": "If true and action is 'remove', discard uncommitted changes. Default false."
                }
            }
        })
    }

    fn is_destructive(&self, input: &Value) -> bool {
        input.get("action").and_then(|v| v.as_str()) == Some("remove")
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

        match params.action.as_str() {
            "keep" => {
                Ok(ToolOutput::text(
                    "Worktree kept. You can return to it later or remove it with exit_worktree action='remove'."
                ))
            }
            "remove" => {
                let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
                    tool_name: self.name().into(),
                    message: "workspace context required".into(),
                })?;

                let fallback = LocalToolFs::new(workspace);
                let fs = ctx.tool_fs().unwrap_or(&fallback);

                let force_flag = if params.discard_changes.unwrap_or(false) {
                    " --force"
                } else {
                    ""
                };

                let cmd = format!(
                    "git worktree remove {}{}",
                    workspace.display(),
                    force_flag
                );

                // Run from parent directory since we're removing the current worktree
                let parent = workspace.parent().map(|p| p.to_str().unwrap_or(""));

                match fs.exec(&cmd, parent, 30_000).await {
                    Ok(result) if result.success() => {
                        Ok(ToolOutput::text("Worktree removed successfully."))
                    }
                    Ok(result) => {
                        Ok(ToolOutput::error(format!(
                            "Failed to remove worktree:\n{}{}",
                            result.stdout,
                            result.stderr
                        )))
                    }
                    Err(e) => Err(AgentError::ToolError {
                        tool_name: self.name().into(),
                        message: format!("Failed to run git worktree: {}", e),
                    }),
                }
            }
            other => {
                Ok(ToolOutput::error(format!(
                    "Invalid action '{}'. Must be 'keep' or 'remove'.",
                    other
                )))
            }
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

    /// Initialise a repo with a worktree. Returns (tempdir, main_repo_path, worktree_path).
    /// Returns None if git is unavailable.
    fn init_repo_with_worktree() -> Option<(tempfile::TempDir, PathBuf, PathBuf)> {
        if Command::new("git").arg("--version").output().is_err() {
            return None;
        }

        let parent = tempfile::TempDir::new().expect("tempdir");
        let repo = parent.path().join("repo");
        std::fs::create_dir(&repo).expect("mkdir repo");

        let git = |args: &[&str], cwd: &Path| {
            let out = Command::new("git")
                .args(args)
                .current_dir(cwd)
                .env("GIT_AUTHOR_NAME", "tester")
                .env("GIT_AUTHOR_EMAIL", "t@example.com")
                .env("GIT_COMMITTER_NAME", "tester")
                .env("GIT_COMMITTER_EMAIL", "t@example.com")
                // Override any user-level config that would force commit/tag signing.
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_CONFIG_SYSTEM", "/dev/null")
                .output()
                .expect("git");
            assert!(
                out.status.success(),
                "git {:?} failed: {}",
                args,
                String::from_utf8_lossy(&out.stderr)
            );
        };

        git(&["init", "-q", "-b", "main"], &repo);
        std::fs::write(repo.join("a.txt"), "x").expect("write");
        git(&["add", "."], &repo);
        git(&["commit", "-q", "-m", "init"], &repo);

        // Place worktree *inside* the repo so that its parent dir is itself a git
        // working tree — required for `git worktree remove` invoked from parent.
        let wt = repo.join("wt");
        git(
            &[
                "worktree",
                "add",
                "-b",
                "branch/wt",
                wt.to_str().unwrap(),
                "HEAD",
            ],
            &repo,
        );

        Some((parent, repo, wt))
    }

    #[tokio::test]
    async fn keep_action_returns_success_without_running_git() {
        // 'keep' does not need a real git repo
        let dir = tempfile::TempDir::new().expect("tempdir");
        let ctx = TestContext {
            workspace: dir.path().to_path_buf(),
            cancel: CancellationToken::new(),
        };
        let tool = ExitWorktreeTool;

        let output = tool
            .execute(json!({ "action": "keep" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error);
        assert!(
            output.model_text().contains("Worktree kept"),
            "unexpected: {}",
            output.model_text()
        );
    }

    #[tokio::test]
    async fn invalid_action_returns_error_output() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let ctx = TestContext {
            workspace: dir.path().to_path_buf(),
            cancel: CancellationToken::new(),
        };
        let tool = ExitWorktreeTool;

        let output = tool
            .execute(json!({ "action": "nope" }), &ctx)
            .await
            .expect("execute should succeed with error output");

        assert!(output.is_error);
        assert!(
            output.model_text().contains("Invalid action"),
            "unexpected: {}",
            output.model_text()
        );
    }

    #[tokio::test]
    async fn missing_action_field_returns_tool_error() {
        let dir = tempfile::TempDir::new().expect("tempdir");
        let ctx = TestContext {
            workspace: dir.path().to_path_buf(),
            cancel: CancellationToken::new(),
        };
        let tool = ExitWorktreeTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing 'action' should fail deserialisation");
        assert!(
            err.to_string().contains("action"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn is_destructive_only_when_removing() {
        let tool = ExitWorktreeTool;
        assert!(tool.is_destructive(&json!({ "action": "remove" })));
        assert!(!tool.is_destructive(&json!({ "action": "keep" })));
        assert!(!tool.is_destructive(&json!({})));
    }

    #[tokio::test]
    async fn remove_action_removes_worktree() {
        let Some((_parent, _repo, wt)) = init_repo_with_worktree() else {
            eprintln!("git not available — skipping remove_action_removes_worktree");
            return;
        };

        assert!(wt.exists(), "precondition: worktree should exist");

        let ctx = TestContext {
            workspace: wt.clone(),
            cancel: CancellationToken::new(),
        };
        let tool = ExitWorktreeTool;

        let output = tool
            .execute(json!({ "action": "remove" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error, "remove failed: {}", output.model_text());
        assert!(
            output.model_text().contains("removed successfully"),
            "unexpected: {}",
            output.model_text()
        );
        assert!(!wt.exists(), "worktree dir should be gone");
    }
}
