// INPUT:  std::sync::Arc, std::time::Duration, alva_types::{LanguageModel, Tool}
// OUTPUT: pub struct SubAgentConfig, pub enum SubAgentModel, pub enum SubAgentTools, pub fn create_task_tool
// POS:    Sub-agent configuration and task-tool factory for spawning child agents within a parent's tool-call cycle.
use std::sync::Arc;
use std::time::Duration;

use alva_types::{
    AgentError, CancellationToken, LanguageModel, Tool, ToolContext, ToolResult,
};
use async_trait::async_trait;
use serde_json::Value;

/// Configuration for a sub-agent that can be spawned as a child task.
///
/// Sub-agents run within a parent agent's tool-call cycle. They inherit
/// (or override) the parent's model and tool set and execute with their
/// own system prompt, turn limit, and timeout.
pub struct SubAgentConfig {
    /// Human-readable name for routing and logging.
    pub name: String,

    /// Description surfaced to the parent model so it can choose the right
    /// sub-agent.
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
    /// Use the same model as the parent agent.
    Inherit,
    /// Use a specific model instance.
    Specific(Arc<dyn LanguageModel>),
}

/// Whether the sub-agent inherits the parent's tools or uses a subset.
pub enum SubAgentTools {
    /// Inherit all tools from the parent (minus `disallowed_tools`).
    Inherit,
    /// Only allow tools whose names appear in the whitelist.
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

/// Creates a "task" tool that spawns sub-agents.
///
/// The returned tool, when invoked by a parent agent, selects the matching
/// sub-agent config by name, spawns a child `Agent`, and returns its output.
///
/// Input schema:
/// ```json
/// {
///   "agent": "agent-name",      // must match a SubAgentConfig.name
///   "task": "description..."     // injected as user message
/// }
/// ```
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

/// Tool implementation that spawns sub-agents from configuration.
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

        // Filter out disallowed tools (including "task" to prevent recursion)
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

        let model = self.resolve_model(config);
        let tools = self.resolve_tools(config);

        // Build sub-agent with its own hooks and run it.
        use alva_agent_core::{Agent, AgentEvent, AgentHooks};
        use alva_types::{AgentMessage, Message};

        // Default convert_to_llm: pass messages through as standard LLM messages
        let convert_fn: alva_agent_core::ConvertToLlmFn = Arc::new(|ctx| {
            ctx.messages
                .iter()
                .filter_map(|m| match m {
                    AgentMessage::Standard(msg) => Some(msg.clone()),
                    _ => None,
                })
                .collect()
        });

        let user_msg = AgentMessage::Standard(Message::user(task));

        let mut hooks = AgentHooks::new(convert_fn);
        hooks.max_iterations = config.max_turns;

        let session_id = format!(
            "sub-{}-{}",
            agent_name,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        let agent = Agent::new(
            model,
            session_id,
            config.system_prompt.clone(),
            hooks,
        );

        // Register resolved tools on the sub-agent
        agent.set_tools(tools).await;

        // Run with timeout
        let timeout = config.timeout;
        let cancel_clone = cancel.clone();
        let result = tokio::time::timeout(timeout, async {
            let mut rx = agent.prompt(vec![user_msg]);
            let mut output = String::new();

            while let Some(event) = rx.recv().await {
                if cancel_clone.is_cancelled() {
                    agent.cancel();
                    break;
                }
                match event {
                    AgentEvent::MessageEnd { message } => {
                        if let AgentMessage::Standard(msg) = &message {
                            output.push_str(&msg.text_content());
                        }
                    }
                    AgentEvent::AgentEnd { error: Some(e) } => {
                        return Err(AgentError::ToolError {
                            tool_name: "task".into(),
                            message: e,
                        });
                    }
                    _ => {}
                }
            }

            Ok(output)
        })
        .await;

        match result {
            Ok(Ok(output)) => Ok(ToolResult {
                content: output,
                is_error: false,
                details: None,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
                agent.cancel();
                Ok(ToolResult {
                    content: format!(
                        "[Sub-agent '{}' timed out after {:?}]",
                        agent_name, config.timeout
                    ),
                    is_error: true,
                    details: None,
                })
            }
        }
    }
}
