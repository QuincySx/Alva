// INPUT:  AlvaAdapterConfig, EventMapper, alva_agent_core::{Agent, AgentHooks, AgentMessage}, alva_engine_runtime::*
// OUTPUT: AlvaAdapter — EngineRuntime implementation that wraps alva-agent-core Agent
// POS:    Core adapter bridging the local agent engine to the unified EngineRuntime interface.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::debug;

use alva_agent_core::{Agent, AgentHooks, AgentMessage};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use alva_types::{ContentBlock, Message, MessageRole};

use crate::config::AlvaAdapterConfig;
use crate::mapping::EventMapper;

// ---------------------------------------------------------------------------
// Session handle — kept alive while the agent loop is running
// ---------------------------------------------------------------------------

struct SessionHandle {
    agent: Arc<Agent>,
}

// ---------------------------------------------------------------------------
// AlvaAdapter
// ---------------------------------------------------------------------------

/// `EngineRuntime` adapter that wraps `alva-agent-core::Agent`.
///
/// Each `execute()` call creates a fresh `Agent`, stores a handle in the
/// session map (keyed by a generated UUID), and returns a `RuntimeEvent`
/// stream. The handle is removed when the stream completes.
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
            .unwrap_or_else(|| "You are a helpful assistant.".to_string());

        // 3. Build AgentHooks from config.
        let mut hooks = AgentHooks::new(self.config.convert_to_llm.clone());
        hooks.tool_execution = self.config.tool_execution;
        if self.config.max_iterations > 0 {
            hooks.max_iterations = self.config.max_iterations;
        }
        // Override with request-level max_turns if provided.
        if let Some(max_turns) = request.options.max_turns {
            hooks.max_iterations = max_turns;
        }

        // 4. Create Agent synchronously.
        let agent = Arc::new(Agent::new(
            self.config.model.clone(),
            system_prompt,
            hooks,
        ));

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

        // 7. Clone what the spawned task needs.
        let sessions = self.sessions.clone();
        let tools = self.config.tools.clone();
        let streaming = request.options.streaming || self.config.streaming;
        let sid = session_id.clone();
        let agent_handle = agent.clone();

        // 8. Spawn the background task.
        tokio::spawn(async move {
            // Register the session.
            {
                let mut map = sessions.lock().await;
                map.insert(
                    sid.clone(),
                    SessionHandle {
                        agent: agent_handle.clone(),
                    },
                );
            }

            // Async setup: set tools and streaming mode.
            agent_handle.set_tools(tools).await;
            agent_handle.set_streaming(streaming).await;

            debug!(session_id = %sid, "AlvaAdapter: starting agent prompt");

            // Start the agent loop — returns an unbounded receiver of AgentEvents.
            let mut agent_rx = agent_handle.prompt(vec![user_message]);

            // Map AgentEvents to RuntimeEvents.
            let mut mapper = EventMapper::new(sid.clone());

            while let Some(agent_event) = agent_rx.recv().await {
                let runtime_events = mapper.map(agent_event);
                for re in runtime_events {
                    if event_tx.send(re).is_err() {
                        // Consumer dropped; stop processing.
                        debug!(session_id = %sid, "AlvaAdapter: consumer dropped, stopping");
                        break;
                    }
                }
            }

            // Clean up the session.
            {
                let mut map = sessions.lock().await;
                map.remove(&sid);
            }

            debug!(session_id = %sid, "AlvaAdapter: session completed");
        });

        // 9. Wrap the receiver as a Stream.
        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let map = self.sessions.lock().await;
        match map.get(session_id) {
            Some(handle) => {
                handle.agent.cancel();
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
        // v1: permission callback is not implemented.
        // Return SessionNotFound if the session doesn't exist,
        // PermissionNotFound otherwise (no actual permission flow yet).
        let map = self.sessions.lock().await;
        if map.contains_key(session_id) {
            Err(RuntimeError::PermissionNotFound(
                "permission callback not implemented in v1".to_string(),
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
    use alva_types::EmptyToolContext;
    use tokio_stream::StreamExt;

    /// Minimal convert_to_llm that just extracts Standard messages.
    fn test_convert_to_llm() -> alva_agent_core::ConvertToLlmFn {
        Arc::new(|ctx| {
            ctx.messages
                .iter()
                .filter_map(|m| match m {
                    AgentMessage::Standard(msg) => Some(msg.clone()),
                    _ => None,
                })
                .collect()
        })
    }

    fn make_config(model: MockLanguageModel) -> AlvaAdapterConfig {
        AlvaAdapterConfig {
            model: Arc::new(model),
            convert_to_llm: test_convert_to_llm(),
            tools: vec![],
            tool_context: Arc::new(EmptyToolContext),
            tool_execution: alva_agent_core::ToolExecutionMode::Parallel,
            max_iterations: 1,
            streaming: false,
        }
    }

    #[tokio::test]
    async fn test_execute_simple_prompt() {
        // Set up a mock model that returns one assistant message.
        let response = make_assistant_message("Hello, world!");
        let model = MockLanguageModel::new().with_response(response);
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        // Execute a simple prompt.
        let request = RuntimeRequest::new("Say hello");
        let stream = adapter.execute(request).expect("execute should succeed");

        // Collect all events.
        let events: Vec<RuntimeEvent> = stream.collect().await;

        // Must have at least: SessionStarted, Message, Completed
        assert!(
            events.len() >= 2,
            "expected at least 2 events, got {}",
            events.len()
        );

        // First event must be SessionStarted.
        assert!(
            matches!(&events[0], RuntimeEvent::SessionStarted { .. }),
            "first event should be SessionStarted, got {:?}",
            &events[0]
        );

        // Last event must be Completed.
        let last = events.last().unwrap();
        assert!(
            matches!(last, RuntimeEvent::Completed { .. }),
            "last event should be Completed, got {:?}",
            last
        );

        // Should have at least one Message event.
        let has_message = events.iter().any(|e| matches!(e, RuntimeEvent::Message { .. }));
        assert!(has_message, "expected at least one Message event");
    }

    #[tokio::test]
    async fn test_cancel_stops_execution() {
        // The mock model responds immediately, so there is an inherent race
        // between the spawned agent task completing and our cancel() call.
        // This test validates two things:
        //   1. cancel() succeeds if the session is still running, OR
        //   2. the stream reaches Completed even if the session finishes
        //      before we can cancel.
        let response = make_assistant_message("Working...");
        let model = MockLanguageModel::new().with_response(response);
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let request = RuntimeRequest::new("Do something");
        let mut stream = adapter.execute(request).expect("execute should succeed");

        use tokio_stream::StreamExt as TokioStreamExt;

        // Consume the first event to get the session_id.
        let first_event = TokioStreamExt::next(&mut stream).await;
        let session_id = match first_event {
            Some(RuntimeEvent::SessionStarted { session_id, .. }) => session_id,
            other => panic!("expected SessionStarted, got {:?}", other),
        };

        // Try to cancel — may succeed or may get SessionNotFound if the
        // agent already finished (race condition with fast mock).
        let cancel_result = adapter.cancel(&session_id).await;
        match &cancel_result {
            Ok(()) => { /* great, we cancelled in time */ }
            Err(RuntimeError::SessionNotFound(_)) => {
                // Agent completed before cancel — this is acceptable.
            }
            Err(e) => panic!("unexpected cancel error: {:?}", e),
        }

        // Drain remaining events — must eventually get Completed.
        let mut got_completed = false;
        while let Some(event) = TokioStreamExt::next(&mut stream).await {
            if matches!(event, RuntimeEvent::Completed { .. }) {
                got_completed = true;
                break;
            }
        }
        assert!(got_completed, "expected Completed event after cancel");
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

        // No session exists — should get SessionNotFound.
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
