// INPUT:  alva_types, async_trait, serde, serde_json, std::io
// OUTPUT: AskHumanTool
// POS:    Requests input from the human user via stdin in CLI mode.
//! ask_human — request input from the user
//!
//! In CLI mode, this reads from stdin.
//! In GUI mode (Tauri), the engine event WaitingForHuman would be used instead.

use alva_types::{AgentError, CancellationToken, Tool, ToolContext, ToolResult};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    question: String,
}

pub struct AskHumanTool;

#[async_trait]
impl Tool for AskHumanTool {
    fn name(&self) -> &str {
        "ask_human"
    }

    fn description(&self) -> &str {
        "Ask the human user a question and wait for their response. Use this when you need clarification, confirmation, or additional information from the user."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["question"],
            "properties": {
                "question": {
                    "type": "string",
                    "description": "The question to ask the user"
                }
            }
        })
    }

    async fn execute(&self, input: Value, _cancel: &CancellationToken, _ctx: &dyn ToolContext) -> Result<ToolResult, AgentError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?;

        // In CLI mode: print question and read from stdin
        eprintln!("\n[ask_human] {}", params.question);
        eprint!("> ");

        let answer = tokio::task::spawn_blocking(|| {
            let mut buf = String::new();
            std::io::stdin()
                .read_line(&mut buf)
                .map(|_| buf.trim().to_string())
        })
        .await
        .map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?
        .map_err(|e| AgentError::ToolError { tool_name: "ask_human".into(), message: e.to_string() })?;

        Ok(ToolResult {
            content: answer,
            is_error: false,
            details: None,
        })
    }
}
