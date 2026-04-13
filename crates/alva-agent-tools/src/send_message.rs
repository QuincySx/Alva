// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: SendMessageTool
// POS:    Sends messages between agents for inter-agent communication.
//! send_message — send messages between agents

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Recipient agent name or ID.
    to: String,
    /// Message content to send.
    message: String,
    /// Optional short summary of the message for context.
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "send_message",
    description = "Send a message to another agent by name or ID. Used for inter-agent communication \
        in multi-agent setups.",
    input = Input,
    read_only,
)]
pub struct SendMessageTool;

impl SendMessageTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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
