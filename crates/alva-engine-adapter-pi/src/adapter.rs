// INPUT:  Pi config/process/protocol/mapping and alva_engine_runtime
// OUTPUT: PiAdapter implementing EngineRuntime
// POS:    Runs Pi JSON print-mode sessions and maps their native event stream.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::warn;
use uuid::Uuid;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};

use crate::config::PiAdapterConfig;
use crate::mapping::EventMapper;
use crate::process::PiProcess;
use crate::protocol::PiEvent;

type KillSender = mpsc::UnboundedSender<()>;

pub struct PiAdapter {
    config: PiAdapterConfig,
    sessions: Arc<Mutex<HashMap<String, KillSender>>>,
}

impl PiAdapter {
    pub fn new(config: PiAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for PiAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel();
        let provisional_id = request
            .resume_session
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let sessions = self.sessions.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            sessions
                .lock()
                .await
                .insert(provisional_id.clone(), kill_tx);
            let mut process = match PiProcess::spawn(&config, &request, &provisional_id).await {
                Ok(process) => process,
                Err(error) => {
                    let _ = event_tx.send(RuntimeEvent::Error {
                        message: error.to_string(),
                        recoverable: false,
                    });
                    let _ = event_tx.send(RuntimeEvent::Completed {
                        session_id: provisional_id.clone(),
                        result: None,
                        usage: None,
                    });
                    sessions.lock().await.remove(&provisional_id);
                    return;
                }
            };

            let mut mapper = EventMapper::new();
            let mut native_session = provisional_id.clone();
            loop {
                tokio::select! {
                    _ = kill_rx.recv() => {
                        if let Err(error) = process.kill().await {
                            warn!(%error, "pi kill failed");
                        }
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Pi session cancelled".into(),
                            recoverable: false,
                        });
                        break;
                    }
                    line = process.next_line() => {
                        match line {
                            Ok(Some(raw)) => {
                                let raw = raw.trim();
                                if raw.is_empty() {
                                    continue;
                                }
                                let event: PiEvent = match serde_json::from_str(raw) {
                                    Ok(event) => event,
                                    Err(error) => {
                                        warn!(%error, "skipping unparseable Pi JSONL line");
                                        continue;
                                    }
                                };
                                for runtime_event in mapper.map(event) {
                                    if let RuntimeEvent::SessionStarted { session_id, .. } = &runtime_event {
                                        if session_id != &native_session {
                                            let mut map = sessions.lock().await;
                                            if let Some(sender) = map.remove(&native_session) {
                                                map.insert(session_id.clone(), sender);
                                            }
                                            native_session = session_id.clone();
                                        }
                                    }
                                    let _ = event_tx.send(runtime_event);
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: error.to_string(),
                                    recoverable: false,
                                });
                                break;
                            }
                        }
                    }
                }
            }
            let session_id = mapper
                .session_id()
                .unwrap_or(native_session.as_str())
                .to_string();
            let result = if mapper.failed() {
                None
            } else {
                mapper.last_agent_text().map(str::to_string)
            };
            let _ = event_tx.send(RuntimeEvent::Completed {
                session_id: session_id.clone(),
                result,
                usage: Some(mapper.usage()),
            });
            sessions.lock().await.remove(&session_id);
            sessions.lock().await.remove(&provisional_id);
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let sender = self
            .sessions
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;
        sender
            .send(())
            .map_err(|_| RuntimeError::SessionNotFound(session_id.into()))
    }

    async fn respond_permission(
        &self,
        _session_id: &str,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::Unsupported(
            "Pi print mode does not expose interactive permission callbacks".into(),
        ))
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
