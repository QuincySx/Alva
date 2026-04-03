// INPUT:  alva_types, async_trait, serde_json
// OUTPUT: ExitPlanModeTool
// POS:    Exits planning mode, re-enabling destructive tools.
//! exit_plan_mode — exit planning mode

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde_json::{json, Value};

pub struct ExitPlanModeTool;

#[async_trait]
impl Tool for ExitPlanModeTool {
    fn name(&self) -> &str {
        "exit_plan_mode"
    }

    fn description(&self) -> &str {
        "Exit planning mode and return to normal operation. Destructive tools \
         will be available again."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "properties": {}
        })
    }

    async fn execute(
        &self,
        _input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        // In a full implementation, this would clear the planning-mode flag
        // on the session/context.
        Ok(ToolOutput::text(
            "Exited planning mode. All tools are now available."
        ))
    }
}
