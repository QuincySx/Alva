// INPUT:  alva_types, async_trait, serde, serde_json, tokio::process
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via tokio process with configurable timeout and working directory.
//! execute_shell — run shell commands via tokio::process::Command

use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
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
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "execute_shell".into(), message: e.to_string() })?;

        let local = ctx.local().ok_or_else(|| AgentError::ToolError {
            tool_name: "execute_shell".into(),
            message: "local filesystem context required".into(),
        })?;
        let cwd = params
            .cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| local.workspace().to_path_buf());

        let timeout = std::time::Duration::from_secs(params.timeout_secs.unwrap_or(30));

        let result = tokio::time::timeout(timeout, async {
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&params.command)
                .current_dir(&cwd)
                .output()
                .await
        })
        .await;

        match result {
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code().unwrap_or(-1);

                let output_text = json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code,
                    "timed_out": false,
                })
                .to_string();

                Ok(ToolResult {
                    content: output_text,
                    is_error: exit_code != 0,
                    details: None,
                })
            }
            Ok(Err(e)) => Err(AgentError::ToolError {
                tool_name: "execute_shell".into(),
                message: format!("Failed to execute command: {}", e),
            }),
            Err(_) => Ok(ToolResult {
                content: json!({
                    "stdout": "",
                    "stderr": "Command timed out",
                    "exit_code": -1,
                    "timed_out": true,
                })
                .to_string(),
                is_error: true,
                details: None,
            }),
        }
    }
}
