// INPUT:  alva_types, async_trait, serde, serde_json, tokio
// OUTPUT: SleepTool
// POS:    Pauses execution for a specified duration.
//! sleep_tool — pause execution

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    duration_ms: u64,
}

/// Maximum sleep duration (5 minutes) to prevent indefinite blocking.
const MAX_SLEEP_MS: u64 = 5 * 60 * 1000;

pub struct SleepTool;

#[async_trait]
impl Tool for SleepTool {
    fn name(&self) -> &str {
        "sleep"
    }

    fn description(&self) -> &str {
        "Pause execution for a specified number of milliseconds. Maximum 5 minutes (300000ms). \
         Use sparingly — prefer event-driven approaches when possible."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["duration_ms"],
            "properties": {
                "duration_ms": {
                    "type": "integer",
                    "description": "Duration to sleep in milliseconds (max 300000)"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    fn is_concurrency_safe(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        input: Value,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        let duration = params.duration_ms.min(MAX_SLEEP_MS);

        if params.duration_ms > MAX_SLEEP_MS {
            return Ok(ToolOutput::error(format!(
                "Requested sleep of {}ms exceeds maximum of {}ms. Use a shorter duration.",
                params.duration_ms, MAX_SLEEP_MS
            )));
        }

        // Sleep with cancellation support
        let mut cancel = ctx.cancel_token().clone();
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_millis(duration)) => {
                Ok(ToolOutput::text(format!(
                    "Slept for {}ms.",
                    duration
                )))
            }
            _ = cancel.cancelled() => {
                Ok(ToolOutput::error("Sleep cancelled."))
            }
        }
    }
}
