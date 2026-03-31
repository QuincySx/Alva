// INPUT:  std::sync::Arc, alva_types::{LanguageModel, AgentSession, Tool}, crate::shared::Extensions
// OUTPUT: AgentState, AgentConfig
// POS:    V2 mutable state and immutable config — separated to avoid Rust borrow conflicts.
use std::sync::Arc;

use alva_types::model::LanguageModel;
use alva_types::session::AgentSession;
use alva_types::tool::Tool;

use alva_types::ModelConfig;

use crate::shared::Extensions;

/// V2 mutable state — what the agent "has" at runtime.
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

/// V2 immutable config — logic that doesn't change during a run.
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
    /// Optional loop hook for runtime message injection (steering, follow-up).
    pub loop_hook: Option<Arc<dyn crate::pending_queue::AgentLoopHook>>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::session::InMemorySession;
    use alva_types::ModelConfig;

    // -----------------------------------------------------------------------
    // MockModel — minimal LanguageModel for testing
    // -----------------------------------------------------------------------
    use alva_types::base::content::ContentBlock;
    use alva_types::base::error::AgentError;
    use alva_types::base::message::{Message, MessageRole};
    use alva_types::base::stream::StreamEvent;
    use async_trait::async_trait;
    use futures_core::Stream;
    use std::pin::Pin;

    fn assistant_msg(text: &str) -> Message {
        Message {
            id: "mock-id".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
    }

    struct MockModel;

    #[async_trait]
    impl LanguageModel for MockModel {
        async fn complete(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Result<Message, AgentError> {
            Ok(assistant_msg("mock response"))
        }

        fn stream(
            &self,
            _messages: &[Message],
            _tools: &[&dyn Tool],
            _config: &ModelConfig,
        ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }

        fn model_id(&self) -> &str {
            "mock-model"
        }
    }

    #[test]
    fn agent_state_creation() {
        let state = AgentState {
            model: Arc::new(MockModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),

        };

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
            loop_hook: None,
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
            model: Arc::new(MockModel),
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
