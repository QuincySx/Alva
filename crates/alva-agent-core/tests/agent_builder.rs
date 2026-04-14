//! Integration tests for `alva_agent_core::Agent` + `AgentBuilder`.

use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio_stream::empty;

use alva_agent_core::AgentBuilder;
use alva_kernel_abi::{
    AgentError, CompletionResponse, LanguageModel, Message, ModelConfig, StreamEvent, Tool,
};

/// Stub model used to satisfy `AgentBuilder::model(...)`. Its methods are
/// never actually invoked in these tests — we only exercise the build
/// pipeline, not the agent loop.
struct DummyModel;

#[async_trait]
impl LanguageModel for DummyModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message::system("ok")))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        Box::pin(empty())
    }

    fn model_id(&self) -> &str {
        "dummy-model"
    }
}

#[tokio::test]
async fn build_minimal_agent_no_extensions() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .system_prompt("you are a test agent")
        .max_iterations(1)
        .build()
        .await
        .expect("build should succeed");

    // No capabilities have been registered on the bus yet — sanity check
    // that `bus()` returns a real handle whose capability map is empty for
    // an arbitrary marker type.
    assert!(!agent.bus().has::<u32>());
}

#[tokio::test]
async fn builder_requires_model() {
    let result = AgentBuilder::new()
        .system_prompt("no model set")
        .build()
        .await;
    assert!(result.is_err(), "build without model must fail");
}
