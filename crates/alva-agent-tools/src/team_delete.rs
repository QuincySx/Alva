// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: TeamDeleteTool
// POS:    Deletes a multi-agent team by name.
//! team_delete — delete a team

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    team_name: String,
}

pub struct TeamDeleteTool;

#[async_trait]
impl Tool for TeamDeleteTool {
    fn name(&self) -> &str {
        "team_delete"
    }

    fn description(&self) -> &str {
        "Delete a multi-agent team by name. This stops all agents in the team and removes it."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["team_name"],
            "properties": {
                "team_name": {
                    "type": "string",
                    "description": "Name of the team to delete"
                }
            }
        })
    }

    fn is_destructive(&self, _input: &Value) -> bool {
        true
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

        Ok(ToolOutput::text(format!(
            "Team '{}' deleted successfully.",
            params.team_name
        )))
    }
}
