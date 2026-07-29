// INPUT:  ClaudePrintConfig, print_process, mapping, alva_engine_runtime
// OUTPUT: ClaudePrintAdapter implementing EngineRuntime
// POS:    Light-mode Claude engine: spawn `claude -p` and map stream-json → RuntimeEvent.

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

use crate::mapping::EventMapper;
use crate::print_config::ClaudePrintConfig;
use crate::print_process::{parse_print_line, ClaudePrintProcess};
use crate::protocol::BridgeMessage;

type KillSender = mpsc::UnboundedSender<()>;

/// Light / exec-style Claude engine for Alva Hub.
///
/// Spawns `claude -p --output-format stream-json` and maps JSONL frames with the
/// same [`EventMapper`] used by the full SDK bridge (print wire == SDK wire).
pub struct ClaudePrintAdapter {
    config: ClaudePrintConfig,
    sessions: Arc<Mutex<HashMap<String, KillSender>>>,
}

impl ClaudePrintAdapter {
    pub fn new(config: ClaudePrintConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for ClaudePrintAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel();
        let provisional_id = Uuid::new_v4().to_string();
        let sessions = self.sessions.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            {
                let mut map = sessions.lock().await;
                map.insert(provisional_id.clone(), kill_tx);
            }

            let mut process = match ClaudePrintProcess::spawn(&config, &request).await {
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
                            warn!(%error, "claude -p kill failed");
                        }
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Claude CLI session cancelled".into(),
                            recoverable: false,
                        });
                        break;
                    }
                    line = process.next_line() => {
                        match line {
                            Ok(Some(raw)) => {
                                let msg = match parse_print_line(&raw) {
                                    Ok(Some(msg)) => msg,
                                    Ok(None) => continue,
                                    Err(error) => {
                                        warn!(%error, "claude -p parse error");
                                        continue;
                                    }
                                };
                                // Reuse full-mode mapper: wrap as bridge SdkMessage.
                                for runtime_event in mapper.map(BridgeMessage::SdkMessage {
                                    message: msg,
                                }) {
                                    if let RuntimeEvent::SessionStarted { session_id, .. } =
                                        &runtime_event
                                    {
                                        if session_id != &native_session {
                                            let mut map = sessions.lock().await;
                                            if let Some(sender) = map.remove(&native_session) {
                                                map.insert(session_id.clone(), sender);
                                            }
                                            native_session = session_id.clone();
                                        }
                                    }
                                    let is_completed =
                                        matches!(&runtime_event, RuntimeEvent::Completed { .. });
                                    let _ = event_tx.send(runtime_event);
                                    if is_completed {
                                        sessions.lock().await.remove(&native_session);
                                        sessions.lock().await.remove(&provisional_id);
                                        return;
                                    }
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

            // Result message normally emits Completed and returns above.
            // Reaching here means EOF/cancel/error without a terminal Completed.
            let session_id = if mapper.session_id().is_empty() {
                native_session.clone()
            } else {
                mapper.session_id().to_string()
            };
            let _ = event_tx.send(RuntimeEvent::Error {
                message: "Claude CLI stream ended without a result".into(),
                recoverable: false,
            });
            let _ = event_tx.send(RuntimeEvent::Completed {
                session_id: session_id.clone(),
                result: None,
                usage: None,
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
        if sender.send(()).is_err() {
            return Err(RuntimeError::SessionNotFound(session_id.into()));
        }
        Ok(())
    }

    async fn respond_permission(
        &self,
        _session_id: &str,
        _request_id: &str,
        _decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        Err(RuntimeError::Unsupported(
            "Claude light mode (`claude -p`) does not support interactive permission callbacks; \
             use --claude-mode full (SDK) or a non-interactive permission mode"
                .into(),
        ))
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: true,
            permission_callback: false,
            // CLI supports --resume; product resume via Hub needs session id from init.
            resume: true,
            cancel: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::print_config::ClaudePrintPermissionMode;

    #[test]
    fn default_print_config_is_non_interactive() {
        let c = ClaudePrintConfig::default();
        assert_eq!(c.claude_path, "claude");
        assert!(
            !c.bare,
            "bare must stay off by default so Claude Code OAuth login works"
        );
        assert_eq!(
            c.permission_mode,
            ClaudePrintPermissionMode::BypassPermissions
        );
    }

    #[test]
    fn capabilities_mark_no_permission_callback() {
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig::default());
        let caps = adapter.capabilities();
        assert!(!caps.permission_callback);
        assert!(caps.cancel);
        assert!(caps.resume);
    }
}
