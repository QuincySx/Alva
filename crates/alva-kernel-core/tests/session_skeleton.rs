//! Integration test: run a minimal agent with a stub model and verify
//! that the session event stream contains the expected skeleton events
//! in the expected parent chain.

use std::sync::Arc;

use alva_kernel_abi::agent_session::{AgentSession, EmitterKind, EventQuery, InMemoryAgentSession};
use alva_kernel_abi::base::content::ContentBlock;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{AgentMessage, CancellationToken, ModelConfig};
use alva_kernel_core::run::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::middleware::MiddlewareStack;
use async_trait::async_trait;
use futures_core::Stream;

// ---------------------------------------------------------------------------
// Minimal EchoModel — returns one text response then done, no tool calls.
// This ensures the run loop completes without error so run_end is emitted.
// ---------------------------------------------------------------------------

struct EchoModel;

#[async_trait]
impl LanguageModel for EchoModel {
    async fn complete(
        &self,
        messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        let last = messages
            .last()
            .map(|m| m.text_content())
            .unwrap_or_default();
        Ok(CompletionResponse::from_message(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: format!("Echo: {}", last),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }))
    }

    fn stream(
        &self,
        messages: &[Message],
        _: &[&dyn Tool],
        _: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        let last = messages
            .last()
            .map(|m| m.text_content())
            .unwrap_or_default();
        let text = format!("Echo: {}", last);
        Box::pin(futures::stream::iter(vec![
            StreamEvent::Start,
            StreamEvent::TextDelta { text },
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "echo-skeleton-test"
    }
}

// ---------------------------------------------------------------------------
// Integration test
// ---------------------------------------------------------------------------

#[tokio::test]
async fn full_run_produces_skeleton_events_in_order() {
    // Build an in-memory session and retain a reference so we can query
    // it after the run completes.
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());

    let mut state = AgentState {
        model: Arc::new(EchoModel),
        tools: vec![],
        session: session.clone(),
        extensions: Extensions::new(),
    };

    let config = AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: "Skeleton event test.".to_string(),
        max_iterations: 10,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };

    let cancel = CancellationToken::new();
    let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();

    // Drive a real agent run — this is what actually exercises the skeleton
    // event emission in run.rs. We use a single user message so the loop
    // completes after one LLM call.
    run_agent(
        &mut state,
        &config,
        cancel,
        vec![AgentMessage::Standard(Message::user("hello skeleton"))],
        tx,
    )
    .await
    .unwrap();

    // Query all events written to the session in seq order.
    let all_events = session
        .query(&EventQuery {
            limit: 1000,
            ..Default::default()
        })
        .await;

    let event_types: Vec<&str> = all_events
        .iter()
        .map(|em| em.event.event_type.as_str())
        .collect();

    // -----------------------------------------------------------------------
    // 1. Required skeleton event types are present.
    // -----------------------------------------------------------------------
    assert!(
        event_types.contains(&"run_start"),
        "missing run_start; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"component_registry"),
        "missing component_registry; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"iteration_start"),
        "missing iteration_start; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"llm_call_start"),
        "missing llm_call_start; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"llm_call_end"),
        "missing llm_call_end; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"iteration_end"),
        "missing iteration_end; got: {:?}",
        event_types
    );
    assert!(
        event_types.contains(&"run_end"),
        "missing run_end; got: {:?}",
        event_types
    );

    // -----------------------------------------------------------------------
    // 2. Every event has seq assigned (none are 0).
    // -----------------------------------------------------------------------
    for em in &all_events {
        assert_ne!(
            em.event.seq,
            0,
            "event {} ({}) has unassigned seq",
            em.event.uuid,
            em.event.event_type
        );
    }

    // -----------------------------------------------------------------------
    // 3. seq is strictly monotonic (query returns events ordered by seq).
    // -----------------------------------------------------------------------
    let mut prev = 0u64;
    for em in &all_events {
        assert!(
            em.event.seq > prev,
            "seq not strictly monotonic: event {} ({}) has seq {} which is not > {}",
            em.event.uuid,
            em.event.event_type,
            em.event.seq,
            prev
        );
        prev = em.event.seq;
    }

    // -----------------------------------------------------------------------
    // 4. run_start is the parent of component_registry.
    // -----------------------------------------------------------------------
    let run_start = all_events
        .iter()
        .find(|em| em.event.event_type == "run_start")
        .expect("run_start event must be present");
    let component_registry = all_events
        .iter()
        .find(|em| em.event.event_type == "component_registry")
        .expect("component_registry event must be present");
    assert_eq!(
        component_registry.event.parent_uuid.as_deref(),
        Some(run_start.event.uuid.as_str()),
        "component_registry.parent_uuid should equal run_start.uuid"
    );

    // -----------------------------------------------------------------------
    // 5. Every skeleton event has Runtime emitter with id "kernel_core".
    // -----------------------------------------------------------------------
    for em in &all_events {
        if matches!(
            em.event.event_type.as_str(),
            "run_start"
                | "component_registry"
                | "iteration_start"
                | "iteration_end"
                | "llm_call_start"
                | "llm_call_end"
                | "run_end"
        ) {
            assert_eq!(
                em.event.emitter.kind,
                EmitterKind::Runtime,
                "skeleton event {} ({}) must have Runtime emitter, got {:?}",
                em.event.uuid,
                em.event.event_type,
                em.event.emitter.kind
            );
            assert_eq!(
                em.event.emitter.id,
                "kernel_core",
                "skeleton event {} ({}) must have emitter.id == \"kernel_core\", got {:?}",
                em.event.uuid,
                em.event.event_type,
                em.event.emitter.id
            );
        }
    }
}
