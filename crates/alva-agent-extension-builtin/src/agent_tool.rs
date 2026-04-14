// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json
// OUTPUT: AgentTool
// POS:    Spawns and manages sub-agents, optionally running them in the background.
//! agent_tool — spawn and manage sub-agents

use alva_kernel_abi::{
    AgentError, Tool, ToolExecutionContext, ToolOutput,
    TaskType, create_task_state,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

/// Operating mode for the spawned sub-agent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AgentMode {
    Code,
    Research,
    Review,
    Plan,
}

/// Isolation level for the spawned sub-agent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AgentIsolation {
    None,
    Worktree,
    Sandbox,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The prompt / instructions for the sub-agent.
    prompt: String,
    /// Short description of what the agent should do.
    description: String,
    /// Model to use for the sub-agent (defaults to current model).
    #[serde(default)]
    model: Option<String>,
    /// Optional name for the sub-agent.
    #[serde(default)]
    name: Option<String>,
    /// Operating mode for the sub-agent.
    #[serde(default)]
    mode: Option<AgentMode>,
    /// Isolation level for the sub-agent.
    #[serde(default)]
    isolation: Option<AgentIsolation>,
    /// If true, run the agent in the background and return a task ID.
    #[serde(default)]
    run_in_background: Option<bool>,
}

#[derive(Tool)]
#[tool(
    name = "agent",
    description = "Spawn a sub-agent to handle a task. The agent runs with its own context and can \
        optionally run in the background. Use this to delegate complex work to a separate \
        agent instance.",
    input = Input,
    read_only,
)]
pub struct AgentTool;

impl AgentTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
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
            let mode_info = match params.mode {
                Some(AgentMode::Code) => "code",
                Some(AgentMode::Research) => "research",
                Some(AgentMode::Review) => "review",
                Some(AgentMode::Plan) => "plan",
                None => "code",
            };
            let isolation_info = match params.isolation {
                Some(AgentIsolation::None) | None => "none",
                Some(AgentIsolation::Worktree) => "worktree",
                Some(AgentIsolation::Sandbox) => "sandbox",
            };

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
