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
    is_fatal_error, ConfigSurface, EngineDescriptor, EngineRuntime, InstallMetadata,
    PermissionDecision, PermissionPlanDecl, PermissionRelation, ProviderProtocol,
    RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest,
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

    /// Self-description (ticket 15 batch 3).
    pub fn descriptor() -> EngineDescriptor {
        EngineDescriptor {
            runtime: Arc::new(PiAdapter::new(PiAdapterConfig::default())),
            inspector: None,
            // The adapter enforces read-only tool policy; it can satisfy a
            // read-only request equivalently, and write requests only more
            // strictly (never weaker).
            permission_plans: vec![
                PermissionPlanDecl {
                    profile_id: "workspace_read".into(),
                    relation: PermissionRelation::Equivalent,
                    summary: "pi read-only tool policy".into(),
                },
                PermissionPlanDecl {
                    profile_id: "workspace_write".into(),
                    relation: PermissionRelation::Stricter,
                    summary: "pi read-only tool policy (stricter than write)".into(),
                },
            ],
            config_surface: ConfigSurface {
                supports_max_turns: false,
                provider_protocol: ProviderProtocol::Native,
                supports_fast_mode: false,
                supports_custom_http_headers: false,
            },
            install_metadata: InstallMetadata {
                required_binary: Some("pi".into()),
                install_hint: Some("npm i -g @earendil-works/pi-coding-agent".into()),
            },
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
            // Stream contract R2: after a non-recoverable Error only Completed may
            // follow, so stop reading as soon as one is forwarded.
            let mut fatal_seen = false;
            // R52: distinguish a clean engine terminal from a bare EOF — a
            // stream that ends without the engine's own Completed is an
            // outage and must surface a fatal Error before the synthesized
            // terminal pair.
            // R82: pi's own whole-turn terminal is `agent_end` with
            // willRetry:false (message_end is per-message, and a retrying
            // agent_end keeps the turn open). It maps to no outward event,
            // so "seen the native terminal" is tracked here; the adapter
            // still sends exactly one Completed when the stream ends.
            let mut native_terminal_seen = false;
            let mut error_forwarded = false;
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
                        error_forwarded = true;
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
                                // R82: only a non-retrying agent_end closes the
                                // whole turn; a retrying one keeps it open.
                                if matches!(event, PiEvent::AgentEnd { will_retry: false }) {
                                    native_terminal_seen = true;
                                }
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
                                    let fatal = is_fatal_error(&runtime_event);
                                    if fatal {
                                        error_forwarded = true;
                                    }
                                    let _ = event_tx.send(runtime_event);
                                    if fatal {
                                        fatal_seen = true;
                                        break;
                                    }
                                }
                                if fatal_seen {
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                let _ = event_tx.send(RuntimeEvent::Error {
                                    message: error.to_string(),
                                    recoverable: false,
                                });
                                error_forwarded = true;
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
            if !native_terminal_seen && !error_forwarded && !fatal_seen {
                let _ = event_tx.send(RuntimeEvent::Error {
                    message: "pi exited without a terminal event".into(),
                    recoverable: false,
                });
            }
            let result = if mapper.failed() || !native_terminal_seen {
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

#[cfg(test)]
mod tests {
    use super::*;
    use alva_engine_runtime::validate_event_stream;
    use tokio_stream::StreamExt;

    fn pi_adapter(executable: std::path::PathBuf) -> PiAdapter {
        let fixture_dir = executable.parent().unwrap().to_path_buf();
        PiAdapter::new(PiAdapterConfig {
            pi_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture_dir.clone()),
            session_dir: fixture_dir.join("sessions"),
            ..Default::default()
        })
    }

    #[cfg(unix)]
    fn write_fake_pi(path: &std::path::Path, script: &str) {
        use std::os::unix::fs::PermissionsExt;

        std::fs::write(path, script).unwrap();
        let mut permissions = std::fs::metadata(path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(path, permissions).unwrap();
    }

    /// A normal Pi failure emits `message_update{error}` immediately followed by
    /// `message_end{stopReason:"error"}`, and both map to a fatal `Error`.
    /// Stream contract R2 allows only the first one to reach the consumer.
    #[cfg(unix)]
    #[tokio::test]
    async fn stops_reading_after_fatal_error() {
        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-pi");
        write_fake_pi(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
printf '%s\n' '{"type":"message_start","message":{"role":"assistant"}}'
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"errorMessage":"upstream refused"}}}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"must not be forwarded"}}'
"#,
        );

        let adapter = PiAdapter::new(PiAdapterConfig {
            pi_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture.path().to_path_buf()),
            session_dir: fixture.path().join("sessions"),
            ..Default::default()
        });

        let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
        let mut events = Vec::new();
        while let Some(event) = stream.next().await {
            events.push(event);
        }

        let fatals: Vec<&str> = events
            .iter()
            .filter_map(|event| match event {
                RuntimeEvent::Error {
                    message,
                    recoverable: false,
                } => Some(message.as_str()),
                _ => None,
            })
            .collect();
        // Asserting the count alone cannot tell "the break worked" apart from
        // "only one fatal was ever produced" (e.g. the message_update line
        // drifting into PiEvent::Unknown). Pinning *which* fatal arrived keeps
        // the test honest if either wire shape changes.
        assert_eq!(
            fatals,
            vec!["upstream refused"],
            "only the first fatal Error may be forwarded: {events:#?}"
        );
        assert!(
            matches!(events.last(), Some(RuntimeEvent::Completed { .. })),
            "Completed must be the terminal event: {events:#?}"
        );
        assert!(validate_event_stream(&events).is_ok());
    }

    /// ticket 10: cancel must close the stream with Error{fatal} + Completed
    /// within a bounded time, even when the child would otherwise run forever.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_emits_one_fatal_error_then_completed() {
        use std::time::Duration;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-pi");
        write_fake_pi(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
sleep 600
"#,
        );

        let adapter = Arc::new(PiAdapter::new(PiAdapterConfig {
            pi_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture.path().to_path_buf()),
            session_dir: fixture.path().join("sessions"),
            ..Default::default()
        }));
        let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
        let session_id = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = stream.next().await {
                if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                    return session_id;
                }
            }
            panic!("fake pi stream ended before SessionStarted")
        })
        .await
        .expect("fake pi must start");

        let collector = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = stream.next().await {
                events.push(event);
            }
            events
        });

        adapter
            .cancel(&session_id)
            .await
            .expect("cancel must be deliverable");

        let events = tokio::time::timeout(Duration::from_secs(10), collector)
            .await
            .expect("cancel must close the pi stream")
            .unwrap();
        assert!(
            matches!(
                events.as_slice(),
                [
                    RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    },
                    RuntimeEvent::Completed { result: None, .. }
                ]
            ),
            "cancel must end with Error{{fatal}} + Completed: {events:#?}"
        );
        assert!(validate_event_stream(&events).is_ok());
    }

    /// ticket 16: five canned scenarios against the whole execute() boundary —
    /// pi previously had zero contract assertions on its streaming loop.
    #[cfg(unix)]
    mod conformance {
        use super::*;
        use alva_engine_runtime::{
            assert_conforms, assert_conforms_events, collect_stream, RuntimeRequest, Scenario,
        };
        use std::time::Duration;

        #[tokio::test]
        async fn normal_run_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-pi");
            write_fake_pi(
                &executable,
                r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
printf '%s\n' '{"type":"message_start","message":{"role":"assistant","id":"m1"}}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","id":"m1","stopReason":"end","content":[{"type":"text","text":"two plus two is four"}],"usage":{"input":1,"output":1,"cost":{"total":0.001}}}}'
printf '%s\n' '{"type":"agent_end","willRetry":false}'"#,
            );
            // R83: the fixture ends with the engine's REAL whole-turn
            // terminal (agent_end, no retry pending) and a known answer, so
            // the harness plus a body/usage assertion pin that a normal run
            // keeps both.
            let events = collect_stream(&pi_adapter(executable), RuntimeRequest::new("hi"))
                .await
                .expect("normal pi run must stream");
            assert_conforms_events(&events, Scenario::Normal);
            let completed = events.iter().find_map(|event| match event {
                RuntimeEvent::Completed { result, usage, .. } => {
                    Some((result.clone(), usage.clone()))
                }
                _ => None,
            });
            let (result, usage) = completed.expect("Normal must end with Completed");
            assert_eq!(result.as_deref(), Some("two plus two is four"));
            let usage = usage.expect("pi message_end carries usage");
            assert_eq!(usage.input_tokens, 1);
            assert_eq!(usage.output_tokens, 1);
        }

        #[tokio::test]
        async fn spawn_failure_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let adapter = pi_adapter(fixture.path().join("no-such-pi"));
            assert_conforms(&adapter, RuntimeRequest::new("hi"), Scenario::SpawnFailure).await;
        }

        #[tokio::test]
        async fn killed_mid_run_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-pi");
            write_fake_pi(
                &executable,
                r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
exec /bin/sleep 60
"#,
            );
            let adapter = Arc::new(pi_adapter(executable));
            let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
            let session_id = tokio::time::timeout(Duration::from_secs(5), async {
                while let Some(event) = stream.next().await {
                    if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                        return session_id;
                    }
                }
                panic!("fake pi must start")
            })
            .await
            .expect("fake pi must start");

            let collector = tokio::spawn(async move {
                let mut events = Vec::new();
                while let Some(event) = stream.next().await {
                    events.push(event);
                }
                events
            });
            adapter
                .cancel(&session_id)
                .await
                .expect("cancel must be deliverable");
            let events = tokio::time::timeout(Duration::from_secs(5), collector)
                .await
                .expect("cancel must close the pi stream")
                .unwrap();
            assert_conforms_events(&events, Scenario::KilledMidRun);
        }

        #[tokio::test]
        async fn eof_without_result_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-pi");
            write_fake_pi(
                &executable,
                r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
printf '%s\n' '{"type":"message_start","message":{"role":"assistant","id":"m1"}}'
"#,
            );
            assert_conforms(
                &pi_adapter(executable),
                RuntimeRequest::new("hi"),
                Scenario::EofWithoutResult,
            )
            .await;
        }

        #[tokio::test]
        async fn output_after_fatal_is_not_forwarded() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-pi");
            write_fake_pi(
                &executable,
                r#"#!/bin/sh
printf '%s\n' '{"type":"session","id":"pi-sess-1"}'
printf '%s\n' '{"type":"message_start","message":{"role":"assistant"}}'
printf '%s\n' '{"type":"message_update","assistantMessageEvent":{"type":"error","error":{"errorMessage":"upstream refused"}}}'
printf '%s\n' '{"type":"message_end","message":{"role":"assistant","stopReason":"error","errorMessage":"must not be forwarded"}}'
"#,
            );
            assert_conforms(
                &pi_adapter(executable),
                RuntimeRequest::new("hi"),
                Scenario::OutputAfterFatal,
            )
            .await;
        }
    }

    /// Real pi CLI end-to-end evidence (ticket 16/10 L-tier). Skips when pi
    /// is missing; the model is machine-specific (defaults to the local
    /// catalogue entry, overridable via PI_REAL_TEST_MODEL).
    #[cfg(unix)]
    mod real_pi {
        use super::*;
        use alva_engine_runtime::{assert_conforms, RuntimeRequest, Scenario};
        use std::process::Command;
        use std::time::Duration;
        use tokio_stream::StreamExt;

        fn real_config(workspace: &tempfile::TempDir) -> PiAdapterConfig {
            let model = std::env::var("PI_REAL_TEST_MODEL")
                .unwrap_or_else(|_| "cc-switch-open-code-go/glm-5.2".into());
            PiAdapterConfig {
                pi_path: "pi".into(),
                model: Some(model),
                provider: None,
                default_cwd: Some(workspace.path().to_path_buf()),
                session_dir: workspace.path().join("sessions"),
                ..Default::default()
            }
        }

        #[tokio::test]
        async fn real_pi_completes_a_turn_conformant() {
            if Command::new("pi").arg("--version").output().is_err() {
                eprintln!("skipping real-pi: pi CLI is not installed");
                return;
            }
            let workspace = tempfile::tempdir().unwrap();
            let adapter = PiAdapter::new(real_config(&workspace));
            assert_conforms(
                &adapter,
                RuntimeRequest::new("Answer with just the number: 2+2").with_cwd(workspace.path()),
                Scenario::Normal,
            )
            .await;
        }

        #[tokio::test]
        async fn real_pi_cancel_stops_the_cli_within_the_grace_bound() {
            if Command::new("pi").arg("--version").output().is_err() {
                eprintln!("skipping real-pi: pi CLI is not installed");
                return;
            }
            let workspace = tempfile::tempdir().unwrap();
            let adapter = Arc::new(PiAdapter::new(real_config(&workspace)));
            let mut stream = adapter
                .execute(
                    RuntimeRequest::new(
                        "Reason slowly and at length, step by step, about the history of \
                         computing; then write a 400-word essay. Do not stop early.",
                    )
                    .with_cwd(workspace.path()),
                )
                .unwrap();
            let session_id = tokio::time::timeout(Duration::from_secs(20), async {
                while let Some(event) = stream.next().await {
                    if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                        return session_id;
                    }
                }
                panic!("real pi stream ended before SessionStarted")
            })
            .await
            .expect("real pi must start");

            let collector = tokio::spawn(async move {
                let mut events = Vec::new();
                while let Some(event) = stream.next().await {
                    events.push(event);
                }
                events
            });
            adapter
                .cancel(&session_id)
                .await
                .expect("cancel must be deliverable");

            // The stream must close quickly: real pi would otherwise keep
            // streaming for minutes. A terminal pair here is direct evidence
            // the pi child process was killed (ticket 13 acceptance).
            let events = tokio::time::timeout(Duration::from_secs(5), collector)
                .await
                .expect("cancel must kill the pi CLI and close the stream")
                .unwrap();
            assert!(
                matches!(events.last(), Some(RuntimeEvent::Completed { .. })),
                "cancel must end with Completed: {events:#?}"
            );
            assert!(
                events.iter().any(|event| matches!(
                    event,
                    RuntimeEvent::Error {
                        recoverable: false,
                        ..
                    }
                )),
                "cancel must surface Error{{fatal}}: {events:#?}"
            );
        }
    }
}
