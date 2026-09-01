// INPUT:  CodexAdapterConfig, process/protocol/mapping/retry_escalation, alva_engine_runtime
// OUTPUT: CodexAdapter implementing EngineRuntime
// POS:    Spawns `codex exec --json` per session and streams mapped RuntimeEvents.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::warn;
use uuid::Uuid;

use alva_engine_runtime::{
    is_fatal_error, EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError,
    RuntimeEvent, RuntimeRequest,
};

use crate::config::CodexAdapterConfig;
use crate::mapping::EventMapper;
use crate::process::CodexProcess;
use crate::protocol::CodexEvent;
use crate::retry_escalation::{build_escalation_message, RecoverableRetryTracker};

type KillSender = mpsc::UnboundedSender<()>;

/// Codex CLI engine for Alva Hub.
///
/// Each `execute` spawns `codex exec --json` (or `codex exec resume <id>
/// --json` when `resume_session` is set) and maps the JSONL event stream
/// into `RuntimeEvent`. Cancel kills the child process. Interactive
/// permission callbacks are not supported (approval_policy defaults to
/// never for non-interactive runs).
///
/// Recoverable/network errors are forwarded as `recoverable: true` until
/// [`RecoverableRetryTracker`] escalates (cindy retry-escalation ceiling).
pub struct CodexAdapter {
    config: CodexAdapterConfig,
    sessions: Arc<Mutex<HashMap<String, KillSender>>>,
}

impl CodexAdapter {
    pub fn new(config: CodexAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for CodexAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (kill_tx, mut kill_rx) = mpsc::unbounded_channel();
        // Prefer the native session id when resuming so Hub journal / cancel
        // map keys stay stable across process restarts.
        let provisional_id = request
            .resume_session
            .clone()
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        let sessions = self.sessions.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            {
                let mut map = sessions.lock().await;
                map.insert(provisional_id.clone(), kill_tx);
            }

            let mut process = match CodexProcess::spawn(&config, &request).await {
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
            let mut retry_tracker = RecoverableRetryTracker::new();
            let mut escalated = false;
            // Stream contract R2: after a non-recoverable Error only Completed may
            // follow, so stop reading as soon as one is forwarded.
            let mut fatal_seen = false;

            loop {
                tokio::select! {
                    _ = kill_rx.recv() => {
                        if let Err(error) = process.kill().await {
                            warn!(%error, "codex kill failed");
                        }
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Codex session cancelled".into(),
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
                                let event: CodexEvent = match serde_json::from_str(raw) {
                                    Ok(event) => event,
                                    Err(error) => {
                                        warn!(%error, %raw, "skipping unparseable codex JSONL line");
                                        continue;
                                    }
                                };
                                // New turn resets recoverable retry budget (cindy).
                                if matches!(event, CodexEvent::TurnStarted) {
                                    retry_tracker.reset();
                                }
                                for runtime_event in mapper.map(event) {
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

                                    // cindy retry-escalation: recoverable errors may
                                    // continue the stream, but unbounded reconnect is
                                    // escalated to a single fatal close + kill.
                                    if let RuntimeEvent::Error {
                                        message,
                                        recoverable: true,
                                    } = &runtime_event
                                    {
                                        let decision = retry_tracker.track(Instant::now());
                                        if decision.escalate {
                                            escalated = true;
                                            let fatal_msg =
                                                build_escalation_message(&decision, message);
                                            warn!(%fatal_msg, "codex recoverable retries escalated");
                                            if let Err(error) = process.kill().await {
                                                warn!(%error, "codex kill after escalation failed");
                                            }
                                            let _ = event_tx.send(RuntimeEvent::Error {
                                                message: fatal_msg,
                                                recoverable: false,
                                            });
                                            break;
                                        }
                                    }

                                    let fatal = is_fatal_error(&runtime_event);
                                    let _ = event_tx.send(runtime_event);
                                    if fatal {
                                        fatal_seen = true;
                                        break;
                                    }
                                }
                                if escalated || fatal_seen {
                                    break;
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
                // Unreachable in practice: every path that sets `escalated` /
                // `fatal_seen` already broke out of the loop inside the arm.
                // Kept as-is (pre-existing) rather than widened.
                if escalated {
                    break;
                }
            }

            let session_id = mapper
                .session_id()
                .unwrap_or(native_session.as_str())
                .to_string();
            // Fatal path: never put a success answer on Completed (stream soft rule).
            let result = if mapper.failed() || escalated {
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
            "Codex exec adapter does not support interactive permission callbacks; \
             configure approval_policy for non-interactive runs"
                .into(),
        ))
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: false,
            tool_control: true,
            permission_callback: false,
            // `codex exec resume <id> --json` (host Codex CLI must support it).
            resume: true,
            cancel: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_engine_runtime::validate_event_stream;
    use tokio_stream::StreamExt;

    #[cfg(unix)]
    fn write_fake_codex(path: &std::path::Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, script).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    /// Stream contract R2: only `Completed` may follow a non-recoverable
    /// `Error`. `codex exec --json` keeps printing after a failed turn, so the
    /// adapter must stop reading rather than forward the trailing items.
    #[cfg(unix)]
    #[tokio::test]
    async fn stops_reading_after_fatal_error() {
        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-codex");
        write_fake_codex(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-1"}'
printf '%s\n' '{"type":"turn.failed","error":{"message":"usage limit reached"}}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"must not be forwarded"}}'
"#,
        );

        let adapter = CodexAdapter::new(CodexAdapterConfig {
            codex_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture.path().to_path_buf()),
            ..Default::default()
        });

        let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        assert!(
            !events
                .iter()
                .any(|event| matches!(event, RuntimeEvent::Message { .. })),
            "no Message may follow a fatal Error: {events:#?}"
        );
        assert_eq!(
            events.len(),
            3,
            "expected SessionStarted / Error / Completed: {events:#?}"
        );
        assert!(matches!(events[0], RuntimeEvent::SessionStarted { .. }));
        assert!(matches!(
            events[1],
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(matches!(
            events[2],
            RuntimeEvent::Completed { result: None, .. }
        ));
        assert!(validate_event_stream(&events).is_ok());
    }
}
