use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, CompletionResponse, ContentBlock, LanguageModel,
    Message, MessageRole, ModelConfig, StreamEvent, Tool,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, MiddlewareStack};
use alva_kernel_core::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use async_trait::async_trait;
use futures_core::Stream;

struct EchoModel;

#[async_trait]
impl LanguageModel for EchoModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message {
            id: "assistant-1".to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text: "ok".into() }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        Box::pin(futures::stream::iter(vec![
            StreamEvent::TextDelta { text: "ok".into() },
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "echo"
    }
}

struct InputCommittedRecorder {
    calls: Arc<AtomicUsize>,
}

fn agent_message_text(message: &AgentMessage) -> String {
    match message {
        AgentMessage::Standard(message)
        | AgentMessage::Steering(message)
        | AgentMessage::FollowUp(message) => message.text_content(),
        AgentMessage::Marker(_) => String::new(),
        AgentMessage::Extension { data, .. } => data.to_string(),
    }
}

#[async_trait]
impl Middleware for InputCommittedRecorder {
    fn name(&self) -> &str {
        "input-recorder"
    }

    async fn input_committed(
        &self,
        state: &mut AgentState,
        message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        assert_eq!(agent_message_text(message), "hello");
        let messages = state.session.messages().await;
        assert!(
            messages
                .iter()
                .any(|stored| agent_message_text(stored) == "hello"),
            "input_committed must run after the input is appended to the session"
        );
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn input_committed_hook_runs_after_input_is_persisted() {
    let calls = Arc::new(AtomicUsize::new(0));
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let mut middleware = MiddlewareStack::new();
    middleware.push(Arc::new(InputCommittedRecorder {
        calls: calls.clone(),
    }));

    let mut state = AgentState {
        model: Arc::new(EchoModel),
        tools: Vec::new(),
        session,
        extensions: Extensions::new(),
    };
    let config = AgentConfig {
        middleware,
        system_prompt: Vec::new(),
        max_iterations: 4,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();

    run_agent(
        &mut state,
        &config,
        CancellationToken::new(),
        vec![AgentMessage::Standard(Message::user("hello"))],
        event_tx,
    )
    .await
    .expect("run should succeed");

    assert_eq!(calls.load(Ordering::SeqCst), 1);
}
