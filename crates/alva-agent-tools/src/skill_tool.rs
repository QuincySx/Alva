// INPUT:  alva_types, async_trait, schemars, serde
// OUTPUT: SkillTool
// POS:    Invokes a named skill/command.
//! skill_tool — invoke skills/commands

use alva_types::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name of the skill to invoke (e.g. 'commit', 'review-pr').
    skill: String,
    /// Optional arguments to pass to the skill.
    #[serde(default)]
    args: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "skill",
    description = "Invoke a skill or command by name. Skills are specialized capabilities registered \
        with the agent framework.",
    input = Input,
    read_only,
)]
pub struct SkillTool;

impl SkillTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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
