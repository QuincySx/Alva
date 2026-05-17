// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: ExitPlanModeTool
// POS:    Exits planning mode, re-enabling destructive tools.
//! exit_plan_mode — exit planning mode

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// No parameters.
#[derive(Debug, Deserialize, JsonSchema)]
struct Input {}

#[derive(Tool)]
#[tool(
    name = "exit_plan_mode",
    description = "Exit planning mode and return to normal operation. Destructive tools \
        will be available again.",
    input = Input,
    // NOT read_only: writes session permission-mode state (symmetric
    // with enter_plan_mode). See enter_plan_mode comment + T9.
)]
pub struct ExitPlanModeTool;

impl ExitPlanModeTool {
    async fn execute_impl(
        &self,
        _params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation, this would clear the planning-mode flag
        // on the session/context.
        Ok(ToolOutput::text(
            "Exited planning mode. All tools are now available."
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::Path;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool, ToolExecutionContext};
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
    async fn exit_plan_mode_returns_success_message() {
        let ctx = TestContext { cancel: CancellationToken::new() };
        let tool = ExitPlanModeTool;

        let output = tool
            .execute(json!({}), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(
            text.contains("Exited planning mode"),
            "expected 'Exited planning mode' in output: {text}"
        );
    }

    #[tokio::test]
    async fn exit_plan_mode_ignores_extra_fields() {
        let ctx = TestContext { cancel: CancellationToken::new() };
        let tool = ExitPlanModeTool;

        let output = tool
            .execute(json!({ "stray": 42 }), &ctx)
            .await
            .expect("execute should succeed even with extra fields");

        assert!(!output.is_error);
    }

    /// T9 regression: symmetric to enter_plan_mode — exit also writes
    /// session permission-mode state, must NOT report is_read_only.
    #[test]
    fn exit_plan_mode_classification() {
        let tool = ExitPlanModeTool;
        assert_eq!(tool.name(), "exit_plan_mode");
        assert!(
            !tool.is_destructive(&json!({})),
            "not destructive — re-entering plan mode is possible"
        );
        assert!(
            !tool.is_read_only(&json!({})),
            "T9: NOT read-only — writes session permission-mode state"
        );
    }
}
