// INPUT:  std::sync::Arc, alva_types::{LanguageModel, Tool, BusHandle}
// OUTPUT: pub struct AlvaAdapterConfig
// POS:    Configuration for the Alva native engine adapter using agent types.

use alva_types::{BusHandle, LanguageModel, Tool};
use std::sync::Arc;

/// Configuration for the Alva engine adapter.
pub struct AlvaAdapterConfig {
    /// LLM model instance.
    pub model: Arc<dyn LanguageModel>,
    /// Tool set available to the agent.
    pub tools: Vec<Arc<dyn Tool>>,
    /// System prompt (can be overridden per request).
    pub system_prompt: String,
    /// Maximum agentic turns (default: 100).
    pub max_iterations: u32,
    /// Enable streaming deltas.
    pub streaming: bool,
    /// Optional event bus handle passed down to tool execution contexts.
    pub bus: Option<BusHandle>,
}
