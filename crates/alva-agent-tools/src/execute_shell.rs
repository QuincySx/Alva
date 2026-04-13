// INPUT:  alva_types, async_trait, schemars, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via ToolFs with configurable timeout, working directory,
//         environment variables, background execution, description, and git operation tracking.
//! execute_shell — run shell commands via ToolFs

use alva_types::{AgentError, ProgressEvent, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::json;
use std::collections::HashMap;

use crate::local_fs::LocalToolFs;

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
#[allow(unused)]
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
            let bg_command = format!("nohup sh -c '{}' > /dev/null 2>&1 & echo $!", effective_command);
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

        match fs.exec(&effective_command, cwd, timeout_ms).await {
            Ok(result) => {
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
            Err(AgentError::ToolError { message, .. }) if message.contains("timed out") => {
                Ok(ToolOutput {
                    content: vec![ToolContent::text(format!(
                        "Command timed out after {}ms",
                        timeout_ms
                    ))],
                    is_error: true,
                    details: Some(json!({ "timed_out": true, "timeout_ms": timeout_ms })),
                })
            }
            Err(e) => Err(AgentError::ToolError {
                tool_name: "execute_shell".into(),
                message: format!("Failed to execute command: {}", e),
            }),
        }
    }
}
