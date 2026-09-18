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
    is_fatal_error, ConfigSurface, EngineDescriptor, EngineRuntime, InstallMetadata,
    PermissionDecision, PermissionPlanDecl, PermissionRelation, ProviderProtocol,
    RuntimeCapabilities, RuntimeError, RuntimeEvent, RuntimeRequest,
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
        Self::with_config(config)
    }

    /// Self-description (ticket 15 batch 3): what this engine can do and what
    /// it needs, declared here so products stop switch-ing on the "codex" name.
    pub fn descriptor() -> EngineDescriptor {
        EngineDescriptor {
            runtime: Arc::new(Self::with_config(CodexAdapterConfig::default())),
            inspector: None,
            permission_plans: vec![
                PermissionPlanDecl {
                    profile_id: "plan".into(),
                    relation: PermissionRelation::Equivalent,
                    summary: "codex exec plan approval policy".into(),
                },
                PermissionPlanDecl {
                    profile_id: "workspace_write".into(),
                    relation: PermissionRelation::Equivalent,
                    summary: "codex exec sandbox write access".into(),
                },
            ],
            config_surface: ConfigSurface {
                supports_max_turns: false,
                provider_protocol: ProviderProtocol::OpenAiResponses,
                supports_fast_mode: true,
                supports_custom_http_headers: false,
            },
            install_metadata: InstallMetadata {
                required_binary: Some("codex".into()),
                install_hint: Some("npm i -g openai-codex".into()),
            },
        }
    }

    fn with_config(config: CodexAdapterConfig) -> Self {
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
            // R81: `turn.completed` is the engine's own normal-terminal
            // evidence, but it maps to no outward event (its usage lands in
            // the mapper). "Seen the native terminal" and "forwarded a
            // Completed" are therefore tracked separately — the adapter
            // sends exactly one Completed when the stream ends, whatever the
            // outcome.
            let mut native_terminal_seen = false;
            let mut error_forwarded = false;
            // Stream contract R2: after a non-recoverable Error only Completed may
            // follow, so stop reading as soon as one is forwarded.
            let mut fatal_seen = false;

            loop {
                tokio::select! {
                    _ = kill_rx.recv() => {
                        if let Err(error) = process.kill().await {
                            warn!(%error, "codex kill failed");
                        }
                        // R94: `turn.completed` 已提交本轮成功；迟到的 cancel
                        // 只结束读取，不得把已提交的结果反转为"取消 fatal +
                        // 成功答案"的矛盾终态。
                        if !native_terminal_seen {
                            let _ = event_tx.send(RuntimeEvent::Error {
                                message: "Codex session cancelled".into(),
                                recoverable: false,
                            });
                            error_forwarded = true;
                        }
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
                                if matches!(event, CodexEvent::TurnCompleted { .. }) {
                                    native_terminal_seen = true;
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
                                            error_forwarded = true;
                                            break;
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
                                if escalated || fatal_seen {
                                    break;
                                }
                            }
                            Ok(None) => break,
                            Err(error) => {
                                // R94: 原生终态已提交成功后，尾部传输失败
                                //（断管 / 非法 UTF-8）不构成失败轮——要么
                                // 忽略提交后的噪声，要么清掉结果，二者不得
                                // 混合。这里选择前者：记录并停止读取，
                                // `turn.completed` 的结算（答案 + usage）保持
                                // 原样。终态之前的读取失败仍按 fatal 结算。
                                if !native_terminal_seen {
                                    let _ = event_tx.send(RuntimeEvent::Error {
                                        message: error.to_string(),
                                        recoverable: false,
                                    });
                                    error_forwarded = true;
                                } else {
                                    warn!(
                                        %error,
                                        "codex stream failed after the native terminal; committed result stands"
                                    );
                                }
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
            // R52 (ticket 16 contract): a stream that ends without the
            // engine's own terminal event is an outage, not a success — it
            // must surface a fatal Error before the synthesized Completed,
            // and never carry stale text as a result.
            if !native_terminal_seen && !error_forwarded && !escalated {
                let _ = event_tx.send(RuntimeEvent::Error {
                    message: "codex exited without a terminal event".into(),
                    recoverable: false,
                });
            }
            // Fatal path: never put a success answer on Completed (stream soft
            // rule). R94: 结算只有三种一致出口——引擎声明的失败
            // (`mapper.failed()`)、升级、或从未见到原生终态；原生终态之后
            // 的 adapter 侧噪声不参与结算，已提交的结果原样保留。
            let result = if mapper.failed() || escalated || !native_terminal_seen {
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
        assert!(validate_event_stream(&events).is_ok());
    }

    /// R94: `turn.completed` 提交本轮成功之后到达的传输失败（非法 UTF-8
    /// 尾行）不得把已提交的成功反转为携带答案的 fatal Error——终态必须
    /// 一致：无 fatal、答案保留、恰好一个 Completed。
    #[cfg(unix)]
    #[tokio::test]
    async fn transport_failure_after_native_terminal_keeps_committed_result() {
        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-codex");
        write_fake_codex(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-1"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"expected successful answer"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":5}}'
printf '\377\n'
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
            !events.iter().any(|event| matches!(
                event,
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            )),
            "a committed turn must not grow a fatal Error from trailing noise: {events:#?}"
        );
        assert!(
            matches!(
                events.last(),
                Some(RuntimeEvent::Completed {
                    result: Some(text),
                    ..
                }) if text == "expected successful answer"
            ),
            "the committed answer must survive the trailing transport failure: {events:#?}"
        );
        assert!(validate_event_stream(&events).is_ok());
    }

    /// R94: 原生终态之后到达的 cancel 只结束读取，不把已提交的成功改成
    /// "取消 fatal + 成功答案"的矛盾终态。fixture 让 `turn.completed` 先于
    /// 答案行，使 Message 事件被观察到时代际判定必然已完成。
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_after_native_terminal_keeps_committed_result() {
        use std::time::Duration;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-codex");
        write_fake_codex(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-1"}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":5}}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"expected successful answer"}}'
sleep 600
"#,
        );

        let adapter = Arc::new(CodexAdapter::new(CodexAdapterConfig {
            codex_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture.path().to_path_buf()),
            ..Default::default()
        }));
        let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
        // 读到答案 Message：`turn.completed` 已在它之前被顺序解析，原生
        // 终态必然已提交。
        tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = stream.next().await {
                if matches!(event, RuntimeEvent::Message { .. }) {
                    return;
                }
            }
            panic!("fixture codex stream ended before the answer Message")
        })
        .await
        .expect("fixture codex must reach the answer");

        let collector = tokio::spawn(async move {
            let mut events = Vec::new();
            while let Some(event) = stream.next().await {
                events.push(event);
            }
            events
        });
        adapter.cancel("codex-thread-1").await.expect("cancel must be deliverable");

        let events = tokio::time::timeout(Duration::from_secs(10), collector)
            .await
            .expect("cancel must close the codex stream")
            .unwrap();
        assert!(
            !events.iter().any(|event| matches!(
                event,
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            )),
            "cancel after the committed terminal must not emit a contradictory fatal: {events:#?}"
        );
        assert!(
            matches!(
                events.last(),
                Some(RuntimeEvent::Completed {
                    result: Some(text),
                    ..
                }) if text == "expected successful answer"
            ),
            "the committed result must survive a late cancel: {events:#?}"
        );
        assert!(validate_event_stream(&events).is_ok());
    }

    /// ticket 10: cancel must close the stream with Error{fatal} + Completed
    /// within a bounded time, even when the child process would otherwise run
    /// forever. A fake codex that sleeps gets killed by the cancel path.
    #[cfg(unix)]
    #[tokio::test]
    async fn cancel_emits_one_fatal_error_then_completed() {
        use std::time::Duration;

        let fixture = tempfile::tempdir().unwrap();
        let executable = fixture.path().join("fake-codex");
        write_fake_codex(
            &executable,
            r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-1"}'
sleep 600
"#,
        );

        let adapter = Arc::new(CodexAdapter::new(CodexAdapterConfig {
            codex_path: executable.to_string_lossy().into_owned(),
            default_cwd: Some(fixture.path().to_path_buf()),
            ..Default::default()
        }));
        let mut stream = adapter.execute(RuntimeRequest::new("hi")).unwrap();
        let session_id = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = stream.next().await {
                if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                    return session_id;
                }
            }
            panic!("fake codex stream ended before SessionStarted")
        })
        .await
        .expect("fake codex must start");

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
            .expect("cancel must close the codex stream")
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

    /// ticket 16: the five canned scenarios against the whole execute()
    /// boundary (SpawnFailure + OutputAfterFatal + KilledMidRun are already
    /// covered by spawn/`stops_reading_after_fatal_error`/`cancel_emits...`;
    /// Normal and EofWithoutResult are missing, added here via the harness).
    #[cfg(unix)]
    mod conformance {
        use super::*;
        use alva_engine_runtime::{
            assert_conforms, assert_conforms_events, collect_stream, RuntimeRequest, Scenario,
        };

        #[tokio::test]
        async fn normal_run_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-codex");
            write_fake_codex(
                &executable,
                r#"#!/bin/sh
printf '%s\n' '{"type":"thread.started","thread_id":"codex-thread-1"}'
printf '%s\n' '{"type":"item.completed","item":{"type":"agent_message","text":"two plus two is four"}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":3,"output_tokens":5}}'
"#,
            );
            let adapter = CodexAdapter::new(CodexAdapterConfig {
                codex_path: executable.to_string_lossy().into_owned(),
                default_cwd: Some(fixture.path().to_path_buf()),
                ..Default::default()
            });
            // R83: the fixture carries the engine's REAL terminal
            // (turn.completed) and the known answer, so the harness plus a
            // body/usage assertion pin that a normal run keeps both.
            let events = collect_stream(&adapter, RuntimeRequest::new("hi"))
                .await
                .expect("normal codex run must stream");
            assert_conforms_events(&events, Scenario::Normal);
            let completed = events.iter().find_map(|event| match event {
                RuntimeEvent::Completed { result, usage, .. } => {
                    Some((result.clone(), usage.clone()))
                }
                _ => None,
            });
            let (result, usage) = completed.expect("Normal must end with Completed");
            assert_eq!(result.as_deref(), Some("two plus two is four"));
            let usage = usage.expect("codex turn.completed carries usage");
            assert_eq!(usage.input_tokens, 3);
            assert_eq!(usage.output_tokens, 5);
        }

        #[tokio::test]
        async fn eof_without_result_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-codex");
            write_fake_codex(
                &executable,
                "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"thread.started\",\"thread_id\":\"codex-thread-1\"}' \nexit 0\n",
            );
            let adapter = CodexAdapter::new(CodexAdapterConfig {
                codex_path: executable.to_string_lossy().into_owned(),
                default_cwd: Some(fixture.path().to_path_buf()),
                ..Default::default()
            });
            assert_conforms(
                &adapter,
                RuntimeRequest::new("hi"),
                Scenario::EofWithoutResult,
            )
            .await;
        }

        // R52: the remaining three scenarios run against the same fake-codex
        // execute() boundary instead of reusing unrelated one-off assertions.
        #[tokio::test]
        async fn spawn_failure_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let adapter = CodexAdapter::new(CodexAdapterConfig {
                codex_path: fixture.path().join("no-such-codex").to_string_lossy().into_owned(),
                default_cwd: Some(fixture.path().to_path_buf()),
                ..Default::default()
            });
            assert_conforms(&adapter, RuntimeRequest::new("hi"), Scenario::SpawnFailure).await;
        }

        #[tokio::test]
        async fn killed_mid_run_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-codex");
            write_fake_codex(
                &executable,
                r#"#!/bin/sh
printf '%s
' '{"type":"thread.started","thread_id":"codex-thread-1"}'
kill -9 $$
"#,
            );
            let adapter = CodexAdapter::new(CodexAdapterConfig {
                codex_path: executable.to_string_lossy().into_owned(),
                default_cwd: Some(fixture.path().to_path_buf()),
                ..Default::default()
            });
            assert_conforms(&adapter, RuntimeRequest::new("hi"), Scenario::KilledMidRun).await;
        }

        #[tokio::test]
        async fn output_after_fatal_conforms() {
            let fixture = tempfile::tempdir().unwrap();
            let executable = fixture.path().join("fake-codex");
            write_fake_codex(
                &executable,
                r#"#!/bin/sh
printf '%s
' '{"type":"thread.started","thread_id":"codex-thread-1"}'
printf '%s
' '{"type":"turn.failed","error":{"message":"usage limit reached"}}'
printf '%s
' '{"type":"item.completed","item":{"type":"agent_message","text":"must not be forwarded"}}'
"#,
            );
            let adapter = CodexAdapter::new(CodexAdapterConfig {
                codex_path: executable.to_string_lossy().into_owned(),
                default_cwd: Some(fixture.path().to_path_buf()),
                ..Default::default()
            });
            assert_conforms(&adapter, RuntimeRequest::new("hi"), Scenario::OutputAfterFatal).await;
        }
    }
}
