// INPUT:  BaseAgentFactory, EventMapper, HistoryStore, alva_app_core::BaseAgent, alva_engine_runtime::*
// OUTPUT: AlvaAdapter implementing EngineRuntime
// POS:    Runs one independently assembled BaseAgent per engine session and exposes it through the neutral engine contract.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use alva_app_core::{ApprovalRequest, BaseAgent, PermissionDecision as AlvaPermissionDecision};
use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};
use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::warn;

use crate::factory::BaseAgentFactory;
use crate::history::HistoryStore;
use crate::mapping::EventMapper;

struct RuntimeSession {
    agent: Option<Arc<BaseAgent>>,
    approval_rx: Option<mpsc::UnboundedReceiver<ApprovalRequest>>,
    running: bool,
}

/// Canonical Hub-facing Alva engine. Each engine session owns one independent
/// BaseAgent, preserving app-core plugins, middleware, security and context.
///
/// When [`HistoryStore`] is configured, message transcripts are written after
/// each turn so a later process can `resume_session` by rebuilding the agent
/// and calling [`BaseAgent::restore_messages`].
pub struct AlvaAdapter {
    factory: Arc<dyn BaseAgentFactory>,
    sessions: Arc<Mutex<HashMap<String, RuntimeSession>>>,
    history: Option<HistoryStore>,
}

impl AlvaAdapter {
    pub fn new(factory: Arc<dyn BaseAgentFactory>) -> Self {
        Self {
            factory,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            history: None,
        }
    }

    /// Persist/restore message history under `dir` for cross-process resume.
    pub fn with_history_dir(mut self, dir: impl Into<PathBuf>) -> Self {
        self.history = Some(HistoryStore::new(dir));
        self
    }

    pub fn with_history_store(mut self, store: HistoryStore) -> Self {
        self.history = Some(store);
        self
    }
}

#[async_trait]
impl EngineRuntime for AlvaAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        let resume_id = request.resume_session.clone();
        let session_id = resume_id
            .clone()
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let resumed_session = if resume_id.is_some() {
            let mut sessions = self
                .sessions
                .lock()
                .expect("Alva adapter session lock poisoned");
            if let Some(session) = sessions.get_mut(&session_id) {
                if session.running {
                    return Err(RuntimeError::NotReady(format!(
                        "session '{session_id}' is already running"
                    )));
                }
                let agent = session
                    .agent
                    .clone()
                    .ok_or_else(|| RuntimeError::NotReady(session_id.clone()))?;
                let approval_rx = session.approval_rx.take();
                session.running = true;
                Some((agent, approval_rx))
            } else {
                // Not in this process — may rehydrate from history below.
                sessions.insert(
                    session_id.clone(),
                    RuntimeSession {
                        agent: None,
                        approval_rx: None,
                        running: true,
                    },
                );
                None
            }
        } else {
            self.sessions
                .lock()
                .expect("Alva adapter session lock poisoned")
                .insert(
                    session_id.clone(),
                    RuntimeSession {
                        agent: None,
                        approval_rx: None,
                        running: true,
                    },
                );
            None
        };

        let factory = self.factory.clone();
        let sessions = self.sessions.clone();
        let history = self.history.clone();
        tokio::spawn(async move {
            let (agent, mut approval_rx) = match resumed_session {
                Some(session) => session,
                None => {
                    // Cross-process resume requires durable history.
                    if resume_id.is_some() {
                        let Some(ref store) = history else {
                            let _ = event_tx.send(RuntimeEvent::Error {
                                message: format!(
                                    "Alva session '{session_id}' is not in this process and no history store is configured"
                                ),
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
                        };
                        match store.load(&session_id) {
                            Ok(Some(_)) => {}
                            Ok(None) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: format!(
                                        "Alva session '{session_id}' not found in memory or history store"
                                    ),
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
                            Err(e) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: format!("Alva history load failed: {e}"),
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
                        }
                    }

                    match factory.build(&request).await {
                        Ok(built) => {
                            let agent = Arc::new(built.agent);
                            if resume_id.is_some() {
                                if let Some(ref store) = history {
                                    match store.load(&session_id) {
                                        Ok(Some(msgs)) => {
                                            agent.restore_messages(msgs).await;
                                        }
                                        Ok(None) => {}
                                        Err(e) => {
                                            warn!(%e, "alva history load after build failed");
                                        }
                                    }
                                }
                            }
                            (agent, built.approval_rx)
                        }
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
                    }
                }
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
            loop {
                tokio::select! {
                    biased;
                    approval = receive_approval(&mut approval_rx) => {
                        match approval {
                            Some(req) => {
                                let request_id = req.request_id.clone();
                                if output_open
                                    && event_tx
                                        .send(RuntimeEvent::PermissionRequest {
                                            request_id: req.request_id,
                                            tool_name: req.tool_name,
                                            tool_input: req.arguments,
                                            description: None,
                                        })
                                        .is_err()
                                {
                                    output_open = false;
                                    agent
                                        .resolve_permission(
                                            &request_id,
                                            "hub-consumer-dropped",
                                            AlvaPermissionDecision::RejectOnce,
                                        )
                                        .await;
                                    agent.cancel();
                                }
                            }
                            None => approval_rx = None,
                        }
                    }
                    event = events.recv() => {
                        let Some(event) = event else {
                            break;
                        };
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
                }
            }

            // Persist transcript for cross-process resume.
            if let Some(ref store) = history {
                let msgs = agent.messages().await;
                if let Err(e) = store.save(&session_id, &msgs) {
                    warn!(%e, session = %session_id, "failed to save Alva session history");
                }
            }

            if let Some(session) = sessions
                .lock()
                .expect("Alva adapter session lock poisoned")
                .get_mut(&session_id)
            {
                session.approval_rx = approval_rx;
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
        agent.resolve_permission(request_id, "hub", decision).await;
        Ok(())
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: true,
            resume: true,
            cancel: true,
        }
    }
}

async fn receive_approval(
    approval_rx: &mut Option<mpsc::UnboundedReceiver<ApprovalRequest>>,
) -> Option<ApprovalRequest> {
    match approval_rx {
        Some(rx) => rx.recv().await,
        None => std::future::pending().await,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use alva_app_core::EngineError;
    use alva_engine_runtime::{validate_event_stream, EngineRuntime, RuntimeEvent, RuntimeRequest};
    use alva_test::fixtures::make_assistant_message;
    use alva_test::mock_provider::MockLanguageModel;
    use async_trait::async_trait;
    use serde_json::json;
    use tokio::sync::mpsc;
    use tokio_stream::StreamExt;

    use super::*;
    use crate::{
        BaseAgentFactory, BuiltAgent, DefaultBaseAgentFactory, DefaultBaseAgentFactoryConfig,
    };

    struct ApprovalFactory {
        inner: DefaultBaseAgentFactory,
        approval_rx: Mutex<Option<mpsc::UnboundedReceiver<ApprovalRequest>>>,
    }

    #[async_trait]
    impl BaseAgentFactory for ApprovalFactory {
        async fn build(&self, request: &RuntimeRequest) -> Result<BuiltAgent, EngineError> {
            let mut built = self.inner.build(request).await?;
            built.approval_rx = self
                .approval_rx
                .lock()
                .expect("approval receiver lock poisoned")
                .take();
            Ok(built)
        }
    }

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
            validate_event_stream(&second).is_ok(),
            "resume stream must satisfy multi-engine contract: {second:?}"
        );
    }

    #[tokio::test]
    async fn approval_receiver_moves_between_turns_without_holding_stream_open() {
        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("first"))
                .with_response(make_assistant_message("second")),
        );
        let config = DefaultBaseAgentFactoryConfig::new(model, std::env::temp_dir());
        let (approval_tx, approval_rx) = mpsc::unbounded_channel();
        let adapter = AlvaAdapter::new(Arc::new(ApprovalFactory {
            inner: DefaultBaseAgentFactory::new(config),
            approval_rx: Mutex::new(Some(approval_rx)),
        }));

        let first = tokio::time::timeout(
            Duration::from_secs(2),
            adapter
                .execute(RuntimeRequest::new("first prompt"))
                .unwrap()
                .collect::<Vec<_>>(),
        )
        .await
        .expect("approval receiver must not keep the completed turn stream open");
        let session_id = match first.first() {
            Some(RuntimeEvent::SessionStarted { session_id, .. }) => session_id.clone(),
            other => panic!("expected SessionStarted, got {other:?}"),
        };
        assert!(
            adapter
                .sessions
                .lock()
                .expect("session lock poisoned")
                .get(&session_id)
                .and_then(|session| session.approval_rx.as_ref())
                .is_some(),
            "idle session must retain its approval receiver for the next turn"
        );

        approval_tx
            .send(ApprovalRequest {
                request_id: "approval-2".into(),
                tool_name: "execute_shell".into(),
                arguments: json!({"command": "pwd"}),
            })
            .unwrap();
        let mut resumed = RuntimeRequest::new("second prompt");
        resumed.resume_session = Some(session_id);
        let second = tokio::time::timeout(
            Duration::from_secs(2),
            adapter.execute(resumed).unwrap().collect::<Vec<_>>(),
        )
        .await
        .expect("resumed turn stream must close");
        assert!(second.iter().any(|event| {
            matches!(
                event,
                RuntimeEvent::PermissionRequest { request_id, .. }
                    if request_id == "approval-2"
            )
        }));
        assert!(matches!(
            second.last(),
            Some(RuntimeEvent::Completed {
                result: Some(text),
                ..
            }) if text == "second"
        ));
    }

    #[tokio::test]
    async fn resumes_from_disk_history_after_process_drop() {
        let dir = std::env::temp_dir().join(format!(
            "alva-adapter-hist-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).unwrap();

        let model = Arc::new(
            MockLanguageModel::new()
                .with_response(make_assistant_message("first"))
                .with_response(make_assistant_message("second")),
        );
        let config = DefaultBaseAgentFactoryConfig::new(model.clone(), std::env::temp_dir());
        let adapter1 = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config.clone())))
            .with_history_dir(&dir);

        let first: Vec<_> = adapter1
            .execute(RuntimeRequest::new("first prompt"))
            .unwrap()
            .collect()
            .await;
        let session_id = match first.first() {
            Some(RuntimeEvent::SessionStarted { session_id, .. }) => session_id.clone(),
            other => panic!("expected SessionStarted, got {other:?}"),
        };
        drop(adapter1);

        // New adapter instance = "new process" (empty in-memory map).
        let adapter2 =
            AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config))).with_history_dir(&dir);
        let mut resumed = RuntimeRequest::new("second prompt");
        resumed.resume_session = Some(session_id);
        let second: Vec<_> = adapter2.execute(resumed).unwrap().collect().await;
        assert!(
            matches!(
                second.last(),
                Some(RuntimeEvent::Completed {
                    result: Some(text),
                    ..
                }) if text == "second"
            ),
            "disk resume should complete with second answer: {second:?}"
        );
        assert_eq!(model.calls().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }
}
