use std::sync::Arc;
use alva_types::{LanguageModel, Tool, ToolContext};
use alva_agent_core::{ConvertToLlmFn, ToolExecutionMode};

/// Configuration for the Alva engine adapter.
pub struct AlvaAdapterConfig {
    /// LLM model instance.
    pub model: Arc<dyn LanguageModel>,
    /// convert_to_llm hook (required by AgentHooks).
    pub convert_to_llm: ConvertToLlmFn,
    /// Tool set available to the agent.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Tool context for execution.
    pub tool_context: Arc<dyn ToolContext>,
    /// Tool execution mode (parallel or sequential).
    pub tool_execution: ToolExecutionMode,
    /// Maximum agentic turns (0 = use AgentHooks default of 100).
    pub max_iterations: u32,
    /// Enable streaming deltas.
    pub streaming: bool,
}
