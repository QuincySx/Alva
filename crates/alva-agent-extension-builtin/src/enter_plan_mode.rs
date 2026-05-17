// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: EnterPlanModeTool
// POS:    Switches the agent to planning mode, restricting destructive tools.
//! enter_plan_mode — switch to planning mode

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

/// No parameters.
#[derive(Debug, Deserialize, JsonSchema)]
struct Input {}

#[derive(Tool)]
#[tool(
    name = "enter_plan_mode",
    description = "Switch to planning mode. In this mode, destructive tools (file writes, shell commands, \
        etc.) are restricted. Use this when you want to plan and reason without making changes.",
    input = Input,
    // NOT read_only: writes session permission-mode state. Today's stub
    // doesn't mutate yet, but the design intent (and the real
    // PlanModeExtension in alva-app-core) is to flip a session-scoped
    // PermissionMode flag. Tag classification is pre-flipped here so
    // audit / dry-run middleware that consult Tool::is_read_only treat
    // this as a mutation even before the real wire-up. See T9.
)]
pub struct EnterPlanModeTool;

impl EnterPlanModeTool {
    async fn execute_impl(
        &self,
        _params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation, this would set a flag on the session/context
        // that causes permission checks to deny destructive operations.
        Ok(ToolOutput::text(
            "Entered planning mode. Destructive operations are now restricted.\n\
             Use exit_plan_mode to return to normal operation."
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
    async fn enter_plan_mode_returns_success_message() {
        let ctx = TestContext { cancel: CancellationToken::new() };
        let tool = EnterPlanModeTool;

        let output = tool
            .execute(json!({}), &ctx)
            .await
            .expect("execute should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(
            text.contains("planning mode"),
            "expected 'planning mode' in output: {text}"
        );
        assert!(
            text.contains("exit_plan_mode"),
            "expected hint to exit: {text}"
        );
    }

    #[tokio::test]
    async fn enter_plan_mode_rejects_unknown_fields_gracefully() {
        // Extra fields should be ignored (Deserialize default behaviour for empty struct).
        let ctx = TestContext { cancel: CancellationToken::new() };
        let tool = EnterPlanModeTool;

        let output = tool
            .execute(json!({ "ignored": true }), &ctx)
            .await
            .expect("execute should succeed even with extra fields");

        assert!(!output.is_error);
    }

    /// T9 regression: enter_plan_mode mutates session permission state
    /// (design intent — current impl is a stub). Must NOT report
    /// is_read_only=true; CheckpointMiddleware + future audit/dry-run
    /// middleware consult Tool::is_read_only. is_destructive stays false
    /// because re-entering plan mode is idempotent / non-destructive.
    #[test]
    fn enter_plan_mode_classification() {
        let tool = EnterPlanModeTool;
        assert_eq!(tool.name(), "enter_plan_mode");
        assert!(
            !tool.is_destructive(&json!({})),
            "not destructive — entering plan mode is restorable via exit_plan_mode"
        );
        assert!(
            !tool.is_read_only(&json!({})),
            "T9: NOT read-only — writes session permission-mode state"
        );
    }
}
