// INPUT:  BaseAgentFactory, EventMapper, alva_app_core::BaseAgent, alva_engine_runtime::*
// OUTPUT: AlvaAdapter implementing EngineRuntime
// POS:    Runs one independently assembled BaseAgent per engine session and exposes it through the neutral engine contract.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use alva_app_core::{BaseAgent, PermissionDecision as AlvaPermissionDecision};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;

use crate::factory::BaseAgentFactory;
use crate::mapping::EventMapper;

struct RuntimeSession {
    agent: Option<Arc<BaseAgent>>,
    running: bool,
}

/// Canonical Hub-facing Alva engine. Each engine session owns one independent
/// BaseAgent, preserving app-core plugins, middleware, security and context.
pub struct AlvaAdapter {
    factory: Arc<dyn BaseAgentFactory>,
    sessions: Arc<Mutex<HashMap<String, RuntimeSession>>>,
}

impl AlvaAdapter {
    pub fn new(factory: Arc<dyn BaseAgentFactory>) -> Self {
        Self {
            factory,
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
        let session_id = request
            .resume_session
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let resumed_agent = if request.resume_session.is_some() {
            let mut sessions = self
                .sessions
                .lock()
                .expect("Alva adapter session lock poisoned");
            let session = sessions
                .get_mut(&session_id)
                .ok_or_else(|| RuntimeError::SessionNotFound(session_id.clone()))?;
            if session.running {
                return Err(RuntimeError::NotReady(format!(
                    "session '{session_id}' is already running"
                )));
            }
            let agent = session
                .agent
                .clone()
                .ok_or_else(|| RuntimeError::NotReady(session_id.clone()))?;
            session.running = true;
            Some(agent)
        } else {
            self.sessions
                .lock()
                .expect("Alva adapter session lock poisoned")
                .insert(
                    session_id.clone(),
                    RuntimeSession {
                        agent: None,
                        running: true,
                    },
                );
            None
        };

        let factory = self.factory.clone();
        let sessions = self.sessions.clone();
        tokio::spawn(async move {
            let agent = match resumed_agent {
                Some(agent) => agent,
                None => match factory.build(&request).await {
                    Ok(agent) => Arc::new(agent),
                    Err(error) => {
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: error.to_string(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: session_id.clone(),
                            result: None,
                            usage: None,
                        });
                        sessions
                            .lock()
                            .expect("Alva adapter session lock poisoned")
                            .remove(&session_id);
                        return;
                    }
                },
            };

            {
                let mut sessions = sessions.lock().expect("Alva adapter session lock poisoned");
                let Some(session) = sessions.get_mut(&session_id) else {
                    return;
                };
                session.agent = Some(agent.clone());
            }

            let model = agent.model_id().await;
            let tools = agent.tool_names();
            let mut events = agent.prompt_text(&request.prompt).await;
            let mut mapper = EventMapper::new(session_id.clone(), model, tools);
            let mut output_open = true;
            while let Some(event) = events.recv().await {
                let mapped = mapper.map(event);
                let terminal = mapped
                    .iter()
                    .any(|event| matches!(event, RuntimeEvent::Completed { .. }));
                for event in mapped {
                    if output_open && event_tx.send(event).is_err() {
                        output_open = false;
                        agent.cancel();
                    }
                }
                if terminal {
                    break;
                }
            }

            if let Some(session) = sessions
                .lock()
                .expect("Alva adapter session lock poisoned")
                .get_mut(&session_id)
            {
                session.running = false;
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let agent = self
            .sessions
            .lock()
            .expect("Alva adapter session lock poisoned")
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?
            .agent
            .clone()
            .ok_or_else(|| RuntimeError::NotReady(session_id.into()))?;
        agent.cancel();
        Ok(())
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let agent = self
            .sessions
            .lock()
            .expect("Alva adapter session lock poisoned")
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?
            .agent
            .clone()
            .ok_or_else(|| RuntimeError::NotReady(session_id.into()))?;
        let decision = match decision {
            PermissionDecision::Allow {
                updated_input: None,
            } => AlvaPermissionDecision::AllowOnce,
            PermissionDecision::Deny { .. } => AlvaPermissionDecision::RejectOnce,
            PermissionDecision::Allow {
                updated_input: Some(_),
            } => {
                return Err(RuntimeError::Unsupported(
                    "Alva permissions cannot modify tool input".into(),
                ));
            }
        };
        // RuntimeEvent's permission response is keyed by request ID only;
        // SecurityGuard uses that ID as the authority and accepts this label
        // for diagnostics.
        agent.resolve_permission(request_id, "hub", decision).await;
        Ok(())
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: false,
            resume: true,
            cancel: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alva_engine_runtime::{
        validate_event_stream, EngineRuntime, RuntimeEvent, RuntimeRequest,
    };
    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use tokio_stream::StreamExt;

    use super::*;
    use crate::{DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig};

    #[tokio::test]
    async fn runs_a_complete_base_agent_session() {
        let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("hub")));
        let config = DefaultBaseAgentFactoryConfig::new(model, std::env::temp_dir());
        let adapter = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config)));

        let events: Vec<_> = adapter
            .execute(RuntimeRequest::new("say hub"))
            .unwrap()
            .collect()
            .await;

        assert!(matches!(
            events.first(),
            Some(RuntimeEvent::SessionStarted { .. })
        ));
        assert!(events
            .iter()
            .any(|event| matches!(event, RuntimeEvent::Message { .. })));
        assert!(matches!(
            events.last(),
            Some(RuntimeEvent::Completed { result: Some(text), .. }) if text == "hub"
        ));
        assert!(
            validate_event_stream(&events).is_ok(),
            "Alva adapter stream must satisfy multi-engine contract: {events:?}"
        );
    }

    #[tokio::test]
    async fn resumes_the_same_base_agent_session() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("first"))
                .with_response(make_assistant_message("second")),
        );
        let config = DefaultBaseAgentFactoryConfig::new(model.clone(), std::env::temp_dir());
        let adapter = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config)));

        let first: Vec<_> = adapter
            .execute(RuntimeRequest::new("first prompt"))
            .unwrap()
            .collect()
            .await;
        let session_id = match first.first() {
            Some(RuntimeEvent::SessionStarted { session_id, .. }) => session_id.clone(),
            other => panic!("expected SessionStarted, got {other:?}"),
        };

        let mut resumed = RuntimeRequest::new("second prompt");
        resumed.resume_session = Some(session_id);
        let second: Vec<_> = adapter.execute(resumed).unwrap().collect().await;
        assert!(matches!(
            second.first(),
            Some(RuntimeEvent::SessionStarted { .. })
        ));
        assert!(matches!(
            second.last(),
            Some(RuntimeEvent::Completed { .. })
        ));
        assert_eq!(model.calls().len(), 2);
        assert!(
            model.calls()[1].len() >= 3,
            "resumed turn retains first history"
        );
    }
}
