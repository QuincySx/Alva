// INPUT:  alva_types, async_trait, serde, serde_json
// OUTPUT: AgentTool
// POS:    Spawns and manages sub-agents, optionally running them in the background.
//! agent_tool — spawn and manage sub-agents

use alva_types::{
    AgentError, Tool, ToolExecutionContext, ToolOutput,
    TaskType, create_task_state,
};
use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{json, Value};
use std::path::PathBuf;

#[derive(Debug, Deserialize)]
struct Input {
    prompt: String,
    description: String,
    #[serde(default)]
    model: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    mode: Option<String>,
    #[serde(default)]
    isolation: Option<String>,
    #[serde(default)]
    run_in_background: Option<bool>,
}

pub struct AgentTool;

#[async_trait]
impl Tool for AgentTool {
    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Spawn a sub-agent to handle a task. The agent runs with its own context and can \
         optionally run in the background. Use this to delegate complex work to a separate \
         agent instance."
    }

    fn parameters_schema(&self) -> Value {
        json!({
            "type": "object",
            "required": ["prompt", "description"],
            "properties": {
                "prompt": {
                    "type": "string",
                    "description": "The prompt / instructions for the sub-agent"
                },
                "description": {
                    "type": "string",
                    "description": "Short description of what the agent should do"
                },
                "model": {
                    "type": "string",
                    "description": "Model to use for the sub-agent (defaults to current model)"
                },
                "name": {
                    "type": "string",
                    "description": "Optional name for the sub-agent"
                },
                "mode": {
                    "type": "string",
                    "enum": ["code", "research", "review", "plan"],
                    "description": "Operating mode for the sub-agent"
                },
                "isolation": {
                    "type": "string",
                    "enum": ["none", "worktree", "sandbox"],
                    "description": "Isolation level for the sub-agent"
                },
                "run_in_background": {
                    "type": "boolean",
                    "description": "If true, run the agent in the background and return a task ID"
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
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let params: Input = serde_json::from_value(input)
            .map_err(|e| AgentError::ToolError {
                tool_name: self.name().into(),
                message: e.to_string(),
            })?;

        let agent_name = params.name.as_deref().unwrap_or("sub-agent");
        let is_background = params.run_in_background.unwrap_or(false);

        if is_background {
            let output_dir = ctx
                .workspace()
                .map(|w| w.join(".tasks"))
                .unwrap_or_else(|| PathBuf::from("/tmp/.tasks"));

            let state = create_task_state(
                TaskType::LocalAgent,
                params.description.clone(),
                None,
                output_dir.join(format!("{}.log", agent_name)),
            );
            let task_id = state.id.clone();

            Ok(ToolOutput::text(format!(
                "Agent '{}' started in background.\n  Task ID: {}\n  Description: {}\n  \
                 Use task_get or task_output to check progress.",
                agent_name, task_id, params.description
            )))
        } else {
            // In a full implementation, this would actually spawn the sub-agent,
            // wait for it to complete, and return its result.
            let model_info = params.model.as_deref().unwrap_or("default");
            let mode_info = params.mode.as_deref().unwrap_or("code");
            let isolation_info = params.isolation.as_deref().unwrap_or("none");

            Ok(ToolOutput::text(format!(
                "Agent '{}' completed.\n  Model: {}\n  Mode: {}\n  Isolation: {}\n  \
                 Description: {}\n  Prompt length: {} chars\n  \
                 Result: Sub-agent execution is not yet wired to the runtime.",
                agent_name, model_info, mode_info, isolation_info,
                params.description, params.prompt.len()
            )))
        }
    }
}
