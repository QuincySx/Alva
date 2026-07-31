// INPUT:  alva_engine_runtime, alva_hub_core, serde, tokio_stream
// OUTPUT: run_once plus request/outcome/timeline product DTOs
// POS:    Highest product seam for one Hub Desktop turn, independent of Tauri.

use std::path::PathBuf;
use std::sync::Arc;

use alva_engine_runtime::{EngineRuntime, RuntimeEvent, RuntimeRequest};
use alva_hub_core::{HubRequest, HubRuntime, ProductSessionId};
use serde::{Deserialize, Serialize};
use tokio_stream::StreamExt;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
pub struct RunRequest {
    pub prompt: String,
    pub workspace: PathBuf,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TimelineStatus {
    Running,
    Completed,
    Failed,
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

pub async fn run_once<F>(
    engine: Arc<dyn EngineRuntime>,
    request: RunRequest,
    mut emit: F,
) -> Result<RunOutcome, String>
where
    F: FnMut(TimelineEvent) + Send,
{
    if request.prompt.trim().is_empty() {
        return Err("prompt must not be empty".into());
    }
    if !request.workspace.is_dir() {
        return Err(format!(
            "workspace is not a directory: {}",
            request.workspace.display()
        ));
    }
    let workspace = request
        .workspace
        .canonicalize()
        .map_err(|error| format!("cannot resolve workspace: {error}"))?;

    let hub = HubRuntime::new();
    hub.register_engine("alva", engine)
        .map_err(|error| error.to_string())?;

    let product_session_id = ProductSessionId::new();
    let product_id = product_session_id.as_str().to_string();
    emit(TimelineEvent {
        status: TimelineStatus::Running,
        engine_id: "alva".into(),
        product_session_id: product_id.clone(),
        message: None,
    });

    let runtime_request = RuntimeRequest::new(request.prompt)
        .with_cwd(workspace)
        .with_streaming();
    let hub_request = HubRequest::new("alva", runtime_request).resume(product_session_id);
    let mut stream = match hub.execute(hub_request) {
        Ok(stream) => stream,
        Err(error) => {
            let message = error.to_string();
            emit(TimelineEvent {
                status: TimelineStatus::Failed,
                engine_id: "alva".into(),
                product_session_id: product_id.clone(),
                message: Some(message),
            });
            return Ok(RunOutcome {
                product_session_id: product_id,
                result: None,
                failed: true,
            });
        }
    };

    let mut terminal_error = None;
    let mut result = None;
    while let Some(hub_event) = stream.next().await {
        match hub_event.event {
            RuntimeEvent::Error {
                message,
                recoverable: false,
            } => {
                emit(TimelineEvent {
                    status: TimelineStatus::Failed,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: Some(message.clone()),
                });
                terminal_error = Some(message);
                break;
            }
            RuntimeEvent::Completed {
                result: completed_result,
                ..
            } => {
                result = completed_result;
                emit(TimelineEvent {
                    status: TimelineStatus::Completed,
                    engine_id: hub_event.engine_id.to_string(),
                    product_session_id: product_id.clone(),
                    message: result.clone(),
                });
                break;
            }
            _ => {}
        }
    }

    Ok(RunOutcome {
        product_session_id: product_id,
        result,
        failed: terminal_error.is_some(),
    })
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
}
