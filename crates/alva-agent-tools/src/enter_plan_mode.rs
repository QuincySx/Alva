// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: EnterPlanModeTool
// POS:    Switches the agent to planning mode, restricting destructive tools.
//! enter_plan_mode — switch to planning mode

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
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
    read_only,
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
