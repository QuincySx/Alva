// INPUT:  alva_types, alva_agent_core::run_child, std::sync::Arc, std::time::Duration
// OUTPUT: SubAgentConfig, SubAgentModel, SubAgentTools, create_task_tool
// POS:    Developer-constrained sub-agent spawning — pre-defined agent configs, enum selection.
//         Moved from alva-agent-graph (not a graph concept) to plugins where it belongs.

//! Task tool — developer-constrained sub-agent spawning.
//!
//! The developer pre-defines a set of `SubAgentConfig`s (roles, prompts,
//! tool access). The LLM chooses which config to use by name, but cannot
//! create new agents or change their configuration.
//!
//! This is the **developer API** for controlled delegation.
//! For the **AI API** (dynamic roles), see [`super::agent_spawn`].

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;

use alva_agent_core::run_child::{run_child_agent, ChildAgentParams};
use alva_types::base::cancel::CancellationToken;
use alva_types::base::error::AgentError;
use alva_types::model::LanguageModel;
use alva_types::tool::{Tool, ToolContext, ToolResult};

/// Configuration for a sub-agent that can be spawned as a child task.
pub struct SubAgentConfig {
    /// Human-readable name for routing and logging.
    pub name: String,
    /// Description surfaced to the parent model so it can choose the right sub-agent.
    pub description: String,
    /// System prompt prepended to the sub-agent's context.
    pub system_prompt: String,
    /// Which model the sub-agent should use.
    pub model: SubAgentModel,
    /// Which tools the sub-agent has access to.
    pub tools: SubAgentTools,
    /// Tool names the sub-agent is explicitly forbidden from using.
    pub disallowed_tools: Vec<String>,
    /// Maximum number of LLM turns before the sub-agent is stopped.
    pub max_turns: u32,
    /// Wall-clock timeout for the entire sub-agent execution.
    pub timeout: Duration,
}

/// Whether the sub-agent inherits the parent's model or uses its own.
pub enum SubAgentModel {
    Inherit,
    Specific(Arc<dyn LanguageModel>),
}

/// Whether the sub-agent inherits the parent's tools or uses a subset.
pub enum SubAgentTools {
    Inherit,
    Whitelist(Vec<String>),
}

impl Default for SubAgentConfig {
    fn default() -> Self {
        Self {
            name: "general-purpose".into(),
            description: "General-purpose sub-agent".into(),
            system_prompt: String::new(),
            model: SubAgentModel::Inherit,
            tools: SubAgentTools::Inherit,
            disallowed_tools: vec!["task".into()],
            max_turns: 50,
            timeout: Duration::from_secs(900),
        }
    }
}

/// Creates a "task" tool that spawns developer-constrained sub-agents.
pub fn create_task_tool(
    configs: Vec<SubAgentConfig>,
    parent_model: Arc<dyn LanguageModel>,
    parent_tools: Vec<Arc<dyn Tool>>,
) -> Box<dyn Tool> {
    Box::new(SubAgentTool {
        configs: Arc::new(configs),
        parent_model,
        parent_tools: Arc::new(parent_tools),
    })
}

struct SubAgentTool {
    configs: Arc<Vec<SubAgentConfig>>,
    parent_model: Arc<dyn LanguageModel>,
    parent_tools: Arc<Vec<Arc<dyn Tool>>>,
}

impl SubAgentTool {
    fn resolve_model(&self, config: &SubAgentConfig) -> Arc<dyn LanguageModel> {
        match &config.model {
            SubAgentModel::Inherit => self.parent_model.clone(),
            SubAgentModel::Specific(model) => model.clone(),
        }
    }

    fn resolve_tools(&self, config: &SubAgentConfig) -> Vec<Arc<dyn Tool>> {
        let base_tools: Vec<Arc<dyn Tool>> = match &config.tools {
            SubAgentTools::Inherit => self.parent_tools.as_ref().clone(),
            SubAgentTools::Whitelist(names) => self
                .parent_tools
                .iter()
                .filter(|t| names.contains(&t.name().to_string()))
                .cloned()
                .collect(),
        };

        base_tools
            .into_iter()
            .filter(|t| !config.disallowed_tools.contains(&t.name().to_string()))
            .collect()
    }
}

#[async_trait]
impl Tool for SubAgentTool {
    fn name(&self) -> &str {
        "task"
    }

    fn description(&self) -> &str {
        "Delegate a task to a specialized sub-agent"
    }

    fn parameters_schema(&self) -> Value {
        let agent_names: Vec<String> = self.configs.iter().map(|c| c.name.clone()).collect();
        let agent_descriptions: Vec<String> = self
            .configs
            .iter()
            .map(|c| format!("{}: {}", c.name, c.description))
            .collect();

        serde_json::json!({
            "type": "object",
            "required": ["agent", "task"],
            "properties": {
                "agent": {
                    "type": "string",
                    "description": format!(
                        "Name of the sub-agent to use. Available: {}",
                        agent_descriptions.join("; ")
                    ),
                    "enum": agent_names,
                },
                "task": {
                    "type": "string",
                    "description": "Complete task description for the sub-agent"
                }
            }
        })
    }

    async fn execute(
        &self,
        input: Value,
        cancel: &CancellationToken,
        _ctx: &dyn ToolContext,
    ) -> Result<ToolResult, AgentError> {
        let agent_name = input["agent"].as_str().ok_or_else(|| AgentError::ToolError {
            tool_name: "task".into(),
            message: "missing 'agent' field".into(),
        })?;

        let task = input["task"].as_str().ok_or_else(|| AgentError::ToolError {
            tool_name: "task".into(),
            message: "missing 'task' field".into(),
        })?;

        let config = self
            .configs
            .iter()
            .find(|c| c.name == agent_name)
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "task".into(),
                message: format!("unknown sub-agent: {}", agent_name),
            })?;

        let result = run_child_agent(ChildAgentParams {
            model: self.resolve_model(config),
            tools: self.resolve_tools(config),
            system_prompt: config.system_prompt.clone(),
            task: task.to_string(),
            max_iterations: config.max_turns,
            timeout: config.timeout,
            parent_session_id: None,
            cancel: cancel.clone(),
            middleware: None,
            model_config: None,
            context_window: 0,
        })
        .await;

        if result.is_error {
            Ok(ToolResult {
                content: format!(
                    "[Sub-agent '{}' error: {}]\n{}",
                    agent_name,
                    result.error.unwrap_or_default(),
                    result.text
                ),
                is_error: true,
                details: None,
            })
        } else {
            Ok(ToolResult {
                content: result.text,
                is_error: false,
                details: None,
            })
        }
    }
}
