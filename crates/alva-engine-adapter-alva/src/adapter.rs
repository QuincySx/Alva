// INPUT:  AlvaAdapterConfig, EventMapper, alva_agent_core (V2), alva_engine_runtime::*
// OUTPUT: AlvaAdapter — EngineRuntime implementation that wraps V2 agent engine
// POS:    Core adapter bridging the V2 agent engine to the unified EngineRuntime interface.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::debug;

use alva_agent_core::middleware::MiddlewareStack;
use alva_agent_core::state::{AgentConfig, AgentState};
use alva_agent_core::run::run_agent;
use alva_agent_core::shared::Extensions;
use alva_agent_core::AgentMessage;
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use alva_types::{CancellationToken, ContentBlock, Message, MessageRole};
use alva_types::session::InMemorySession;

use crate::config::AlvaAdapterConfig;
use crate::mapping::EventMapper;

// ---------------------------------------------------------------------------
// Session handle — kept alive while the agent loop is running
// ---------------------------------------------------------------------------

struct SessionHandle {
    cancel: CancellationToken,
}

// ---------------------------------------------------------------------------
// AlvaAdapter
// ---------------------------------------------------------------------------

/// `EngineRuntime` adapter that wraps V2 agent engine.
///
/// Each `execute()` call creates a fresh `AgentState` + `AgentConfig`, runs
/// `run_agent` in a spawned task, and returns a `RuntimeEvent` stream.
pub struct AlvaAdapter {
    config: AlvaAdapterConfig,
    sessions: Arc<Mutex<HashMap<String, SessionHandle>>>,
}

impl AlvaAdapter {
    /// Create a new adapter from the given configuration.
    pub fn new(config: AlvaAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for AlvaAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        // 1. Generate a unique session ID.
        let session_id = uuid::Uuid::new_v4().to_string();

        // 2. Determine the system prompt.
        let system_prompt = request
            .system_prompt
            .unwrap_or_else(|| self.config.system_prompt.clone());

        // 3. Build V2 AgentState.
        let session: Arc<dyn alva_types::session::AgentSession> =
            Arc::new(InMemorySession::new());
        let state = AgentState {
            model: self.config.model.clone(),
            tools: self.config.tools.clone(),
            session,
            extensions: Extensions::new(),
        };

        // 4. Build V2 AgentConfig.
        let config = AgentConfig {
            middleware: MiddlewareStack::new(),
            system_prompt,
            max_iterations: 100,
            model_config: alva_types::ModelConfig::default(),
        };

        // 5. Build the user message from request.prompt.
        let user_message = AgentMessage::Standard(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: request.prompt,
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        });

        // 6. Create the output channel for RuntimeEvents.
        let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();

        // 7. Create cancellation token.
        let cancel = CancellationToken::new();

        // 8. Clone what the spawned task needs.
        let sessions = self.sessions.clone();
        let sid = session_id.clone();
        let cancel_clone = cancel.clone();

        // 9. Register the session handle.
        {
            let sessions_clone = sessions.clone();
            let sid_clone = sid.clone();
            let cancel_for_handle = cancel.clone();
            tokio::spawn(async move {
                let mut map = sessions_clone.lock().await;
                map.insert(sid_clone, SessionHandle { cancel: cancel_for_handle });
            });
        }

        // 10. Spawn the background task.
        tokio::spawn(async move {
            debug!(session_id = %sid, "AlvaAdapter: starting V2 agent");

            // Create a mapper to convert AgentEvents to RuntimeEvents.
            let (agent_tx, mut agent_rx) = mpsc::unbounded_channel();

            // Run the V2 agent loop.
            let mut state = state;
            let run_handle = tokio::spawn(async move {
                let _ = run_agent(
                    &mut state,
                    &config,
                    cancel_clone,
                    vec![user_message],
                    agent_tx,
                )
                .await;
            });

            // Map AgentEvents to RuntimeEvents.
            let mut mapper = EventMapper::new(sid.clone());

            while let Some(agent_event) = agent_rx.recv().await {
                let runtime_events = mapper.map(agent_event);
                for re in runtime_events {
                    if event_tx.send(re).is_err() {
                        debug!(session_id = %sid, "AlvaAdapter: consumer dropped, stopping");
                        break;
                    }
                }
            }

            // Wait for the run task to complete.
            let _ = run_handle.await;

            // Clean up the session.
            {
                let mut map = sessions.lock().await;
                map.remove(&sid);
            }

            debug!(session_id = %sid, "AlvaAdapter: session completed");
        });

        // 11. Wrap the receiver as a Stream.
        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let map = self.sessions.lock().await;
        match map.get(session_id) {
            Some(handle) => {
                handle.cancel.cancel();
                Ok(())
            }
            None => Err(RuntimeError::SessionNotFound(session_id.to_string())),
        }
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let map = self.sessions.lock().await;
        if map.contains_key(session_id) {
            Err(RuntimeError::PermissionNotFound(
                "permission callback not implemented in v2".to_string(),
            ))
        } else {
            Err(RuntimeError::SessionNotFound(session_id.to_string()))
        }
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: false,
            resume: false,
            cancel: true,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use tokio_stream::StreamExt;

    fn make_config(model: MockLanguageModel) -> AlvaAdapterConfig {
        AlvaAdapterConfig {
            model: Arc::new(model),
            tools: vec![],
            system_prompt: "You are a helpful assistant.".to_string(),
            max_iterations: 1,
            streaming: false,
        }
    }

    #[tokio::test]
    async fn test_execute_simple_prompt() {
        let response = make_assistant_message("Hello, world!");
        let model = MockLanguageModel::new().with_response(response);
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let request = RuntimeRequest::new("Say hello");
        let stream = adapter.execute(request).expect("execute should succeed");

        let events: Vec<RuntimeEvent> = stream.collect().await;

        assert!(
            events.len() >= 2,
            "expected at least 2 events, got {}",
            events.len()
        );

        assert!(
            matches!(&events[0], RuntimeEvent::SessionStarted { .. }),
            "first event should be SessionStarted, got {:?}",
            &events[0]
        );

        let last = events.last().unwrap();
        assert!(
            matches!(last, RuntimeEvent::Completed { .. }),
            "last event should be Completed, got {:?}",
            last
        );

        let has_message = events.iter().any(|e| matches!(e, RuntimeEvent::Message { .. }));
        assert!(has_message, "expected at least one Message event");
    }

    #[tokio::test]
    async fn test_cancel_unknown_session() {
        let model = MockLanguageModel::new();
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let result = adapter.cancel("nonexistent-session").await;
        assert!(matches!(result, Err(RuntimeError::SessionNotFound(_))));
    }

    #[tokio::test]
    async fn test_capabilities() {
        let model = MockLanguageModel::new();
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let caps = adapter.capabilities();
        assert!(caps.streaming);
        assert!(caps.tool_control);
        assert!(!caps.permission_callback);
        assert!(!caps.resume);
        assert!(caps.cancel);
    }

    #[tokio::test]
    async fn test_respond_permission_not_implemented() {
        let model = MockLanguageModel::new();
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let result = adapter
            .respond_permission(
                "no-session",
                "req-1",
                PermissionDecision::Allow {
                    updated_input: None,
                },
            )
            .await;
        assert!(matches!(result, Err(RuntimeError::SessionNotFound(_))));
    }
}
