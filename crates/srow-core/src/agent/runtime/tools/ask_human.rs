// INPUT:  crate::domain::tool, crate::error, crate::ports::tool, async_trait, serde, serde_json, std::io
// OUTPUT: AskHumanTool
// POS:    Requests input from the human user via stdin in CLI mode.
//! ask_human — request input from the user
//!
//! In CLI mode, this reads from stdin.
//! In GUI mode (Tauri), the engine event WaitingForHuman would be used instead.

use crate::domain::tool::{ToolDefinition, ToolResult};
use crate::error::EngineError;
use crate::ports::tool::{Tool, ToolContext};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::time::Instant;

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

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "ask_human".to_string(),
            description: "Ask the human user a question and wait for their response. Use this when you need clarification, confirmation, or additional information from the user.".to_string(),
            parameters: json!({
                "type": "object",
                "required": ["question"],
                "properties": {
                    "question": {
                        "type": "string",
                        "description": "The question to ask the user"
                    }
                }
            }),
        }
    }

    async fn execute(&self, input: Value, _ctx: &ToolContext) -> Result<ToolResult, EngineError> {
        let params: Input =
            serde_json::from_value(input).map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let start = Instant::now();

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
        .map_err(|e| EngineError::ToolExecution(e.to_string()))?
        .map_err(|e| EngineError::ToolExecution(e.to_string()))?;

        let duration_ms = start.elapsed().as_millis() as u64;

        Ok(ToolResult {
            tool_call_id: String::new(),
            tool_name: "ask_human".to_string(),
            output: answer,
            is_error: false,
            duration_ms,
        })
    }
}
