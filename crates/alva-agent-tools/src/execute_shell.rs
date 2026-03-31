// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via ToolFs with configurable timeout and working directory.
//! execute_shell — run shell commands via ToolFs

use alva_types::{AgentError, ProgressEvent, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

use crate::local_fs::LocalToolFs;

#[derive(Debug, Deserialize)]
struct Input {
    command: String,
    cwd: Option<String>,
    timeout_secs: Option<u64>,
}

pub struct ExecuteShellTool;

#[async_trait]
impl Tool for ExecuteShellTool {
    fn name(&self) -> &str {
        "execute_shell"
    }

    fn description(&self) -> &str {
        "Execute a shell command and return stdout/stderr/exit_code. Use this for running programs, checking system state, or performing operations that require shell access."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["command"],
            "properties": {
                "command": {
                    "type": "string",
                    "description": "Shell command to execute"
                },
                "cwd": {
                    "type": "string",
                    "description": "Working directory, defaults to workspace root"
                },
                "timeout_secs": {
                    "type": "integer",
                    "description": "Timeout in seconds, default 30"
                }
            }
        })
    }

    async fn execute(&self, input: Value, ctx: &dyn ToolExecutionContext) -> Result<ToolOutput, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: self.name().into(), message: e.to_string() })?;

        let workspace = ctx.workspace().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "workspace context required".into(),
        })?;
        let fallback = LocalToolFs::new(workspace);
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let timeout_ms = params.timeout_secs.unwrap_or(30) * 1000;
        let cwd = params.cwd.as_deref();

        match fs.exec(&params.command, cwd, timeout_ms).await {
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

                Ok(ToolOutput {
                    content: vec![ToolContent::text(content)],
                    is_error: result.exit_code != 0,
                    details: Some(json!({
                        "stdout": result.stdout,
                        "stderr": result.stderr,
                        "exit_code": result.exit_code,
                        "truncated": tr.truncated,
                    })),
                })
            }
            Err(AgentError::ToolError { message, .. }) if message.contains("timed out") => {
                Ok(ToolOutput {
                    content: vec![ToolContent::text("Command timed out")],
                    is_error: true,
                    details: Some(json!({ "timed_out": true })),
                })
            }
            Err(e) => Err(AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("Failed to execute command: {}", e),
            }),
        }
    }
}
