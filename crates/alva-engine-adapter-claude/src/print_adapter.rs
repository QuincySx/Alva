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

async fn remove_session_if_current(
    sessions: &Mutex<HashMap<String, KillSender>>,
    session_id: &str,
    expected_sender: &KillSender,
) {
    let mut sessions = sessions.lock().await;
    if sessions
        .get(session_id)
        .is_some_and(|sender| sender.same_channel(expected_sender))
    {
        sessions.remove(session_id);
    }
}

async fn register_session(
    sessions: &Mutex<HashMap<String, KillSender>>,
    session_id: &str,
    sender: KillSender,
) -> Result<(), RuntimeError> {
    let mut sessions = sessions.lock().await;
    if let Some(existing) = sessions.get(session_id) {
        if existing.same_channel(&sender) {
            return Ok(());
        }
        if !existing.is_closed() {
            return Err(RuntimeError::ProcessError(
                "Duplicate active Claude session ID".into(),
            ));
        }
    }
    sessions.insert(session_id.into(), sender);
    Ok(())
}

async fn cleanup_session(
    process: &mut ClaudePrintProcess,
    sessions: &Mutex<HashMap<String, KillSender>>,
    provisional_id: &str,
    active_session_id: &str,
    expected_sender: &KillSender,
) {
    remove_session_if_current(sessions, active_session_id, expected_sender).await;
    remove_session_if_current(sessions, provisional_id, expected_sender).await;
    let _ = process.kill().await;
}

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
        let session_sender = kill_tx.clone();

        tokio::spawn(async move {
            {
                let mut map = sessions.lock().await;
                map.insert(provisional_id.clone(), session_sender.clone());
            }

            let mut process = match ClaudePrintProcess::spawn(&config, &request).await {
                Ok(process) => process,
                Err(error) => {
                    remove_session_if_current(&sessions, &provisional_id, &session_sender).await;
                    let _ = event_tx.send(RuntimeEvent::Error {
                        message: error.to_string(),
                        recoverable: false,
                    });
                    let _ = event_tx.send(RuntimeEvent::Completed {
                        session_id: provisional_id.clone(),
                        result: None,
                        usage: None,
                    });
                    return;
                }
            };

            let mut mapper = EventMapper::new();
            let mut native_session = provisional_id.clone();

            loop {
                tokio::select! {
                    _ = event_tx.closed() => {
                        cleanup_session(
                            &mut process,
                            &sessions,
                            &provisional_id,
                            &native_session,
                            &session_sender,
                        ).await;
                        return;
                    }
                    Some(()) = kill_rx.recv() => {
                        let session_id = if mapper.session_id().is_empty() {
                            native_session.clone()
                        } else {
                            mapper.session_id().to_string()
                        };
                        cleanup_session(
                            &mut process,
                            &sessions,
                            &provisional_id,
                            &native_session,
                            &session_sender,
                        ).await;
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Claude CLI session cancelled".into(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id,
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    line = process.next_line() => {
                        match line {
                            Ok(Some(raw)) => {
                                let msg = match parse_print_line(&raw) {
                                    Ok(Some(msg)) => msg,
                                    Ok(None) => continue,
                                    Err(_) => {
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
                                            if let Err(error) = register_session(
                                                &sessions,
                                                session_id,
                                                session_sender.clone(),
                                            ).await {
                                                cleanup_session(
                                                    &mut process,
                                                    &sessions,
                                                    &provisional_id,
                                                    &native_session,
                                                    &session_sender,
                                                ).await;
                                                let _ = event_tx.send(RuntimeEvent::Error {
                                                    message: error.to_string(),
                                                    recoverable: false,
                                                });
                                                let _ = event_tx.send(RuntimeEvent::Completed {
                                                    session_id: session_id.clone(),
                                                    result: None,
                                                    usage: None,
                                                });
                                                return;
                                            }
                                            remove_session_if_current(
                                                &sessions,
                                                &native_session,
                                                &session_sender,
                                            ).await;
                                            native_session = session_id.clone();
                                        }
                                    }
                                    let is_completed =
                                        matches!(&runtime_event, RuntimeEvent::Completed { .. });
                                    if is_completed {
                                        cleanup_session(
                                            &mut process,
                                            &sessions,
                                            &provisional_id,
                                            &native_session,
                                            &session_sender,
                                        ).await;
                                        let _ = event_tx.send(runtime_event);
                                        return;
                                    }
                                    if event_tx.send(runtime_event).is_err() {
                                        cleanup_session(
                                            &mut process,
                                            &sessions,
                                            &provisional_id,
                                            &native_session,
                                            &session_sender,
                                        ).await;
                                        return;
                                    }
                                }
                            }
                            Ok(None) => {
                                let session_id = if mapper.session_id().is_empty() {
                                    native_session.clone()
                                } else {
                                    mapper.session_id().to_string()
                                };
                                cleanup_session(
                                    &mut process,
                                    &sessions,
                                    &provisional_id,
                                    &native_session,
                                    &session_sender,
                                ).await;
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: "Claude CLI stream ended without a result".into(),
                                    recoverable: false,
                                });
                                let _ = event_tx.send(RuntimeEvent::Completed {
                                    session_id,
                                    result: None,
                                    usage: None,
                                });
                                return;
                            }
                            Err(_) => {
                                let session_id = if mapper.session_id().is_empty() {
                                    native_session.clone()
                                } else {
                                    mapper.session_id().to_string()
                                };
                                cleanup_session(
                                    &mut process,
                                    &sessions,
                                    &provisional_id,
                                    &native_session,
                                    &session_sender,
                                ).await;
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: "Claude CLI stream read failed".into(),
                                    recoverable: false,
                                });
                                let _ = event_tx.send(RuntimeEvent::Completed {
                                    session_id,
                                    result: None,
                                    usage: None,
                                });
                                return;
                            }
                        }
                    }
                }
            }
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
            remove_session_if_current(&self.sessions, session_id, &sender).await;
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

    #[cfg(unix)]
    fn fake_claude(path: &std::path::Path, body: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    #[test]
    fn default_print_config_is_non_interactive() {
        let c = ClaudePrintConfig::default();
        assert_eq!(c.claude_path, "claude");
        assert!(c.safe_mode);
        assert_eq!(c.permission_mode, ClaudePrintPermissionMode::DontAsk);
    }

    #[test]
    fn capabilities_mark_no_permission_callback() {
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig::default());
        let caps = adapter.capabilities();
        assert!(!caps.permission_callback);
        assert!(caps.cancel);
        assert!(caps.resume);
    }

    #[tokio::test]
    async fn stale_cancel_route_is_removed_after_send_failure() {
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig::default());
        let (sender, receiver) = mpsc::unbounded_channel();
        drop(receiver);
        adapter
            .sessions
            .lock()
            .await
            .insert("stale-session".into(), sender);

        assert!(matches!(
            adapter.cancel("stale-session").await,
            Err(RuntimeError::SessionNotFound(id)) if id == "stale-session"
        ));
        assert!(!adapter.sessions.lock().await.contains_key("stale-session"));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_emits_one_fatal_error_then_completed() {
        use tokio_stream::StreamExt;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-claude");
        fake_claude(
            &executable,
            "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"cancel-session\",\"model\":\"fake\"}'\nexec /bin/sleep 30",
        );
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig {
            claude_path: executable.to_string_lossy().into_owned(),
            ..Default::default()
        });
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(fixture.path()))
            .unwrap();
        let session_id =
            match tokio::time::timeout(std::time::Duration::from_secs(2), events.next())
                .await
                .unwrap()
                .unwrap()
            {
                RuntimeEvent::SessionStarted { session_id, .. } => session_id,
                event => panic!("expected SessionStarted, got {event:?}"),
            };
        adapter.cancel(&session_id).await.unwrap();
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            vec![events.next().await.unwrap(), events.next().await.unwrap()]
        })
        .await
        .unwrap();
        assert!(matches!(
            terminal.as_slice(),
            [
                RuntimeEvent::Error {
                    message,
                    recoverable: false,
                },
                RuntimeEvent::Completed { result: None, .. },
            ] if message == "Claude CLI session cancelled"
        ));
        assert!(matches!(
            adapter.cancel(&session_id).await,
            Err(RuntimeError::SessionNotFound(id)) if id == session_id
        ));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn consumer_drop_cleans_the_native_session_route() {
        use tokio_stream::StreamExt;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-claude");
        fake_claude(
            &executable,
            "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"dropped-session\",\"model\":\"fake\"}'\nexec /bin/sleep 30",
        );
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig {
            claude_path: executable.to_string_lossy().into_owned(),
            ..Default::default()
        });
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(fixture.path()))
            .unwrap();
        let session_id = match events.next().await.unwrap() {
            RuntimeEvent::SessionStarted { session_id, .. } => session_id,
            event => panic!("expected SessionStarted, got {event:?}"),
        };
        drop(events);

        tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if !adapter.sessions.lock().await.contains_key(&session_id) {
                    break;
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("dropping the event consumer must clear the session route");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn duplicate_native_session_does_not_replace_the_active_route() {
        use tokio_stream::StreamExt;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-claude");
        fake_claude(
            &executable,
            "printf '%s\\n' '{\"type\":\"system\",\"subtype\":\"init\",\"session_id\":\"shared-session\",\"model\":\"fake\"}'\nexec /bin/sleep 30",
        );
        let adapter = ClaudePrintAdapter::new(ClaudePrintConfig {
            claude_path: executable.to_string_lossy().into_owned(),
            ..Default::default()
        });
        let mut first = adapter
            .execute(RuntimeRequest::new("first").with_cwd(fixture.path()))
            .unwrap();
        assert!(matches!(
            first.next().await,
            Some(RuntimeEvent::SessionStarted { session_id, .. }) if session_id == "shared-session"
        ));

        let mut duplicate = adapter
            .execute(RuntimeRequest::new("second").with_cwd(fixture.path()))
            .unwrap();
        let terminal = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            vec![
                duplicate.next().await.unwrap(),
                duplicate.next().await.unwrap(),
            ]
        })
        .await
        .unwrap();
        assert!(matches!(
            terminal.as_slice(),
            [RuntimeEvent::Error { message, recoverable: false }, RuntimeEvent::Completed { .. }]
                if message.contains("Duplicate active Claude session ID")
        ));

        adapter.cancel("shared-session").await.unwrap();
        assert!(matches!(
            first.next().await,
            Some(RuntimeEvent::Error { message, recoverable: false }) if message == "Claude CLI session cancelled"
        ));
    }
}
