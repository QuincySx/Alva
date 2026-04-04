// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: SkillTool
// POS:    Invokes a named skill/command.
//! skill_tool — invoke skills/commands

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Debug, Deserialize)]
struct Input {
    skill: String,
    #[serde(default)]
    args: Option<String>,
}

pub struct SkillTool;

#[async_trait]
impl Tool for SkillTool {
    fn name(&self) -> &str {
        "skill"
    }

    fn description(&self) -> &str {
        "Invoke a skill or command by name. Skills are specialized capabilities registered \
         with the agent framework."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["skill"],
            "properties": {
                "skill": {
                    "type": "string",
                    "description": "Name of the skill to invoke (e.g. 'commit', 'review-pr')"
                },
                "args": {
                    "type": "string",
                    "description": "Optional arguments to pass to the skill"
                }
            }
        })
    }

    fn is_read_only(&self, _input: &Value) -> bool {
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

        let args_info = params.args.as_deref().unwrap_or("(none)");

        // In a full implementation, this would look up the skill from a registry
        // and invoke it with the given arguments.
        Ok(ToolOutput::text(format!(
            "Skill '{}' invoked with args: {}\n\
             Skill execution is not yet wired to the skill registry.",
            params.skill, args_info
        )))
    }
}
