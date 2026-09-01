// INPUT:  BaseAgentFactory, EventMapper, SessionRepository, BaseAgent, engine contracts
// OUTPUT: AlvaAdapter implementing EngineRuntime + EngineInspector
// POS:    Runs one independently assembled BaseAgent per engine session and exposes it through the neutral engine contract.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use crate::factory::BaseAgentFactory;
use crate::mapping::EventMapper;
use crate::session_repository::{RepositoryInspector, SessionRepository};
use alva_app_core::{ApprovalRequest, BaseAgent, PermissionDecision as AlvaPermissionDecision};
use alva_engine_runtime::{
    ChildSessionLink, EngineInspector, EngineRuntime, InspectionCapabilities, InspectionSnapshot,
    InspectionStream, InspectionUpdate, PermissionDecision, RuntimeCapabilities, RuntimeError,
    RuntimeEvent, RuntimeRequest,
};
use alva_kernel_abi::{ExecutionRecord, SessionRecordPayload};
use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::mpsc;
use tokio_stream::wrappers::UnboundedReceiverStream;
use tokio_stream::StreamExt as _;

struct RuntimeSession {
    agent: Option<Arc<BaseAgent>>,
    approval_rx: Option<mpsc::UnboundedReceiver<ApprovalRequest>>,
    running: bool,
}

/// Canonical Hub-facing Alva engine. Each engine session owns one independent
/// BaseAgent, preserving app-core plugins, middleware, security and context.
///
/// When [`SessionRepository`] is configured, the AgentSession record stream is
/// authoritative for both cross-process resume and Inspector.
pub struct AlvaAdapter {
    factory: Arc<dyn BaseAgentFactory>,
    sessions: Arc<Mutex<HashMap<String, RuntimeSession>>>,
    repository: Option<SessionRepository>,
}

impl AlvaAdapter {
    pub fn new(factory: Arc<dyn BaseAgentFactory>) -> Self {
        Self {
            factory,
            sessions: Arc::new(Mutex::new(HashMap::new())),
            repository: None,
        }
    }

    pub fn with_session_repository(mut self, repository: SessionRepository) -> Self {
        self.repository = Some(repository);
        self
    }

    async fn inspection_session(
        &self,
        native_session_id: &str,
    ) -> Result<Arc<dyn alva_kernel_abi::AgentSession>, RuntimeError> {
        let active = self
            .sessions
            .lock()
            .expect("Alva adapter session lock poisoned")
            .get(native_session_id)
            .and_then(|session| session.agent.clone());
        if let Some(agent) = active {
            return Ok(agent.session_handle().await);
        }
        let repository = self
            .repository
            .as_ref()
            .ok_or_else(|| RuntimeError::SessionNotFound(native_session_id.to_string()))?;
        if !repository
            .contains(native_session_id)
            .map_err(|error| RuntimeError::Other(error.to_string()))?
        {
            return Err(RuntimeError::SessionNotFound(native_session_id.to_string()));
        }
        repository
            .open_session(native_session_id.to_string(), None)
            .await
            .map(|session| session as Arc<dyn alva_kernel_abi::AgentSession>)
            .map_err(|error| RuntimeError::Other(error.to_string()))
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
                // Not in this process — may rehydrate from native records below.
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
        let repository = self.repository.clone();
        tokio::spawn(async move {
            let (agent, mut approval_rx) = match resumed_session {
                Some(session) => session,
                None => {
                    // Cross-process resume requires the native record store.
                    if resume_id.is_some() {
                        let Some(ref repository) = repository else {
                            let _ = event_tx.send(RuntimeEvent::Error {
                                message: format!(
                                    "Alva session '{session_id}' is not in this process and no native record store is configured"
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
                        match repository.contains(&session_id) {
                            Ok(true) => {}
                            Ok(false) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: format!(
                                        "Alva session '{session_id}' not found in memory or native record store"
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
                            Err(error) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: format!("Alva record lookup failed: {error}"),
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

                    let durable_session: Arc<dyn alva_kernel_abi::AgentSession> =
                        if let Some(repository) = &repository {
                            match repository.open_session(session_id.clone(), None).await {
                                Ok(session) => session,
                                Err(error) => {
                                    let _ = event_tx.send(RuntimeEvent::Error {
                                        message: format!("Alva record store failed: {error}"),
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
                        } else {
                            Arc::new(
                                alva_kernel_core::agent_session::ListenableInMemorySession::new(),
                            )
                        };

                    match factory.build(&request, durable_session).await {
                        Ok(built) => {
                            let agent = Arc::new(built.agent);
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

            let _ = agent.session_handle().await.flush().await;

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

#[async_trait]
impl EngineInspector for AlvaAdapter {
    fn inspection_capabilities(&self) -> InspectionCapabilities {
        InspectionCapabilities::full()
    }

    async fn snapshot(&self, native_session_id: &str) -> Result<InspectionSnapshot, RuntimeError> {
        if let Some(repository) = &self.repository {
            return RepositoryInspector::new(repository.clone())
                .snapshot(native_session_id)
                .await;
        }
        let session = self.inspection_session(native_session_id).await?;
        let parent_session_id = session.parent_session_id().map(str::to_string);
        let records = session.records(0).await;
        let high_water_seq = records.last().map(|record| record.seq).unwrap_or(0);
        let children = records
            .iter()
            .filter_map(|record| match &record.payload {
                SessionRecordPayload::Execution(ExecutionRecord::ChildSessionLinked {
                    child_session_id,
                    parent_run_id,
                    spawn_tool_call_id,
                }) => Some(ChildSessionLink {
                    child_session_id: child_session_id.clone(),
                    parent_session_id: native_session_id.to_string(),
                    parent_run_id: parent_run_id.clone(),
                    spawn_tool_call_id: spawn_tool_call_id.clone(),
                }),
                _ => None,
            })
            .collect();
        Ok(InspectionSnapshot {
            engine_id: "alva".to_string(),
            native_session_id: native_session_id.to_string(),
            parent_session_id,
            high_water_seq,
            records,
            children,
        })
    }

    async fn subscribe(
        &self,
        native_session_id: &str,
        after_seq: u64,
    ) -> Result<InspectionStream, RuntimeError> {
        if let Some(repository) = &self.repository {
            return RepositoryInspector::new(repository.clone())
                .subscribe(native_session_id, after_seq)
                .await;
        }
        let session = self.inspection_session(native_session_id).await?;
        let stream =
            session
                .subscribe_events(after_seq)
                .await
                .map(|event| InspectionUpdate::Record {
                    record: Box::new(alva_kernel_abi::SessionRecord::from_event(event)),
                });
        Ok(Box::pin(stream))
    }

    async fn delete_session(&self, native_session_id: &str) -> Result<(), RuntimeError> {
        let session = self
            .sessions
            .lock()
            .expect("Alva adapter session lock poisoned")
            .remove(native_session_id);
        if let Some(repository) = &self.repository {
            repository
                .delete(native_session_id)
                .map_err(|error| RuntimeError::Other(error.to_string()))?;
        } else if let Some(agent) = session.and_then(|session| session.agent) {
            agent
                .session_handle()
                .await
                .clear()
                .await
                .map_err(|error| RuntimeError::Other(error.to_string()))?;
        }
        Ok(())
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
    use alva_kernel_abi::AgentSession as _;
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
        async fn build(
            &self,
            request: &RuntimeRequest,
            session: Arc<dyn alva_kernel_abi::AgentSession>,
        ) -> Result<BuiltAgent, EngineError> {
            let mut built = self.inner.build(request, session).await?;
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
    async fn exposes_full_typed_inspection_snapshot_from_authoritative_session() {
        let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("hub")));
        let config = DefaultBaseAgentFactoryConfig::new(model, std::env::temp_dir());
        let adapter = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config)));
        let events: Vec<_> = adapter
            .execute(RuntimeRequest::new("inspect me"))
            .unwrap()
            .collect()
            .await;
        let session_id = match &events[0] {
            RuntimeEvent::SessionStarted { session_id, .. } => session_id,
            event => panic!("expected session start, got {event:?}"),
        };

        assert_eq!(
            adapter.inspection_capabilities(),
            InspectionCapabilities::full()
        );
        let snapshot = adapter.snapshot(session_id).await.unwrap();
        assert_eq!(snapshot.native_session_id, *session_id);
        assert!(snapshot.high_water_seq > 0);
        assert!(snapshot
            .records
            .windows(2)
            .all(|pair| pair[0].seq < pair[1].seq));
        let type_names: std::collections::HashSet<_> = snapshot
            .records
            .iter()
            .map(|record| record.type_name.as_str())
            .collect();
        assert!(type_names.contains("llm_first_output"));
        assert!(type_names.contains("llm_stream_summary"));
        assert!(type_names.contains("middleware_hook_batch"));
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
    async fn resumes_from_native_records_after_process_drop() {
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
        let repository_path = dir.join("records.db");
        let adapter1 = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config.clone())))
            .with_session_repository(SessionRepository::open(&repository_path).unwrap());

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
        let adapter2 = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config)))
            .with_session_repository(SessionRepository::open(&repository_path).unwrap());
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
            "native record resume should complete with second answer: {second:?}"
        );
        assert_eq!(model.calls().len(), 2);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[tokio::test]
    async fn deleting_an_active_repository_session_removes_recursive_children() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let model =
            Arc::new(MockLanguageModel::new().with_response(make_assistant_message("done")));
        let config = DefaultBaseAgentFactoryConfig::new(model, directory.path());
        let adapter = AlvaAdapter::new(Arc::new(DefaultBaseAgentFactory::new(config)))
            .with_session_repository(repository.clone());

        let events: Vec<_> = adapter
            .execute(RuntimeRequest::new("parent"))
            .unwrap()
            .collect()
            .await;
        let parent_id = match events.first() {
            Some(RuntimeEvent::SessionStarted { session_id, .. }) => session_id.clone(),
            other => panic!("expected SessionStarted, got {other:?}"),
        };
        let parent = repository
            .open_session(parent_id.clone(), None)
            .await
            .unwrap();
        let child = parent
            .create_child_session("run-1", "spawn-1")
            .await
            .unwrap()
            .unwrap();
        let child_id = child.session_id().to_string();

        adapter.delete_session(&parent_id).await.unwrap();

        assert!(!repository.contains(&parent_id).unwrap());
        assert!(!repository.contains(&child_id).unwrap());
    }
}
