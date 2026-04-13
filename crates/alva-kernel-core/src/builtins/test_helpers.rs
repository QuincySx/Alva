// INPUT:  alva_kernel_abi, async_trait, futures, std::sync::Arc, crate::{shared::Extensions, state::AgentState}
// OUTPUT: StubModel, make_state()
// POS:    Shared test helpers for builtins — StubModel and AgentState factory used across multiple test modules.

#[cfg(test)]
pub(crate) mod helpers {
    use std::sync::Arc;

    use alva_kernel_abi::base::error::AgentError;
    use alva_kernel_abi::base::message::Message;
    use alva_kernel_abi::base::stream::StreamEvent;
    use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
    use alva_kernel_abi::session::InMemorySession;
    use alva_kernel_abi::tool::Tool;
    use alva_kernel_abi::ModelConfig;
    use async_trait::async_trait;
    use futures_core::Stream;

    use crate::shared::Extensions;
    use crate::state::AgentState;

    /// Minimal LanguageModel stub for unit tests that never calls complete/stream.
    pub(crate) struct StubModel;

    #[async_trait]
    impl LanguageModel for StubModel {
        async fn complete(
            &self,
            _: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> Result<CompletionResponse, AgentError> {
            unreachable!()
        }

        fn stream(
            &self,
            _: &[Message],
            _: &[&dyn Tool],
            _: &ModelConfig,
        ) -> std::pin::Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
            Box::pin(futures::stream::empty())
        }

        fn model_id(&self) -> &str {
            "stub"
        }
    }

    /// Create a minimal `AgentState` for testing middlewares.
    pub(crate) fn make_state() -> AgentState {
        AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        }
    }
}
