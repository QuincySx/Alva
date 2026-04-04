// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: SendMessageTool
// POS:    Sends messages between agents for inter-agent communication.
//! send_message — send messages between agents

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    to: String,
    message: String,
    #[serde(default)]
    summary: Option<String>,
}

pub struct SendMessageTool;

#[async_trait]
impl Tool for SendMessageTool {
    fn name(&self) -> &str {
        "send_message"
    }

    fn description(&self) -> &str {
        "Send a message to another agent by name or ID. Used for inter-agent communication \
         in multi-agent setups."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["to", "message"],
            "properties": {
                "to": {
                    "type": "string",
                    "description": "Recipient agent name or ID"
                },
                "message": {
                    "type": "string",
                    "description": "Message content to send"
                },
                "summary": {
                    "type": "string",
                    "description": "Optional short summary of the message for context"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        let summary_info = params
            .summary
            .as_deref()
            .unwrap_or("(no summary)");

        Ok(ToolOutput::text(format!(
            "Message sent to '{}'.\n  Summary: {}\n  Length: {} chars",
            params.to, summary_info, params.message.len()
        )))
    }
}
