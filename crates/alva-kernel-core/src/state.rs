// INPUT:  std::sync::Arc, alva_kernel_abi::{LanguageModel, AgentSession, Tool, BusHandle, ModelConfig, ContextSystem}, crate::shared::Extensions
// OUTPUT: AgentState, AgentConfig
// POS:    mutable state and immutable config — AgentConfig carries optional BusHandle and optional ContextSystem.
use std::sync::Arc;

use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::scope::context::ContextSystem;
use alva_kernel_abi::session::AgentSession;
use alva_kernel_abi::tool::Tool;

use alva_kernel_abi::BusHandle;
use alva_kernel_abi::ModelConfig;

use crate::shared::Extensions;

/// mutable state — what the agent "has" at runtime.
///
/// Messages are NOT stored here — they live in `session` (single source of truth).
/// This avoids duplication and keeps the state focused on capabilities.
pub struct AgentState {
    /// The language model used for completion / streaming.
    pub model: Arc<dyn LanguageModel>,
    /// Available tools the agent can invoke.
    pub tools: Vec<Arc<dyn Tool>>,
    /// Session managing message history.
    pub session: Arc<dyn AgentSession>,
    /// Type-safe key-value store for cross-middleware communication.
    pub extensions: Extensions,
}

/// immutable config — logic that doesn't change during a run.
///
/// Separated from `AgentState` so middleware can borrow state mutably
/// while config is borrowed immutably, avoiding Rust borrow conflicts.
pub struct AgentConfig {
    /// The middleware stack for this agent run.
    pub middleware: crate::middleware::MiddlewareStack,
    /// System prompt prepended to every LLM call.
    pub system_prompt: String,
    /// Maximum number of iterations (LLM call + tool execution rounds) before stopping.
    pub max_iterations: u32,
    /// Model configuration (temperature, max_tokens, etc.).
    pub model_config: ModelConfig,
    /// Maximum number of recent messages to include in LLM context.
    /// 0 means no limit (use all messages).
    pub context_window: usize,
    /// Workspace root path — passed to tools via ToolExecutionContext.
    /// None means tools that require a workspace will fail.
    pub workspace: Option<std::path::PathBuf>,
    /// Cross-layer coordination bus. None when bus is not wired.
    pub bus: Option<BusHandle>,
    /// Optional context plugin system. When set, the run loop calls
    /// ContextHooks (`bootstrap` / `on_message` / `after_turn` / `dispose`)
    /// at the matching lifecycle points, letting plugins observe and react
    /// to the message stream. `assemble` and `on_budget_exceeded` are not
    /// wired yet — they require a ContextEntry ↔ Message translation layer.
    /// None means no context plugins, run loop behavior unchanged.
    pub context_system: Option<Arc<ContextSystem>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builtins::test_helpers::helpers::{make_state, StubModel};
    use alva_kernel_abi::session::InMemorySession;
    use alva_kernel_abi::ModelConfig;

    #[test]
    fn agent_state_creation() {
        let state = make_state();

        assert!(state.tools.is_empty());
        assert!(!state.session.id().is_empty());
    }

    #[test]
    fn agent_config_with_system_prompt() {
        let config = AgentConfig {
            middleware: crate::middleware::MiddlewareStack::new(),
            system_prompt: "You are a helpful assistant.".to_string(),
            max_iterations: 100,
            model_config: ModelConfig::default(),
            context_window: 0,
            workspace: None,
            bus: None,
            context_system: None,
        };

        assert_eq!(config.system_prompt, "You are a helpful assistant.");
        assert!(config.middleware.is_empty());
        assert_eq!(config.max_iterations, 100);
    }

    #[test]
    fn extensions_on_state() {
        #[derive(Debug, PartialEq)]
        struct TokenBudget(u32);

        let mut state = AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        };

        state.extensions.insert(TokenBudget(5000));
        assert_eq!(
            state.extensions.get::<TokenBudget>(),
            Some(&TokenBudget(5000))
        );
    }
}
