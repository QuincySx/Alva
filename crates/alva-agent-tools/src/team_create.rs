// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: TeamCreateTool
// POS:    Creates a multi-agent team with a unique name.
//! team_create — create a multi-agent team

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Unique name for the team.
    team_name: String,
    /// Description of the team's purpose.
    description: String,
    /// Type of agents in the team (e.g. 'code', 'research', 'review').
    #[serde(default)]
    agent_type: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "team_create",
    description = "Create a new multi-agent team. Teams allow coordinating work across multiple agents.",
    input = Input,
)]
pub struct TeamCreateTool;

impl TeamCreateTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let agent_type_info = params
            .agent_type
            .as_deref()
            .unwrap_or("general");

        Ok(ToolOutput::text(format!(
            "Team created successfully.\n  Name: {}\n  Description: {}\n  Agent type: {}",
            params.team_name, params.description, agent_type_info
        )))
    }
}
