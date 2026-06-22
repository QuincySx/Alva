// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json, tokio
// OUTPUT: SleepTool
// POS:    Pauses execution for a specified duration.
//! sleep_tool — pause execution

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
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

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::Path;

    use super::*;
    use alva_kernel_abi::{CancellationToken, ToolExecutionContext};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn workspace(&self) -> Option<&Path> {
            None
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[tokio::test]
    async fn sleeps_for_short_duration() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
        };
        let tool = SleepTool;

        let start = std::time::Instant::now();
        let output = tool
            .execute(json!({ "duration_ms": 10 }), &ctx)
            .await
            .expect("execution should succeed");
        let elapsed = start.elapsed();

        assert!(!output.is_error, "got: {}", output.model_text());
        assert!(output.model_text().contains("Slept for 10ms"));
        // Allow ~5ms slack just in case the runtime is fast or slow.
        assert!(
            elapsed >= std::time::Duration::from_millis(5),
            "elapsed was {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn over_max_duration_returns_error_output() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
        };
        let tool = SleepTool;

        let output = tool
            .execute(json!({ "duration_ms": MAX_SLEEP_MS + 1 }), &ctx)
            .await
            .expect("execution should succeed with error output");
        assert!(output.is_error);
        assert!(output.model_text().contains("exceeds maximum"));
    }

    #[tokio::test]
    async fn cancellation_returns_error_promptly() {
        let cancel = CancellationToken::new();
        let ctx = TestContext {
            cancel: cancel.clone(),
        };
        let tool = SleepTool;

        // Cancel after a brief delay so the sleep is interrupted.
        let cancel_clone = cancel.clone();
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
            cancel_clone.cancel();
        });

        let start = std::time::Instant::now();
        let output = tool
            .execute(json!({ "duration_ms": 60_000 }), &ctx)
            .await
            .expect("execution should resolve");
        let elapsed = start.elapsed();

        assert!(output.is_error);
        assert!(output.model_text().contains("cancelled"));
        // Should return well under the requested 60s.
        assert!(
            elapsed < std::time::Duration::from_secs(2),
            "took too long: {elapsed:?}"
        );
    }
}
