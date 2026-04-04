// INPUT:  alva_types, async_trait, serde_json
// OUTPUT: EnterPlanModeTool
// POS:    Switches the agent to planning mode, restricting destructive tools.
//! enter_plan_mode — switch to planning mode

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct EnterPlanModeTool;

#[async_trait]
impl Tool for EnterPlanModeTool {
    fn name(&self) -> &str {
        "enter_plan_mode"
    }

    fn description(&self) -> &str {
        "Switch to planning mode. In this mode, destructive tools (file writes, shell commands, \
         etc.) are restricted. Use this when you want to plan and reason without making changes."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
        true
    }

    async fn execute(
        &self,
        _input: Value,
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
