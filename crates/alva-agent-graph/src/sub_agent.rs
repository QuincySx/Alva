// INPUT:  std::sync::Arc, std::time::Duration, alva_types::{LanguageModel, Tool}
// OUTPUT: pub struct SubAgentConfig, pub enum SubAgentModel, pub enum SubAgentTools, pub fn create_task_tool
// POS:    Sub-agent configuration and task-tool factory for spawning child agents within a parent's tool-call cycle.
//         Updated to use V2 engine (run_agent + AgentState + AgentConfig).
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

        // Build V2 sub-agent state + config and run it.
        use alva_agent_core::middleware::MiddlewareStack;
        use alva_agent_core::state::{AgentConfig, AgentState};
        use alva_agent_core::run::run_agent;
        use alva_agent_core::event::AgentEvent;
        use alva_agent_core::shared::Extensions;
        use alva_types::{AgentMessage, Message};
        use alva_types::session::InMemorySession;

        let session: Arc<dyn alva_types::session::AgentSession> =
            Arc::new(InMemorySession::new());

        let mut state = AgentState {
            model,
            tools,
            session,
            extensions: Extensions::new(),
        };

        let agent_config = AgentConfig {
            middleware: MiddlewareStack::new(),
            system_prompt: config.system_prompt.clone(),
            max_iterations: 100,
            model_config: alva_types::ModelConfig::default(),
            context_window: 0,
        };

        let user_msg = AgentMessage::Standard(Message::user(task));
        let child_cancel = cancel.clone();
        let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel();

        // Run with timeout
        let timeout = config.timeout;
        let result = tokio::time::timeout(timeout, async {
            let run_result = run_agent(
                &mut state,
                &agent_config,
                child_cancel,
                vec![user_msg],
                event_tx,
            )
            .await;

            // Collect output from events
            let mut output = String::new();
            while let Ok(event) = event_rx.try_recv() {
                match event {
                    AgentEvent::MessageEnd { message } => {
                        if let AgentMessage::Standard(msg) = &message {
                            output.push_str(&msg.text_content());
                        }
                    }
                    _ => {}
                }
            }

            match run_result {
                Ok(()) => Ok(output),
                Err(e) => Err(AgentError::ToolError {
                    tool_name: "task".into(),
                    message: e.to_string(),
                }),
            }
        })
        .await;

        // Fallback: get output from session messages
        let session_output: String = state
            .session
            .messages()
            .iter()
            .filter_map(|m| {
                if let AgentMessage::Standard(msg) = m {
                    if msg.role == alva_types::MessageRole::Assistant {
                        let text = msg.text_content();
                        if !text.is_empty() {
                            return Some(text);
                        }
                    }
                }
                None
            })
            .collect::<Vec<_>>()
            .join("\n");

        match result {
            Ok(Ok(output)) => Ok(ToolResult {
                content: if output.is_empty() { session_output } else { output },
                is_error: false,
                details: None,
            }),
            Ok(Err(e)) => Err(e),
            Err(_) => {
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
