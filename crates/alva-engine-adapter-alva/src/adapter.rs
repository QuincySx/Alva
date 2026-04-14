// INPUT:  AlvaAdapterConfig, EventMapper, alva_kernel_core, alva_engine_runtime::*
// OUTPUT: AlvaAdapter — EngineRuntime implementation that wraps agent engine
// POS:    Core adapter bridging the agent engine to the unified EngineRuntime interface.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use std::sync::Mutex;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::debug;

use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::run::run_agent;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::AgentMessage;
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use alva_kernel_abi::agent_session::InMemoryAgentSession;
use alva_kernel_abi::{CancellationToken, ContentBlock, Message, MessageRole};

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

/// `EngineRuntime` adapter that wraps agent engine.
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
        if request.resume_session.is_some() {
            return Err(RuntimeError::Unsupported(
                "resume_session is not supported by AlvaAdapter".to_string(),
            ));
        }

        let max_iterations = request
            .options
            .max_turns
            .unwrap_or(self.config.max_iterations);

        // 1. Generate a unique session ID.
        let session_id = uuid::Uuid::new_v4().to_string();

        // 2. Determine the system prompt.
        let system_prompt = request
            .system_prompt
            .unwrap_or_else(|| self.config.system_prompt.clone());

        // 3. Build AgentState.
        let session: Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(InMemoryAgentSession::new());
        let state = AgentState {
            model: self.config.model.clone(),
            tools: self.config.tools.clone(),
            session,
            extensions: Extensions::new(),
        };

        // 4. Build AgentConfig.
        let config = AgentConfig {
            middleware: MiddlewareStack::new(),
            system_prompt,
            max_iterations,
            model_config: alva_kernel_abi::ModelConfig::default(),
            context_window: 0,
            workspace: request.working_directory,
            bus: self.config.bus.clone(),
            context_system: None,
            context_token_budget: None,
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

        // 9. Register the session handle synchronously to avoid a race where
        //    cancel() is called before the spawned task inserts the entry.
        {
            let mut map = self.sessions.lock().unwrap();
            map.insert(
                sid.clone(),
                SessionHandle {
                    cancel: cancel.clone(),
                },
            );
        }

        // 10. Spawn the background task.
        tokio::spawn(async move {
            debug!(session_id = %sid, "AlvaAdapter: starting agent");

            // Create a mapper to convert AgentEvents to RuntimeEvents.
            let (agent_tx, mut agent_rx) = mpsc::unbounded_channel();

            // Run the agent loop.
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
                let mut map = sessions.lock().unwrap();
                map.remove(&sid);
            }

            debug!(session_id = %sid, "AlvaAdapter: session completed");
        });

        // 11. Wrap the receiver as a Stream.
        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let map = self.sessions.lock().unwrap();
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
        let map = self.sessions.lock().unwrap();
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
    use alva_kernel_abi::{AgentError, Message, Tool, ToolExecutionContext, ToolOutput};
    use async_trait::async_trait;
    use serde_json::json;
    use std::path::Path;
    use tokio_stream::StreamExt;

    fn make_config(model: MockLanguageModel) -> AlvaAdapterConfig {
        AlvaAdapterConfig {
            model: Arc::new(model),
            tools: vec![],
            system_prompt: "You are a helpful assistant.".to_string(),
            max_iterations: 1,
            streaming: false,
            bus: None,
        }
    }

    struct WorkspaceEchoTool;

    #[async_trait]
    impl Tool for WorkspaceEchoTool {
        fn name(&self) -> &str {
            "workspace_echo"
        }

        fn description(&self) -> &str {
            "Echoes the current workspace path"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {},
            })
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
            ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            let workspace = ctx
                .workspace()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|| "<none>".to_string());
            Ok(ToolOutput::text(workspace))
        }
    }

    struct NoopTool;

    #[async_trait]
    impl Tool for NoopTool {
        fn name(&self) -> &str {
            "noop"
        }

        fn description(&self) -> &str {
            "Returns ok"
        }

        fn parameters_schema(&self) -> serde_json::Value {
            json!({
                "type": "object",
                "properties": {},
            })
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: &dyn ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
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

        let has_message = events
            .iter()
            .any(|e| matches!(e, RuntimeEvent::Message { .. }));
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

    #[tokio::test]
    async fn test_execute_propagates_working_directory_to_tool_context() {
        let first = Message {
            id: "tool-call".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".into(),
                name: "workspace_echo".into(),
                input: json!({}),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let second = make_assistant_message("done");
        let model = MockLanguageModel::new()
            .with_response(first)
            .with_response(second);
        let mut config = make_config(model);
        config.tools = vec![Arc::new(WorkspaceEchoTool)];
        config.max_iterations = 4;
        let adapter = AlvaAdapter::new(config);

        let workspace = Path::new("/tmp/alva-adapter-workspace");
        let request = RuntimeRequest::new("inspect workspace").with_cwd(workspace);
        let stream = adapter.execute(request).expect("execute should succeed");
        let events: Vec<RuntimeEvent> = stream.collect().await;

        let tool_result_text = events.iter().find_map(|event| match event {
            RuntimeEvent::ToolEnd { result, .. } => Some(result.model_text()),
            _ => None,
        });

        assert_eq!(
            tool_result_text.as_deref(),
            Some("/tmp/alva-adapter-workspace")
        );
    }

    #[tokio::test]
    async fn test_execute_honors_request_max_turns() {
        let first = Message {
            id: "tool-call".into(),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolUse {
                id: "tool-1".into(),
                name: "noop".into(),
                input: json!({}),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        let second = make_assistant_message("this should never be reached");
        let model = MockLanguageModel::new()
            .with_response(first)
            .with_response(second);
        let mut config = make_config(model);
        config.tools = vec![Arc::new(NoopTool)];
        config.max_iterations = 5;
        let adapter = AlvaAdapter::new(config);

        let mut request = RuntimeRequest::new("loop once");
        request.options.max_turns = Some(1);
        let stream = adapter.execute(request).expect("execute should succeed");
        let events: Vec<RuntimeEvent> = stream.collect().await;

        assert!(events.iter().any(|event| matches!(
            event,
            RuntimeEvent::Error { message, .. } if message.contains("Max iterations reached: 1")
        )));
    }

    #[tokio::test]
    async fn test_execute_rejects_resume_session_for_unsupported_adapter() {
        let response = make_assistant_message("Hello, world!");
        let model = MockLanguageModel::new().with_response(response);
        let config = make_config(model);
        let adapter = AlvaAdapter::new(config);

        let mut request = RuntimeRequest::new("Say hello");
        request.resume_session = Some("existing-session".into());

        let result = adapter.execute(request);
        assert!(matches!(result, Err(RuntimeError::Unsupported(_))));
    }
}
