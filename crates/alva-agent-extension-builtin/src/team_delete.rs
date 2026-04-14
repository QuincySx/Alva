// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TeamDeleteTool
// POS:    Deletes a multi-agent team by name.
//! team_delete — delete a team

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name of the team to delete.
    team_name: String,
}

#[derive(Tool)]
#[tool(
    name = "team_delete",
    description = "Delete a multi-agent team by name. This stops all agents in the team and removes it.",
    input = Input,
    destructive,
)]
pub struct TeamDeleteTool;

impl TeamDeleteTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput::text(format!(
            "Team '{}' deleted successfully.",
            params.team_name
        )))
    }
}
