use std::sync::Arc;

use alva_kernel_abi::scope::context::{
    ContextHandle, ContextHooks, ContextSystem, Injection, NoopContextHandle, Priority,
    PromptSection,
};
use alva_kernel_abi::{AgentMessage, Message, ModelConfig};
use alva_kernel_core::context_runtime::ContextRuntime;
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::state::AgentConfig;
use async_trait::async_trait;

struct PromptInjectingHooks;

#[async_trait]
impl ContextHooks for PromptInjectingHooks {
    fn name(&self) -> &str {
        "prompt-injecting"
    }

    async fn on_message(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _message: &AgentMessage,
    ) -> Vec<Injection> {
        vec![Injection::system_prompt(PromptSection {
            id: "test".to_string(),
            content: "context-injected".to_string(),
            priority: Priority::Normal,
        })]
    }
}

fn config_with_context() -> AgentConfig {
    let hooks: Arc<dyn ContextHooks> = Arc::new(PromptInjectingHooks);
    let handle: Arc<dyn ContextHandle> = Arc::new(NoopContextHandle);

    AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: Vec::new(),
        max_iterations: 10,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: Some(Arc::new(ContextSystem::new(hooks, handle))),
        context_token_budget: None,
    }
}

#[tokio::test]
async fn context_runtime_applies_pending_injections_before_llm_request() {
    let config = config_with_context();
    let mut runtime = ContextRuntime::new("agent-1");
    let message = AgentMessage::Standard(Message::user("hello"));

    runtime.on_message(&config, &message).await;

    let mut system_prompt = Vec::new();
    let working = runtime
        .prepare_llm_context(&config, &mut system_prompt, vec![message.clone()])
        .await;

    assert_eq!(system_prompt.len(), 1);
    assert!(system_prompt[0].contains("context-injected"));
    assert_eq!(working.len(), 1);
    match &working[0] {
        AgentMessage::Standard(message) => assert_eq!(message.text_content(), "hello"),
        other => panic!("expected standard message, got {other:?}"),
    }
}
