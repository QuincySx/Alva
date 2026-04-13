// INPUT:  alva_types, async_trait, schemars, serde, serde_json, tokio
// OUTPUT: SleepTool
// POS:    Pauses execution for a specified duration.
//! sleep_tool — pause execution

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// Input parameters for the `sleep` tool.
///
/// `schemars` derives the JSON Schema from this struct's fields and
/// their doc comments — the tool's `parameters_schema()` is generated
/// by `#[derive(Tool)]` below.
#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Duration to sleep in milliseconds (max 300000).
    duration_ms: u64,
}

/// Maximum sleep duration (5 minutes) to prevent indefinite blocking.
const MAX_SLEEP_MS: u64 = 5 * 60 * 1000;

#[derive(Tool)]
#[tool(
    name = "sleep",
    description = "Pause execution for a specified number of milliseconds. \
        Maximum 5 minutes (300000ms). Use sparingly — prefer event-driven \
        approaches when possible.",
    input = Input,
    read_only,
    concurrency_safe,
)]
pub struct SleepTool;

impl SleepTool {
    /// Core execution with the input already deserialized. Called by
    /// the `#[derive(Tool)]`-generated `execute` after it parses the
    /// JSON input into `Input`.
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        if params.duration_ms > MAX_SLEEP_MS {
            return Ok(ToolOutput::error(format!(
                "Requested sleep of {}ms exceeds maximum of {}ms. Use a shorter duration.",
                params.duration_ms, MAX_SLEEP_MS
            )));
        }

        let duration = params.duration_ms.min(MAX_SLEEP_MS);

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
