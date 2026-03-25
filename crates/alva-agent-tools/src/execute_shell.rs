// INPUT:  alva_types, async_trait, serde, serde_json, crate::local_fs::LocalToolFs
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via ToolFs with configurable timeout and working directory.
//! execute_shell — run shell commands via ToolFs

use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
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

    async fn execute(&self, input: Value, _cancel: &CancellationToken, ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: self.name().into(), message: e.to_string() })?;

        let local = ctx.local().ok_or_else(|| AgentError::ToolError {
            tool_name: self.name().into(),
            message: "local context required".into(),
        })?;
        let fallback = LocalToolFs::new(local.workspace());
        let fs = ctx.tool_fs().unwrap_or(&fallback);

        let timeout_ms = params.timeout_secs.unwrap_or(30) * 1000;
        let cwd = params.cwd.as_deref();

        match fs.exec(&params.command, cwd, timeout_ms).await {
            Ok(result) => {
                let output_text = json!({
                    "stdout": result.stdout,
                    "stderr": result.stderr,
                    "exit_code": result.exit_code,
                    "timed_out": false,
                })
                .to_string();

                Ok(ToolResult {
                    content: output_text,
                    is_error: result.exit_code != 0,
                    details: None,
                })
            }
            Err(AgentError::ToolError { message, .. }) if message.contains("timed out") => {
                Ok(ToolResult {
                    content: json!({
                        "stdout": "",
                        "stderr": "Command timed out",
                        "exit_code": -1,
                        "timed_out": true,
                    })
                    .to_string(),
                    is_error: true,
                    details: None,
                })
            }
            Err(e) => Err(AgentError::ToolError {
                tool_name: self.name().into(),
                message: format!("Failed to execute command: {}", e),
            }),
        }
    }
}
