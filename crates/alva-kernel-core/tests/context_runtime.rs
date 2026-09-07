use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alva_kernel_abi::scope::context::{
    ContextEntry, ContextHandle, ContextHooks, ContextSystem, IngestAction, Injection,
    NoopContextHandle, Priority, PromptSection,
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

struct LifecycleHooks {
    assembled_budget: Arc<AtomicUsize>,
    ingested: Arc<AtomicUsize>,
}

#[async_trait]
impl ContextHooks for LifecycleHooks {
    async fn ingest(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        _entry: &ContextEntry,
    ) -> IngestAction {
        self.ingested.fetch_add(1, Ordering::SeqCst);
        IngestAction::Keep
    }

    async fn assemble(
        &self,
        _sdk: &dyn ContextHandle,
        _agent_id: &str,
        entries: Vec<ContextEntry>,
        token_budget: usize,
    ) -> Vec<ContextEntry> {
        self.assembled_budget.store(token_budget, Ordering::SeqCst);
        entries
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
        // Ticket 10: execution lives in the context capability library —
        // the fixture attaches the default executor, exactly like
        // `default_context_system()` does in production.
        context_system: Some(Arc::new(
            ContextSystem::new(hooks, handle)
                .with_apply(Arc::new(alva_agent_context::DefaultContextApply)),
        )),
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

#[tokio::test]
async fn context_runtime_drives_ingest_and_passes_the_configured_budget() {
    let assembled_budget = Arc::new(AtomicUsize::new(0));
    let ingested = Arc::new(AtomicUsize::new(0));
    let hooks: Arc<dyn ContextHooks> = Arc::new(LifecycleHooks {
        assembled_budget: assembled_budget.clone(),
        ingested: ingested.clone(),
    });
    let mut config = config_with_context();
    config.context_system = Some(Arc::new(ContextSystem::new(
        hooks,
        Arc::new(NoopContextHandle),
    )));
    config.context_token_budget = Some(321);

    let mut runtime = ContextRuntime::new("agent-1");
    let message = AgentMessage::Standard(Message::user("hello"));
    runtime.on_message(&config, &message).await;
    let mut prompt = vec![];
    let _ = runtime
        .prepare_llm_context(&config, &mut prompt, vec![message])
        .await;

    assert_eq!(ingested.load(Ordering::SeqCst), 1);
    assert_eq!(assembled_budget.load(Ordering::SeqCst), 321);
}
