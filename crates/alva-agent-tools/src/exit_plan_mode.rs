// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: ExitPlanModeTool
// POS:    Exits planning mode, re-enabling destructive tools.
//! exit_plan_mode — exit planning mode

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
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
    read_only,
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
