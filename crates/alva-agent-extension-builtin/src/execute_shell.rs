// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via ToolFs with configurable timeout, working directory,
//         environment variables, background execution, description, and git operation tracking.
//! execute_shell — run shell commands via ToolFs

use alva_kernel_abi::{
    AgentError, CancellationToken, ProgressEvent, Tool, ToolContent, ToolExecutionContext,
    ToolFsExecResult, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;
use tokio::process::Command;

use crate::local_fs::LocalToolFs;

// Foreground exec path — bypasses `ToolFs::exec` so we can `select!` on the
// caller's CancellationToken alongside timeout / wait. `ToolFs::exec`'s
// trait signature has no token slot; rather than break that public API for
// every adapter, we spawn directly here and rely on `kill_on_drop(true)`
// for cleanup. When the user cancels, the wait future is dropped, the
// owned Child drops, and the OS receives SIGKILL.
enum ForegroundOutcome {
    Output(ToolFsExecResult),
    TimedOut,
    Cancelled,
    SpawnFailed(String),
}

async fn exec_foreground_cancellable(
    command: &str,
    cwd: &Path,
    timeout_ms: u64,
    cancel: &CancellationToken,
) -> ForegroundOutcome {
    let mut cmd = Command::new("sh");
    cmd.kill_on_drop(true);
    cmd.arg("-c").arg(command);
    cmd.current_dir(cwd);
    // `spawn()` inherits stdio from the parent by default; `wait_with_output()`
    // only reads captured pipes. Explicit `piped()` here is what
    // `Command::output()` does internally — required to get stdout/stderr.
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    let child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => return ForegroundOutcome::SpawnFailed(e.to_string()),
    };

    // `wait_with_output` consumes the Child; dropping this future drops the
    // Child which (via kill_on_drop) terminates the process group.
    let wait_fut = child.wait_with_output();
    tokio::pin!(wait_fut);

    // `CancellationToken::cancelled()` needs `&mut`; clone the shared
    // watch handle so we don't have to plumb a mut borrow through the
    // tool API.
    let mut cancel = cancel.clone();
    let duration = Duration::from_millis(timeout_ms);

    tokio::select! {
        biased;
        _ = cancel.cancelled() => ForegroundOutcome::Cancelled,
        _ = tokio::time::sleep(duration) => ForegroundOutcome::TimedOut,
        result = &mut wait_fut => match result {
            Ok(output) => ForegroundOutcome::Output(ToolFsExecResult {
                stdout: String::from_utf8_lossy(&output.stdout).trim_end().to_string(),
                stderr: String::from_utf8_lossy(&output.stderr).trim_end().to_string(),
                exit_code: output.status.code().unwrap_or(-1),
            }),
            Err(e) => ForegroundOutcome::SpawnFailed(e.to_string()),
        }
    }
}

/// Default timeout in milliseconds (120 seconds).
const DEFAULT_TIMEOUT_MS: u64 = 120_000;
/// Maximum timeout in milliseconds (600 seconds / 10 minutes).
const MAX_TIMEOUT_MS: u64 = 600_000;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Shell command to execute.
    command: String,
    /// Working directory, defaults to workspace root.
    #[serde(default)]
    cwd: Option<String>,
    /// Timeout in seconds (legacy parameter, prefer `timeout` in ms).
    #[serde(default)]
    timeout_secs: Option<u64>,
    /// Timeout in milliseconds (default 120000, max 600000).
    #[serde(default)]
    timeout: Option<u64>,
    /// Human-readable description of what the command does.
    #[serde(default)]
    description: Option<String>,
    /// Spawn command as background task, return immediately (default false).
    #[serde(default)]
    run_in_background: Option<bool>,
    /// Additional environment variables to inject.
    #[serde(default)]
    env: Option<HashMap<String, String>>,
}

/// Known git operation prefixes for tracking.
const GIT_OPERATIONS: &[&str] = &[
    "git push",
    "git commit",
    "git pull",
    "git merge",
    "git rebase",
    "git reset",
    "git checkout",
    "git branch",
    "git tag",
    "git stash",
    "git fetch",
    "git clone",
];

/// Check if a command is a git operation and return the operation type.
fn detect_git_operation(command: &str) -> Option<&'static str> {
    let trimmed = command.trim();
    for op in GIT_OPERATIONS {
        if trimmed.starts_with(op) {
            return Some(op);
        }
    }
    None
}

#[derive(Tool)]
#[tool(
    name = "execute_shell",
    description = "Execute a shell command and return stdout/stderr/exit_code. Use this for running programs, \
        checking system state, or performing operations that require shell access. \
        Supports timeout, background execution, environment variables, and git operation tracking.",
    input = Input,
    // Shell commands can mutate arbitrary FS paths, environment vars, background
    // processes, network state — not precisely modelable with resource_keys.
    // Treat as globally exclusive: blocks all other tools while running, and
    // waits for all in-flight tools to finish before starting.
    execution_mode = "serial-global",
)]
pub struct ExecuteShellTool;

impl ExecuteShellTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: "execute_shell".into(),
            message: "workspace context required".into(),
        })?;
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        // Resolve timeout: prefer `timeout` (ms), then `timeout_secs` (converted), then default
        let timeout_ms = if let Some(ms) = params.timeout {
            ms.min(MAX_TIMEOUT_MS)
        } else if let Some(secs) = params.timeout_secs {
            (secs * 1000).min(MAX_TIMEOUT_MS)
        } else {
            DEFAULT_TIMEOUT_MS
        };

        let cwd = params.cwd.as_deref();

        // Report description as a progress event if provided
        if let Some(ref desc) = params.description {
            ctx.report_progress(ProgressEvent::Status {
                message: desc.clone(),
            });
        }

        // Detect git operations for tracking
        let git_op = detect_git_operation(&params.command);
        if let Some(op) = git_op {
            ctx.report_progress(ProgressEvent::Custom {
                data: json!({ "git_operation": op, "command": params.command }),
            });
        }

        // Build effective command with environment variables
        let effective_command = if let Some(ref env_vars) = params.env {
            if env_vars.is_empty() {
                params.command.clone()
            } else {
                // Prefix command with env var exports
                let mut exports = String::new();
                for (key, value) in env_vars {
                    // Escape single quotes in values
                    let escaped = value.replace('\'', "'\\''");
                    exports.push_str(&format!("export {}='{}'; ", key, escaped));
                }
                format!("{}{}", exports, params.command)
            }
        } else {
            params.command.clone()
        };

        // Handle background execution
        if params.run_in_background.unwrap_or(false) {
            // For background: spawn the command but don't wait for it
            let bg_command = format!(
                "nohup sh -c '{}' > /dev/null 2>&1 & echo $!",
                effective_command
            );
            match fs.exec(&bg_command, cwd, 5000).await {
                Ok(result) => {
                    let pid = result.stdout.trim().to_string();
                    return Ok(ToolOutput {
                        content: vec![ToolContent::text(format!(
                            "Command started in background (PID: {})\nCommand: {}",
                            pid, params.command
                        ))],
                        is_error: false,
                        details: Some(json!({
                            "background": true,
                            "pid": pid,
                            "command": params.command,
                        })),
                    });
                }
                Err(e) => {
                    return Err(AgentError::ToolError {
                        tool_name: "execute_shell".into(),
                        message: format!("Failed to start background command: {}", e),
                    });
                }
            }
        }

        // Resolve cwd against workspace for the direct spawn path. The
        // foreground branch no longer routes through `ToolFs::exec` — we
        // spawn directly to gain CancellationToken support (see comment on
        // `exec_foreground_cancellable`). Test/MockToolFs injection still
        // applies to background path + read/write/list_dir/exists.
        let resolved_cwd = match cwd {
            Some(rel) => {
                let p = Path::new(rel);
                if p.is_absolute() {
                    p.to_path_buf()
                } else {
                    workspace.join(p)
                }
            }
            None => workspace.to_path_buf(),
        };
        let _ = &fs; // foreground path bypasses fs; binding kept for background

        match exec_foreground_cancellable(
            &effective_command,
            &resolved_cwd,
            timeout_ms,
            ctx.cancel_token(),
        )
        .await
        {
            ForegroundOutcome::Output(result) => {
                // Report progress events (for real-time UI streaming)
                for line in result.stdout.lines() {
                    ctx.report_progress(ProgressEvent::StdoutLine {
                        line: line.to_string(),
                    });
                }
                for line in result.stderr.lines() {
                    ctx.report_progress(ProgressEvent::StderrLine {
                        line: line.to_string(),
                    });
                }

                // Combine stdout + stderr for the model
                let mut combined = String::new();
                if !result.stdout.is_empty() {
                    combined.push_str(&result.stdout);
                }
                if !result.stderr.is_empty() {
                    if !combined.is_empty() {
                        combined.push_str("\n--- stderr ---\n");
                    }
                    combined.push_str(&result.stderr);
                }

                // Use TAIL truncation — errors are usually at the end
                let tr = crate::truncate::truncate_tail(
                    &combined,
                    crate::truncate::MAX_LINES,
                    crate::truncate::MAX_BYTES,
                );

                let mut content = tr.text;
                if tr.truncated {
                    content = format!(
                        "[Output truncated: showing last {} of {} lines]\n{}",
                        tr.shown_lines, tr.total_lines, content
                    );
                }
                content.push_str(&format!("\n\nexit_code: {}", result.exit_code));

                let mut details = json!({
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                    "exit_code": result.exit_code,
                    "truncated": tr.truncated,
                });

                if let Some(op) = git_op {
                    details["git_operation"] = json!(op);
                }
                if let Some(ref desc) = params.description {
                    details["description"] = json!(desc);
                }

                Ok(ToolOutput {
                    content: vec![ToolContent::text(content)],
                    is_error: result.exit_code != 0,
                    details: Some(details),
                })
            }
            ForegroundOutcome::TimedOut => Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Command timed out after {}ms",
                    timeout_ms
                ))],
                is_error: true,
                details: Some(json!({ "timed_out": true, "timeout_ms": timeout_ms })),
            }),
            ForegroundOutcome::Cancelled => Ok(ToolOutput {
                content: vec![ToolContent::text("Command cancelled by user".to_string())],
                is_error: true,
                details: Some(json!({ "cancelled": true })),
            }),
            ForegroundOutcome::SpawnFailed(msg) => Err(AgentError::ToolError {
                tool_name: "execute_shell".into(),
                message: format!("Failed to execute command: {}", msg),
            }),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::{Path, PathBuf};

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

    fn make_ctx() -> (TestContext, TempDir) {
        let dir = TempDir::new().expect("tempdir");
        let ctx = TestContext {
            workspace: dir.path().to_path_buf(),
            cancel: CancellationToken::new(),
        };
        (ctx, dir)
    }

    #[tokio::test]
    async fn echo_captures_stdout_and_zero_exit() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;

        let output = tool
            .execute(json!({ "command": "echo hi" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error, "echo should have exit 0");
        let text = output.model_text();
        assert!(text.contains("hi"), "expected 'hi' in output: {text}");
        assert!(text.contains("exit_code: 0"), "exit_code missing: {text}");
    }

    #[tokio::test]
    async fn nonzero_exit_marks_is_error() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;

        // `exit 3` returns code 3 deterministically
        let output = tool
            .execute(json!({ "command": "exit 3" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(output.is_error, "nonzero exit should set is_error");
        let text = output.model_text();
        assert!(text.contains("exit_code: 3"), "expected exit 3: {text}");
    }

    #[tokio::test]
    async fn stderr_is_appended_to_output() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;

        // Send to stderr only
        let output = tool
            .execute(json!({ "command": "echo oops 1>&2" }), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error, "exit 0 even with stderr");
        let text = output.model_text();
        assert!(text.contains("oops"), "expected 'oops' in output: {text}");
    }

    #[tokio::test]
    async fn timeout_returns_timeout_output() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;

        // 100ms timeout, sleep 5s — should be killed
        let output = tool
            .execute(json!({ "command": "sleep 5", "timeout": 100 }), &ctx)
            .await
            .expect("timeout should produce ToolOutput, not Err");

        assert!(output.is_error, "timed-out command should be marked error");
        let text = output.model_text();
        assert!(
            text.contains("timed out"),
            "expected 'timed out' in output: {text}"
        );
        let details = output.details.as_ref().expect("details present");
        assert_eq!(
            details.get("timed_out").and_then(|v| v.as_bool()),
            Some(true)
        );
    }

    #[tokio::test]
    async fn env_vars_are_injected() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;

        let mut env = serde_json::Map::new();
        env.insert("MY_VAR".to_string(), json!("vault-secret"));

        let output = tool
            .execute(
                json!({
                    "command": "echo $MY_VAR",
                    "env": env,
                }),
                &ctx,
            )
            .await
            .expect("execute should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(
            text.contains("vault-secret"),
            "env var not injected: {text}"
        );
    }

    #[test]
    fn detect_git_operation_matches_known_prefixes() {
        assert_eq!(
            detect_git_operation("git push origin main"),
            Some("git push")
        );
        assert_eq!(
            detect_git_operation("  git commit -m 'x'"),
            Some("git commit")
        );
        assert_eq!(detect_git_operation("ls"), None);
        assert_eq!(detect_git_operation("git status"), None); // not in tracked list
    }

    /// Bug T4 regression guard: cancellation token must actually terminate
    /// the child mid-flight. Before this fix, only the `timeout` parameter
    /// could kill long-running commands — `CancellationToken::cancel()`
    /// was silently ignored because `LocalToolFs::exec` had no cancel
    /// slot in its signature.
    #[tokio::test]
    async fn cancellation_kills_running_command() {
        let (ctx, _dir) = make_ctx();
        let tool = ExecuteShellTool;
        let token = ctx.cancel.clone();

        // Long sleep with a generous timeout — without cancellation this
        // would block for 10s. We assert it returns in under ~1s after
        // the cancel.
        let start = std::time::Instant::now();
        let cancel_task = tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(80)).await;
            token.cancel();
        });

        let output = tool
            .execute(
                json!({
                    "command": "sleep 10",
                    "timeout": 60_000u64,
                }),
                &ctx,
            )
            .await
            .expect("cancel should produce ToolOutput, not Err");

        cancel_task.await.expect("cancel task should join");
        let elapsed = start.elapsed();

        assert!(output.is_error, "cancelled command should be marked error");
        let text = output.model_text();
        assert!(
            text.contains("cancelled"),
            "expected 'cancelled' in output: {text}"
        );
        let details = output.details.as_ref().expect("details present");
        assert_eq!(
            details.get("cancelled").and_then(|v| v.as_bool()),
            Some(true)
        );
        // Must return well under the 10s sleep — the kill happened.
        assert!(
            elapsed < Duration::from_secs(2),
            "cancel didn't kill promptly: {elapsed:?}"
        );
    }
}
