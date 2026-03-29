// INPUT:  std::sync::Arc, alva_types::{LanguageModel, Tool}
// OUTPUT: pub struct AlvaAdapterConfig
// POS:    Configuration for the Alva native engine adapter using V2 types.

use std::sync::Arc;
use alva_types::{LanguageModel, Tool};

/// Configuration for the Alva engine adapter (V2).
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
}
