use std::sync::Arc;
use std::time::Duration;

use alva_types::{LanguageModel, Tool};

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
/// The returned tool, when invoked by a parent agent, selects and runs a
/// sub-agent based on the provided configs. Implementation deferred to the
/// integration phase.
pub fn create_task_tool(
    _configs: Vec<SubAgentConfig>,
    _parent_model: Arc<dyn LanguageModel>,
    _parent_tools: Vec<Arc<dyn Tool>>,
) -> Box<dyn Tool> {
    todo!("Implement task tool that spawns sub-agent loops")
}
