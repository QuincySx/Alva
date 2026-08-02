// INPUT:  alva_engine_runtime, alva_hub_core, HubStore, serde, tokio_stream
// OUTPUT: multi-engine run seams, delta/full handoff injection, Attempt capture, timeline DTOs
// POS:    Highest product seam for durable Hub Desktop branch execution and native-resume recovery.

use std::path::PathBuf;
use std::sync::Arc;

use alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest, RuntimeUsage};
use alva_hub_core::{HubRequest, HubRuntime, HubSessionStatus, ProductSessionId};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

use crate::store::{EngineTarget, HubStore, PreparedTurn, SideEffectEvidence, StoredUsage};

fn default_engine_id() -> String {
    "alva".to_string()
}

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RunRequest {
    pub prompt: String,
    pub workspace: PathBuf,
    #[serde(default = "default_engine_id")]
    pub engine_id: String,
    #[serde(default)]
    pub product_session_id: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineStatus {
    Running,
    Completed,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TimelineEvent {
    pub status: TimelineStatus,
    pub engine_id: String,
    pub product_session_id: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct RunOutcome {
    pub product_session_id: String,
    pub result: Option<String>,
    pub failed: bool,
}

struct ExecutedTurn {
    outcome: RunOutcome,
    engine_session_id: Option<String>,
    terminal_error: Option<String>,
    usage: Option<StoredUsage>,
}

pub async fn run_once<F>(
    engine: Arc<dyn EngineRuntime>,
    request: RunRequest,
    mut emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    validate_prompt(&request.prompt)?;
    let workspace = canonical_workspace(&request.workspace)?;
    let target = EngineTarget::for_engine(&request.engine_id)?;
    let hub = HubRuntime::new();
    hub.register_engine(target.engine_id.as_str(), engine)
        .map_err(|error| error.to_string())?;
    let product_session_id = ProductSessionId::new();
    Ok(execute_turn(
        &hub,
        product_session_id,
        request.prompt,
        workspace,
        &target.engine_id,
        None,
        &mut emit,
    )
    .await
    .outcome)
}

pub async fn run_persisted<F>(
    store: &HubStore,
    engine: Arc<dyn EngineRuntime>,
    request: RunRequest,
    mut emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    validate_prompt(&request.prompt)?;

    let prepared = if let Some(product_session_id) = request.product_session_id.as_deref() {
        let detail = store
            .get_session(product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        canonical_workspace(PathBuf::from(&detail.session.workspace).as_path())?;
        if request.engine_id != detail.session.engine_id {
            return Err(format!(
                "Hub session '{product_session_id}' is active on '{}'; switch engines before running '{}'",
                detail.session.engine_id, request.engine_id
            ));
        }
        store.prepare_resumed_turn(product_session_id, &request.prompt)?
    } else {
        let workspace = canonical_workspace(&request.workspace)?;
        let product_session_id = ProductSessionId::new();
        let target = EngineTarget::for_engine(&request.engine_id)?;
        store.prepare_new_turn(
            product_session_id.as_str(),
            &workspace,
            &request.prompt,
            &target,
        )?
    };

    let mut prepared = prepared;
    let mut execution = execute_persisted_turn(
        store,
        Arc::clone(&engine),
        &prepared,
        &request.prompt,
        &mut emit,
    )
    .await?;
    if prepared.native_resume
        && execution
            .terminal_error
            .as_deref()
            .is_some_and(is_invalid_native_resume_error)
    {
        let failure_reason = execution
            .terminal_error
            .as_deref()
            .expect("native resume failure was checked above");
        prepared = store.upgrade_failed_native_resume(&prepared, failure_reason)?;
        execution =
            execute_persisted_turn(store, engine, &prepared, &request.prompt, &mut emit).await?;
    }
    store.finish_turn(
        &prepared,
        execution.engine_session_id.as_deref(),
        execution.outcome.result.as_deref(),
        execution.terminal_error.as_deref(),
        execution.usage.as_ref(),
    )?;
    store.apply_pending_switch(&prepared.product_session_id)?;
    Ok(execution.outcome)
}

async fn execute_persisted_turn<F>(
    store: &HubStore,
    engine: Arc<dyn EngineRuntime>,
    prepared: &PreparedTurn,
    user_prompt: &str,
    emit: &mut F,
) -> Result<ExecutedTurn, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    let hub = HubRuntime::new();
    hub.register_engine(prepared.target.engine_id.as_str(), engine)
        .map_err(|error| error.to_string())?;
    let product_session_id = ProductSessionId::from_existing(&prepared.product_session_id);
    if let Some(engine_session_id) = &prepared.engine_session_id {
        hub.restore_session(
            product_session_id.clone(),
            prepared.target.engine_id.as_str(),
            engine_session_id,
            HubSessionStatus::Completed,
        )
        .map_err(|error| error.to_string())?;
    }
    let prompt = prepared
        .handoff
        .as_ref()
        .map(|handoff| {
            format!(
                "{}\n\n<current-user-instruction>\n{}\n</current-user-instruction>",
                handoff.render_for_engine(),
                user_prompt
            )
        })
        .unwrap_or_else(|| user_prompt.to_string());
    Ok(execute_turn(
        &hub,
        product_session_id,
        prompt,
        canonical_workspace(&prepared.workspace)?,
        &prepared.target.engine_id,
        Some((store, prepared)),
        emit,
    )
    .await)
}

fn is_invalid_native_resume_error(error: &str) -> bool {
    let error = error.to_ascii_lowercase();
    error.contains("session not found")
        || error.contains("invalid session")
        || error.contains("unknown session")
        || (error.contains("resume") && error.contains("session"))
}

async fn execute_turn<F>(
    hub: &HubRuntime,
    product_session_id: ProductSessionId,
    prompt: String,
    workspace: PathBuf,
    engine_id: &str,
    persistence: Option<(&HubStore, &PreparedTurn)>,
    emit: &mut F,
) -> ExecutedTurn
where
    F: FnMut(TimelineEvent) + Send,
{
    let product_id = product_session_id.as_str().to_string();
    emit(TimelineEvent {
        status: TimelineStatus::Running,
        engine_id: engine_id.to_string(),
        product_session_id: product_id.clone(),
        message: None,
    });

    let runtime_request = RuntimeRequest::new(prompt)
        .with_cwd(workspace)
        .with_streaming();
    let hub_request = HubRequest::new(engine_id, runtime_request).resume(product_session_id);
    let mut stream = match hub.execute(hub_request) {
        Ok(stream) => stream,
        Err(error) => {
            let message = error.to_string();
            emit(TimelineEvent {
                status: TimelineStatus::Failed,
                engine_id: engine_id.to_string(),
                product_session_id: product_id.clone(),
                message: Some(message.clone()),
            });
            return ExecutedTurn {
                outcome: RunOutcome {
                    product_session_id: product_id,
                    result: None,
                    failed: true,
                },
                engine_session_id: None,
                terminal_error: Some(message),
                usage: None,
            };
        }
    };

    let mut terminal_error = None;
    let mut result = None;
    let mut engine_session_id = None;
    let mut usage = None;
    while let Some(hub_event) = stream.next().await {
        match hub_event.event {
            RuntimeEvent::SessionStarted { session_id, .. } => {
                if let Some((store, prepared)) = persistence {
                    capture_persistence_error(
                        store.record_engine_session(prepared, &session_id),
                        &mut terminal_error,
                        &product_id,
                        engine_id,
                        emit,
                    );
                }
                engine_session_id = Some(session_id);
            }
            RuntimeEvent::ToolStart { id, name, .. } => {
                if let Some((store, prepared)) = persistence {
                    capture_persistence_error(
                        store.record_side_effect(
                            prepared,
                            SideEffectEvidence {
                                tool_call_id: id,
                                tool_name: name,
                                succeeded: None,
                                duration_ms: None,
                            },
                        ),
                        &mut terminal_error,
                        &product_id,
                        engine_id,
                        emit,
                    );
                }
            }
            RuntimeEvent::ToolEnd {
                id,
                name,
                result: tool_result,
                duration_ms,
            } => {
                if let Some((store, prepared)) = persistence {
                    capture_persistence_error(
                        store.record_side_effect(
                            prepared,
                            SideEffectEvidence {
                                tool_call_id: id,
                                tool_name: name,
                                succeeded: Some(!tool_result.is_error),
                                duration_ms,
                            },
                        ),
                        &mut terminal_error,
                        &product_id,
                        engine_id,
                        emit,
                    );
                }
            }
            RuntimeEvent::Error {
                message,
                recoverable: false,
            } => {
                if terminal_error.is_none() {
                    emit(TimelineEvent {
                        status: TimelineStatus::Failed,
                        engine_id: hub_event.engine_id.to_string(),
                        product_session_id: product_id.clone(),
                        message: Some(message.clone()),
                    });
                    terminal_error = Some(message);
                }
            }
            RuntimeEvent::Completed {
                session_id,
                result: completed_result,
                usage: completed_usage,
            } if terminal_error.is_none() => {
                if !session_id.is_empty() {
                    engine_session_id = Some(session_id);
                }
                usage = completed_usage.as_ref().map(stored_usage);
                result = completed_result;
                emit(TimelineEvent {
                    status: TimelineStatus::Completed,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: result.clone(),
                });
                break;
            }
            RuntimeEvent::Completed {
                session_id,
                usage: completed_usage,
                ..
            } => {
                if !session_id.is_empty() {
                    engine_session_id = Some(session_id);
                }
                usage = completed_usage.as_ref().map(stored_usage);
                break;
            }
            _ => {}
        }
    }

    ExecutedTurn {
        outcome: RunOutcome {
            product_session_id: product_id,
            result,
            failed: terminal_error.is_some(),
        },
        engine_session_id,
        terminal_error,
        usage,
    }
}

fn capture_persistence_error<F>(
    result: Result<(), String>,
    terminal_error: &mut Option<String>,
    product_session_id: &str,
    engine_id: &str,
    emit: &mut F,
) where
    F: FnMut(TimelineEvent),
{
    if let Err(error) = result {
        if terminal_error.is_none() {
            let message = format!("persist engine attempt progress: {error}");
            emit(TimelineEvent {
                status: TimelineStatus::Failed,
                engine_id: engine_id.to_string(),
                product_session_id: product_session_id.to_string(),
                message: Some(message.clone()),
            });
            *terminal_error = Some(message);
        }
    }
}

fn stored_usage(usage: &RuntimeUsage) -> StoredUsage {
    StoredUsage {
        input_tokens: usage.input_tokens,
        output_tokens: usage.output_tokens,
        total_cost_usd: usage.total_cost_usd,
        duration_ms: usage.duration_ms,
        num_turns: usage.num_turns,
    }
}

fn validate_prompt(prompt: &str) -> Result<(), String> {
    if prompt.trim().is_empty() {
        Err("prompt must not be empty".into())
    } else {
        Ok(())
    }
}

fn canonical_workspace(workspace: &std::path::Path) -> Result<PathBuf, String> {
    if !workspace.is_dir() {
        return Err(format!(
            "workspace is not a directory: {}",
            workspace.display()
        ));
    }
    workspace
        .canonicalize()
        .map_err(|error| format!("cannot resolve workspace: {error}"))
}

#[cfg(test)]
mod tests {
    use std::pin::Pin;
    use std::sync::Mutex;

    use alva_engine_runtime::{
        PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest,
    };
    use async_trait::async_trait;
    use futures_core::Stream;

    use super::*;

    struct FakeEngine {
        events: Vec<RuntimeEvent>,
        requests: Arc<Mutex<Vec<RuntimeRequest>>>,
    }

    impl FakeEngine {
        fn new(events: Vec<RuntimeEvent>) -> Self {
            Self {
                events,
                requests: Arc::new(Mutex::new(Vec::new())),
            }
        }
    }

    #[async_trait]
    impl EngineRuntime for FakeEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request);
            Ok(Box::pin(tokio_stream::iter(self.events.clone())))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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

    struct ResumeFailureThenFreshEngine {
        requests: Arc<Mutex<Vec<RuntimeRequest>>>,
    }

    #[async_trait]
    impl EngineRuntime for ResumeFailureThenFreshEngine {
        fn execute(
            &self,
            request: RuntimeRequest,
        ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
            self.requests.lock().unwrap().push(request.clone());
            if request.resume_session.is_some() {
                return Err(RuntimeError::SessionNotFound(
                    "expired native session".into(),
                ));
            }
            Ok(Box::pin(tokio_stream::iter(vec![
                RuntimeEvent::SessionStarted {
                    session_id: "replacement-native".into(),
                    model: None,
                    tools: Vec::new(),
                },
                RuntimeEvent::Completed {
                    session_id: "replacement-native".into(),
                    result: Some("continued after fresh start".into()),
                    usage: None,
                },
            ])))
        }

        async fn cancel(&self, _session_id: &str) -> Result<(), RuntimeError> {
            Ok(())
        }

        async fn respond_permission(
            &self,
            _session_id: &str,
            _request_id: &str,
            _decision: PermissionDecision,
        ) -> Result<(), RuntimeError> {
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

    #[tokio::test]
    async fn projects_successful_engine_run_to_timeline() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("done".into()),
                usage: None,
            },
        ]));
        let requests = engine.requests.clone();
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("done"));
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![TimelineStatus::Running, TimelineStatus::Completed]
        );
        assert!(timeline
            .iter()
            .all(|event| event.product_session_id == outcome.product_session_id));

        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 1);
        assert_eq!(requests[0].prompt, "ship it");
        assert_eq!(
            requests[0].working_directory.as_deref(),
            Some(workspace.path().canonicalize().unwrap().as_path())
        );
        assert!(requests[0].options.streaming);
    }

    #[tokio::test]
    async fn projects_terminal_engine_error_as_failed() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-2".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Error {
                message: "provider unavailable".into(),
                recoverable: false,
            },
            RuntimeEvent::Completed {
                session_id: "native-2".into(),
                result: Some("must not surface".into()),
                usage: None,
            },
        ]));
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        assert_eq!(outcome.result, None);
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![TimelineStatus::Running, TimelineStatus::Failed]
        );
        assert_eq!(
            timeline.last().and_then(|event| event.message.as_deref()),
            Some("provider unavailable")
        );
    }

    #[tokio::test]
    async fn treats_stream_without_terminal_event_as_failed() {
        let workspace = tempfile::tempdir().unwrap();
        let engine = Arc::new(FakeEngine::new(vec![RuntimeEvent::SessionStarted {
            session_id: "native-3".into(),
            model: None,
            tools: Vec::new(),
        }]));
        let mut timeline = Vec::new();

        let outcome = run_once(
            engine,
            RunRequest {
                prompt: "ship it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |event| timeline.push(event),
        )
        .await
        .unwrap();

        assert!(outcome.failed);
        assert_eq!(outcome.result, None);
        assert_eq!(
            timeline
                .iter()
                .map(|event| event.status)
                .collect::<Vec<_>>(),
            vec![TimelineStatus::Running, TimelineStatus::Failed]
        );
        assert_eq!(
            timeline.last().and_then(|event| event.message.as_deref()),
            Some("engine stream ended without terminal Completed event")
        );
    }

    #[tokio::test]
    async fn persists_reopens_and_resumes_a_product_session() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        let first_engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("first result".into()),
                usage: None,
            },
        ]));

        let first = run_persisted(
            &store,
            first_engine,
            RunRequest {
                prompt: "first turn".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_ne!(first.product_session_id, "native-1");
        drop(store);

        let reopened = crate::HubStore::open(&database).unwrap();
        let sessions = reopened.list_sessions().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].product_session_id, first.product_session_id);
        assert_eq!(sessions[0].engine_session_id.as_deref(), Some("native-1"));

        let second_engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-1".into(),
                model: Some("fake-model".into()),
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-1".into(),
                result: Some("second result".into()),
                usage: None,
            },
        ]));
        let second_requests = second_engine.requests.clone();

        let second = run_persisted(
            &reopened,
            second_engine,
            RunRequest {
                prompt: "second turn".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(first.product_session_id.clone()),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(second.product_session_id, first.product_session_id);
        assert_eq!(second.result.as_deref(), Some("second result"));
        assert_eq!(
            second_requests.lock().unwrap()[0].resume_session.as_deref(),
            Some("native-1")
        );

        let detail = reopened
            .get_session(&first.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns.len(), 2);
        assert_eq!(detail.turns[0].prompt, "first turn");
        assert_eq!(detail.turns[0].result.as_deref(), Some("first result"));
        assert_eq!(detail.turns[1].prompt, "second turn");
        assert_eq!(detail.turns[1].result.as_deref(), Some("second result"));
    }

    #[tokio::test]
    async fn persists_attempt_usage_and_side_effect_evidence() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-evidence".into(),
                model: Some("fake-model".into()),
                tools: vec!["write_file".into()],
            },
            RuntimeEvent::ToolStart {
                id: "tool-1".into(),
                name: "write_file".into(),
                input: serde_json::json!({"path": "changed.txt"}),
            },
            RuntimeEvent::ToolEnd {
                id: "tool-1".into(),
                name: "write_file".into(),
                result: serde_json::from_value(serde_json::json!({
                    "content": [{"type": "text", "text": "written"}],
                    "is_error": false
                }))
                .unwrap(),
                duration_ms: Some(14),
            },
            RuntimeEvent::Completed {
                session_id: "native-evidence".into(),
                result: Some("done".into()),
                usage: Some(alva_engine_runtime::RuntimeUsage {
                    input_tokens: 11,
                    output_tokens: 7,
                    total_cost_usd: Some(0.03),
                    duration_ms: 28,
                    num_turns: 1,
                }),
            },
        ]));

        let outcome = run_persisted(
            &store,
            engine,
            RunRequest {
                prompt: "change it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        let detail = store
            .get_session(&outcome.product_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns.len(), 1);
        assert_eq!(detail.turns[0].attempts.len(), 1);
        let attempt = &detail.turns[0].attempts[0];
        assert_eq!(attempt.status, "completed");
        assert!(attempt.started_at > 0);
        assert!(attempt
            .completed_at
            .is_some_and(|end| end >= attempt.started_at));
        assert_eq!(attempt.failure_reason, None);
        assert_eq!(attempt.usage.as_ref().unwrap().input_tokens, 11);
        assert_eq!(attempt.side_effects.len(), 1);
        assert_eq!(attempt.side_effects[0].tool_name, "write_file");
        assert_eq!(attempt.side_effects[0].succeeded, Some(true));
    }

    #[tokio::test]
    async fn recovered_attempt_without_native_session_continues_fresh() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let database = data_dir.path().join("alva-hub.db");
        let store = crate::HubStore::open(&database).unwrap();
        store
            .prepare_new_turn(
                "product-before-start",
                workspace.path(),
                "first",
                &EngineTarget::for_engine("alva").unwrap(),
            )
            .unwrap();
        drop(store);

        let reopened = crate::HubStore::open(&database).unwrap();
        let engine = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "native-fresh".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "native-fresh".into(),
                result: Some("continued".into()),
                usage: None,
            },
        ]));
        let requests = Arc::clone(&engine.requests);

        let outcome = run_persisted(
            &reopened,
            engine,
            RunRequest {
                prompt: "continue".into(),
                workspace: PathBuf::from("ignored-for-resume"),
                engine_id: "alva".into(),
                product_session_id: Some("product-before-start".into()),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert_eq!(outcome.product_session_id, "product-before-start");
        assert_eq!(requests.lock().unwrap()[0].resume_session, None);
        let detail = reopened
            .get_session("product-before-start")
            .unwrap()
            .unwrap();
        assert_eq!(detail.turns[0].status, "interrupted");
        assert_eq!(detail.turns[1].status, "completed");
        assert_eq!(
            detail.session.engine_session_id.as_deref(),
            Some("native-fresh")
        );
    }

    #[tokio::test]
    async fn first_turn_on_a_new_branch_receives_full_then_native_return_receives_delta() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "alva-native-1".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "alva-native-1".into(),
                result: Some("Alva finished the first task".into()),
                usage: None,
            },
        ]));
        let first = run_persisted(
            &store,
            alva,
            RunRequest {
                prompt: "Implement the first task".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: None,
            },
            |_| {},
        )
        .await
        .unwrap();

        store
            .request_engine_switch(
                &first.product_session_id,
                &EngineTarget::for_engine("claude").unwrap(),
                "User explicitly selected Claude",
            )
            .unwrap();
        let claude = Arc::new(FakeEngine::new(vec![
            RuntimeEvent::SessionStarted {
                session_id: "claude-native-1".into(),
                model: None,
                tools: Vec::new(),
            },
            RuntimeEvent::Completed {
                session_id: "claude-native-1".into(),
                result: Some("Claude continued".into()),
                usage: None,
            },
        ]));
        let claude_requests = Arc::clone(&claude.requests);
        run_persisted(
            &store,
            claude,
            RunRequest {
                prompt: "Continue with Claude".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "claude".into(),
                product_session_id: Some(first.product_session_id.clone()),
            },
            |_| {},
        )
        .await
        .unwrap();
        let claude_request = claude_requests.lock().unwrap()[0].clone();
        assert!(claude_request
            .prompt
            .contains("<alva-hub-handoff kind=\"full\">"));
        assert!(claude_request
            .prompt
            .contains("User explicitly selected Claude"));
        assert!(claude_request
            .prompt
            .contains("Alva finished the first task"));
        assert!(claude_request.prompt.contains("Continue with Claude"));
        assert_eq!(claude_request.resume_session, None);

        store
            .request_engine_switch(
                &first.product_session_id,
                &EngineTarget::for_engine("alva").unwrap(),
                "User returned to Alva",
            )
            .unwrap();
        let resumed_alva = Arc::new(FakeEngine::new(vec![RuntimeEvent::Completed {
            session_id: "alva-native-1".into(),
            result: Some("Alva resumed".into()),
            usage: None,
        }]));
        let resumed_requests = Arc::clone(&resumed_alva.requests);
        run_persisted(
            &store,
            resumed_alva,
            RunRequest {
                prompt: "Resume the Alva branch".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some(first.product_session_id),
            },
            |_| {},
        )
        .await
        .unwrap();
        let resumed_request = resumed_requests.lock().unwrap()[0].clone();
        assert_eq!(
            resumed_request.resume_session.as_deref(),
            Some("alva-native-1")
        );
        assert!(resumed_request
            .prompt
            .contains("<alva-hub-handoff kind=\"delta\">"));
        assert!(resumed_request.prompt.contains("User returned to Alva"));
        assert!(resumed_request.prompt.contains("Claude continued"));
    }

    #[tokio::test]
    async fn invalid_native_resume_replaces_delta_with_full_handoff_before_fresh_execution() {
        let workspace = tempfile::tempdir().unwrap();
        let data_dir = tempfile::tempdir().unwrap();
        let store = crate::HubStore::open(data_dir.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let initial = store
            .prepare_new_turn(
                "product-resume-upgrade",
                workspace.path(),
                "initial Alva work",
                &alva,
            )
            .unwrap();
        store
            .record_engine_session(&initial, "expired-native")
            .unwrap();
        store
            .finish_turn(
                &initial,
                Some("expired-native"),
                Some("Alva result"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-resume-upgrade", &claude, "switch to Claude")
            .unwrap();
        let claude_turn = store
            .prepare_resumed_turn("product-resume-upgrade", "Claude follow-up")
            .unwrap();
        store
            .finish_turn(&claude_turn, None, Some("Claude progress"), None, None)
            .unwrap();
        store
            .request_engine_switch("product-resume-upgrade", &alva, "return to Alva")
            .unwrap();

        let requests = Arc::new(Mutex::new(Vec::new()));
        let engine = Arc::new(ResumeFailureThenFreshEngine {
            requests: Arc::clone(&requests),
        });
        let outcome = run_persisted(
            &store,
            engine,
            RunRequest {
                prompt: "finish it".into(),
                workspace: workspace.path().to_path_buf(),
                engine_id: "alva".into(),
                product_session_id: Some("product-resume-upgrade".into()),
            },
            |_| {},
        )
        .await
        .unwrap();

        assert!(!outcome.failed);
        let requests = requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert_eq!(
            requests[0].resume_session.as_deref(),
            Some("expired-native")
        );
        assert!(requests[0].prompt.contains("kind=\"delta\""));
        assert!(requests[1].resume_session.is_none());
        assert!(requests[1].prompt.contains("kind=\"full\""));
        drop(requests);

        let detail = store
            .get_session("product-resume-upgrade")
            .unwrap()
            .unwrap();
        assert_eq!(
            detail.session.engine_session_id.as_deref(),
            Some("replacement-native")
        );
        let latest_turn = detail.turns.last().unwrap();
        assert_eq!(latest_turn.attempts.len(), 2);
        assert_eq!(latest_turn.attempts[0].status, "failed");
        assert_eq!(latest_turn.attempts[1].status, "completed");
    }
}
