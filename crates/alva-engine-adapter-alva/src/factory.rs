// INPUT:  alva_app_core::{BaseAgent, BaseAgentBuilder}, alva_kernel_abi::LanguageModel, alva_engine_runtime::RuntimeRequest
// OUTPUT: BaseAgentFactory, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig
// POS:    Per-engine-session BaseAgent construction boundary, preserving app harness assembly instead of raw kernel loops.

use std::path::PathBuf;
use std::sync::Arc;

use alva_app_core::{BaseAgent, EngineError};
use alva_engine_runtime::RuntimeRequest;
use alva_kernel_abi::LanguageModel;
use async_trait::async_trait;

/// Builds a fresh complete `BaseAgent` for each new engine session.
#[async_trait]
pub trait BaseAgentFactory: Send + Sync {
    async fn build(&self, request: &RuntimeRequest) -> Result<BaseAgent, EngineError>;
}

/// Standard factory input for the minimal Hub vertical slice.
#[derive(Clone)]
pub struct DefaultBaseAgentFactoryConfig {
    pub model: Arc<dyn LanguageModel>,
    pub workspace: PathBuf,
    pub system_prompt: String,
    pub max_iterations: u32,
}

impl DefaultBaseAgentFactoryConfig {
    pub fn new(model: Arc<dyn LanguageModel>, workspace: impl Into<PathBuf>) -> Self {
        Self {
            model,
            workspace: workspace.into(),
            system_prompt: "You are a helpful AI assistant.".into(),
            max_iterations: 100,
        }
    }
}

/// Standard `BaseAgent` baseline. It preserves default-plugin replacement,
/// security, memory, and system context but does not reproduce the native
/// CLI/Tauri component catalog; products can provide a richer factory.
pub struct DefaultBaseAgentFactory {
    config: DefaultBaseAgentFactoryConfig,
}

impl DefaultBaseAgentFactory {
    pub fn new(config: DefaultBaseAgentFactoryConfig) -> Self {
        Self { config }
    }
}

#[async_trait]
impl BaseAgentFactory for DefaultBaseAgentFactory {
    async fn build(&self, request: &RuntimeRequest) -> Result<BaseAgent, EngineError> {
        let workspace = request
            .working_directory
            .as_ref()
            .unwrap_or(&self.config.workspace);
        let system_prompt = request
            .system_prompt
            .as_deref()
            .unwrap_or(&self.config.system_prompt);
        let max_iterations = request
            .options
            .max_turns
            .unwrap_or(self.config.max_iterations);

        BaseAgent::builder()
            .workspace(workspace)
            .system_prompt(system_prompt)
            .max_iterations(max_iterations)
            .build(self.config.model.clone())
            .await
    }
}
