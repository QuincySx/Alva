// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, tokio::process
// OUTPUT: ExecuteShellTool
// POS:    Executes shell commands via tokio process with configurable timeout and working directory.
//! execute_shell — run shell commands via tokio::process::Command

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "execute_shell".to_string(),
            description: "Execute a shell command and return stdout/stderr/exit_code. Use this for running programs, checking system state, or performing operations that require shell access.".to_string(),
            parameters: json!({
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
            }),
        }
    }

    async fn execute(&self, input: Value, ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let cwd = params
            .cwd
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| ctx.workspace.clone());

        let timeout = std::time::Duration::from_secs(params.timeout_secs.unwrap_or(30));
        let start = Instant::now();

        let result = tokio::time::timeout(timeout, async {
            tokio::process::Command::new("sh")
                .arg("-c")
                .arg(&params.command)
                .current_dir(&cwd)
                .output()
                .await
        })
        .await;

        let duration_ms = start.elapsed().as_millis() as u64;

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
                    tool_call_id: String::new(), // Filled by engine
                    tool_name: "execute_shell".to_string(),
                    output: output_text,
                    is_error: exit_code != 0,
                    duration_ms,
                })
            }
            Ok(Err(e)) => Err(EngineError::ToolExecution(format!(
                "Failed to execute command: {}",
                e
            ))),
            Err(_) => Ok(ToolResult {
                tool_call_id: String::new(),
                tool_name: "execute_shell".to_string(),
                output: json!({
                    "stdout": "",
                    "stderr": "Command timed out",
                    "exit_code": -1,
                    "timed_out": true,
                })
                .to_string(),
                is_error: true,
                duration_ms,
            }),
        }
    }
}
