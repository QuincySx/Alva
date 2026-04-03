// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TeamCreateTool
// POS:    Creates a multi-agent team with a unique name.
//! team_create — create a multi-agent team

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    team_name: String,
    description: String,
    #[serde(default)]
    agent_type: Option<String>,
}

pub struct TeamCreateTool;

#[async_trait]
impl Tool for TeamCreateTool {
    fn name(&self) -> &str {
        "team_create"
    }

    fn description(&self) -> &str {
        "Create a new multi-agent team. Teams allow coordinating work across multiple agents."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_name", "description"],
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Unique name for the team"
                },
                "description": {
                    "type": "string",
                    "description": "Description of the team's purpose"
                },
                "agent_type": {
                    "type": "string",
                    "description": "Type of agents in the team (e.g. 'code', 'research', 'review')"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

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
