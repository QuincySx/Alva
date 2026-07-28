// INPUT:  async_trait, futures_core::Stream, tokio::sync, alva_engine_runtime::*, crate::{bridge, config, mapping, process, protocol}
// OUTPUT: pub struct ClaudeAdapter
// POS:    EngineRuntime implementation that spawns a Node.js bridge process to communicate with the Claude Agent SDK.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, oneshot, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};

use crate::bridge::ensure_bridge_script;
use crate::config::{BridgeConfig, ClaudeAdapterConfig};
use crate::mapping::EventMapper;
use crate::process::{BridgeProcess, BridgeSpawnConfig};
use crate::protocol::{BridgeOutbound, BridgePermissionDecision};

type ControlCommand = (
    BridgeOutbound,
    oneshot::Sender<Result<(), RuntimeError>>,
);
type SessionSender = mpsc::UnboundedSender<ControlCommand>;
const CONTROL_ACK_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// Claude Agent SDK bridge adapter.
///
/// Implements `EngineRuntime` by spawning a Node.js bridge process that
/// communicates with the Claude Agent SDK via stdin/stdout JSON-line protocol.
pub struct ClaudeAdapter {
    config: ClaudeAdapterConfig,
    /// Active sessions: session_id -> sender for control messages.
    sessions: Arc<Mutex<HashMap<String, SessionSender>>>,
}

impl ClaudeAdapter {
    pub fn new(config: ClaudeAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    fn build_bridge_config(&self, request: RuntimeRequest) -> BridgeConfig {
        // Build env with cloud provider flags
        let mut env = self.config.env.clone();
        if self.config.use_bedrock {
            env.insert("CLAUDE_CODE_USE_BEDROCK".into(), "1".into());
        }
        if self.config.use_vertex {
            env.insert("CLAUDE_CODE_USE_VERTEX".into(), "1".into());
        }
        if self.config.use_azure {
            env.insert("CLAUDE_CODE_USE_FOUNDRY".into(), "1".into());
        }

        BridgeConfig {
            prompt: request.prompt,
            cwd: request
                .working_directory
                .map(|p| p.to_string_lossy().into_owned()),
            system_prompt: request.system_prompt,
            streaming: request.options.streaming,
            max_turns: request.options.max_turns,
            resume_session: request.resume_session,
            api_key: self.config.api_key.clone(),
            api_base_url: self.config.api_base_url.clone(),
            model: self.config.model.clone(),
            effort: self.config.effort.clone(),
            max_budget_usd: self.config.max_budget_usd,
            permission_mode: self.config.permission_mode.as_sdk_str().to_string(),
            allowed_tools: self.config.allowed_tools.clone(),
            disallowed_tools: self.config.disallowed_tools.clone(),
            sandbox: self.config.sandbox.clone(),
            mcp_servers: self.config.mcp_servers.clone(),
            agents: self.config.agents.clone(),
            setting_sources: self.config.setting_sources.clone(),
            persist_session: self.config.persist_session,
            sdk_package_path: self.config.sdk_package_path.clone(),
            env,
        }
    }

    async fn send_control(
        &self,
        session_id: &str,
        message: BridgeOutbound,
    ) -> Result<(), RuntimeError> {
        let sender = self
            .sessions
            .lock()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;
        let (ack_tx, ack_rx) = oneshot::channel();
        if sender.send((message, ack_tx)).is_err() {
            remove_session_if_current(&self.sessions, session_id, &sender).await;
            return Err(RuntimeError::ProcessError("Session channel closed".into()));
        }

        match await_control_ack(ack_rx, CONTROL_ACK_TIMEOUT).await {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                remove_session_if_current(&self.sessions, session_id, &sender).await;
                Err(error)
            }
            Err(error) => {
                remove_session_if_current(&self.sessions, session_id, &sender).await;
                Err(error)
            }
        }
    }
}

async fn forward_controls(
    control: crate::process::BridgeControl,
    mut controls: mpsc::UnboundedReceiver<ControlCommand>,
    control_failures: mpsc::UnboundedSender<()>,
) {
    while let Some((message, acknowledgement)) = controls.recv().await {
        match control.send(&message).await {
            Ok(()) => {
                let _ = acknowledgement.send(Ok(()));
            }
            Err(error) => {
                let _ = acknowledgement.send(Err(error));
                let _ = control_failures.send(());
                return;
            }
        }
    }
}

async fn await_control_ack(
    acknowledgement: oneshot::Receiver<Result<(), RuntimeError>>,
    timeout_duration: std::time::Duration,
) -> Result<Result<(), RuntimeError>, RuntimeError> {
    match tokio::time::timeout(timeout_duration, acknowledgement).await {
        Ok(Ok(result)) => Ok(result),
        Ok(Err(_)) => Err(RuntimeError::ProcessError(
            "Bridge control command was not acknowledged".into(),
        )),
        Err(_) => Err(RuntimeError::ProcessError(
            "Bridge control command acknowledgement timed out".into(),
        )),
    }
}

async fn remove_session_if_current(
    sessions: &Mutex<HashMap<String, SessionSender>>,
    session_id: &str,
    expected_sender: &SessionSender,
) {
    let mut sessions = sessions.lock().await;
    if sessions
        .get(session_id)
        .is_some_and(|sender| sender.same_channel(expected_sender))
    {
        sessions.remove(session_id);
    }
}

/// Install a session route without allowing a newer child to steal an active
/// native session ID. A closed sender is stale and may be replaced.
async fn register_session(
    sessions: &Mutex<HashMap<String, SessionSender>>,
    session_id: &str,
    sender: SessionSender,
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

async fn finish_session(
    mut process: BridgeProcess,
    control_task: tokio::task::JoinHandle<()>,
    sessions: &Mutex<HashMap<String, SessionSender>>,
    session_id: Option<String>,
    expected_sender: SessionSender,
) {
    control_task.abort();
    if let Some(session_id) = session_id {
        remove_session_if_current(sessions, &session_id, &expected_sender).await;
    }
    // A graceful bridge shutdown can wait up to five seconds. The caller
    // enqueues its terminal event after this synchronous route detachment, so
    // perform the slow child I/O in the background.
    tokio::spawn(async move {
        let _ = process.shutdown().await;
    });
}

async fn kill_session(
    mut process: BridgeProcess,
    control_task: tokio::task::JoinHandle<()>,
    sessions: &Mutex<HashMap<String, SessionSender>>,
    session_id: Option<String>,
    expected_sender: SessionSender,
) {
    control_task.abort();
    if let Some(session_id) = session_id {
        remove_session_if_current(sessions, &session_id, &expected_sender).await;
    }
    tokio::spawn(async move {
        let _ = process.kill().await;
    });
}

#[async_trait]
impl EngineRuntime for ClaudeAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        // Write bridge script (sync I/O — acceptable here since it's a one-time idempotent write)
        let script_path = ensure_bridge_script()?;
        let bridge_config = self.build_bridge_config(request);

        let config_json = serde_json::to_string(&bridge_config)?;
        let node_path = self
            .config
            .node_path
            .clone()
            .unwrap_or_else(|| "node".into());

        let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let (control_tx, control_rx) = mpsc::unbounded_channel::<ControlCommand>();
        let (control_failure_tx, mut control_failures) = mpsc::unbounded_channel::<()>();
        let sessions = self.sessions.clone();

        tokio::spawn(async move {
            // Spawn bridge process
            let spawn_config = BridgeSpawnConfig {
                node_path,
                script_path: script_path.to_string_lossy().into_owned(),
                config_json,
                env: vec![],
            };

            let mut process = match BridgeProcess::spawn(spawn_config).await {
                Ok(p) => p,
                Err(e) => {
                    let _ = event_tx.send(RuntimeEvent::Error {
                        message: e.to_string(),
                        recoverable: false,
                    });
                    let _ = event_tx.send(RuntimeEvent::Completed {
                        session_id: String::new(),
                        result: None,
                        usage: None,
                    });
                    return;
                }
            };

            // Forward stdin controls in a dedicated task. This is deliberately
            // independent of the stdout read loop: the bridge may pause stdout
            // while canUseTool waits for a permission decision.
            let control = process.control();
            let control_task = tokio::spawn(async move {
                forward_controls(control, control_rx, control_failure_tx).await;
            });
            let mut active_session_id = None;
            let mut mapper = EventMapper::new();

            // Read stdout while also observing the consumer lifecycle. A
            // receiver dropped before the first bridge event must still tear
            // down the child rather than waiting forever on stdout.
            loop {
                tokio::select! {
                    _ = event_tx.closed() => {
                        kill_session(
                            process,
                            control_task,
                            &sessions,
                            active_session_id,
                            control_tx.clone(),
                        )
                        .await;
                        return;
                    }
                    Some(()) = control_failures.recv() => {
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Bridge control write failed".into(),
                            recoverable: false,
                        });
                        kill_session(
                            process,
                            control_task,
                            &sessions,
                            active_session_id,
                            control_tx.clone(),
                        )
                        .await;
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    received = process.recv() => match received {
                    Ok(Some(msg)) => {
                        let is_done = matches!(&msg, crate::protocol::BridgeMessage::Done);
                        let events = mapper.map(msg);

                        // Register session once we know the ID. A duplicate
                        // active native ID is fail-closed: keep the old route
                        // intact and terminate only this new bridge child.
                        for event in &events {
                            if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                                if let Err(error) = register_session(
                                    &sessions,
                                    session_id,
                                    control_tx.clone(),
                                )
                                .await
                                {
                                    let _ = event_tx.send(RuntimeEvent::Error {
                                        message: error.to_string(),
                                        recoverable: false,
                                    });
                                    kill_session(
                                        process,
                                        control_task,
                                        &sessions,
                                        None,
                                        control_tx.clone(),
                                    )
                                    .await;
                                    let _ = event_tx.send(RuntimeEvent::Completed {
                                        session_id: session_id.clone(),
                                        result: None,
                                        usage: None,
                                    });
                                    return;
                                }
                                active_session_id = Some(session_id.clone());
                            }
                        }

                        for event in events {
                            let is_completed = matches!(&event, RuntimeEvent::Completed { .. });
                            if is_completed {
                                finish_session(
                                    process,
                                    control_task,
                                    &sessions,
                                    active_session_id,
                                    control_tx.clone(),
                                )
                                .await;
                                let _ = event_tx.send(event);
                                return;
                            }
                            if event_tx.send(event).is_err() {
                                kill_session(
                                    process,
                                    control_task,
                                    &sessions,
                                    active_session_id,
                                    control_tx.clone(),
                                )
                                .await;
                                return;
                            }
                        }

                        if is_done {
                            // A bridge without Result is never success: surface a terminal
                            // error before completing so callers cannot treat it as done.
                            let _ = event_tx.send(RuntimeEvent::Error {
                                message: "Bridge completed without a result".into(),
                                recoverable: false,
                            });
                            finish_session(
                                process,
                                control_task,
                                &sessions,
                                active_session_id,
                                control_tx.clone(),
                            )
                            .await;
                            let _ = event_tx.send(RuntimeEvent::Completed {
                                session_id: mapper.session_id().to_string(),
                                result: None,
                                usage: None,
                            });
                            return;
                        }
                    }
                    Ok(None) => {
                        // stdout closed — process exited
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Bridge process exited unexpectedly".into(),
                            recoverable: false,
                        });
                        finish_session(
                            process,
                            control_task,
                            &sessions,
                            active_session_id,
                            control_tx.clone(),
                        )
                        .await;
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    Err(e) => {
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: e.to_string(),
                            recoverable: false,
                        });
                        kill_session(
                            process,
                            control_task,
                            &sessions,
                            active_session_id,
                            control_tx.clone(),
                        )
                        .await;
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    },
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        self.send_control(session_id, BridgeOutbound::Cancel).await
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let bridge_decision = match decision {
            PermissionDecision::Allow { updated_input } => {
                BridgePermissionDecision::Allow { updated_input }
            }
            PermissionDecision::Deny { message } => BridgePermissionDecision::Deny { message },
        };

        self.send_control(
            session_id,
            BridgeOutbound::PermissionResponse {
                request_id: request_id.into(),
                decision: bridge_decision,
            },
        )
        .await
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: false,
            permission_callback: true,
            resume: true,
            cancel: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::path::PathBuf;
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::Duration;

    use tokio_stream::StreamExt;

    fn make_config() -> ClaudeAdapterConfig {
        ClaudeAdapterConfig {
            model: Some("claude-sonnet-4-6".into()),
            ..Default::default()
        }
    }

    fn node_environment_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct EnvironmentRestore {
        key: &'static str,
        previous: Option<OsString>,
    }

    impl EnvironmentRestore {
        fn replace(key: &'static str, value: OsString) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvironmentRestore {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.take() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn build_bridge_config_maps_runtime_request_fields() {
        let adapter = ClaudeAdapter::new(make_config());
        let mut request = RuntimeRequest::new("Say hello");
        request.resume_session = Some("session-123".into());
        request.system_prompt = Some("Be terse".into());
        request.working_directory = Some(PathBuf::from("/tmp/alva-claude"));
        request.options.streaming = true;
        request.options.max_turns = Some(3);

        let bridge = adapter.build_bridge_config(request);

        assert_eq!(bridge.prompt, "Say hello");
        assert_eq!(bridge.resume_session.as_deref(), Some("session-123"));
        assert_eq!(bridge.system_prompt.as_deref(), Some("Be terse"));
        assert_eq!(bridge.cwd.as_deref(), Some("/tmp/alva-claude"));
        assert!(bridge.streaming);
        assert_eq!(bridge.max_turns, Some(3));
    }

    #[test]
    fn build_bridge_config_includes_cloud_provider_flags() {
        let config = ClaudeAdapterConfig {
            use_bedrock: true,
            use_vertex: true,
            use_azure: true,
            ..make_config()
        };
        let adapter = ClaudeAdapter::new(config);

        let bridge = adapter.build_bridge_config(RuntimeRequest::new("hello"));

        assert_eq!(
            bridge
                .env
                .get("CLAUDE_CODE_USE_BEDROCK")
                .map(String::as_str),
            Some("1")
        );
        assert_eq!(
            bridge.env.get("CLAUDE_CODE_USE_VERTEX").map(String::as_str),
            Some("1")
        );
        assert_eq!(
            bridge
                .env
                .get("CLAUDE_CODE_USE_FOUNDRY")
                .map(String::as_str),
            Some("1")
        );
    }

    #[test]
    fn capabilities_report_resume_support() {
        let adapter = ClaudeAdapter::new(make_config());
        assert!(adapter.capabilities().resume);
    }

    #[tokio::test]
    async fn stale_cleanup_keeps_a_newer_sender_for_the_same_session() {
        let sessions = Mutex::new(HashMap::new());
        let (old_sender, _old_receiver) = mpsc::unbounded_channel::<ControlCommand>();
        let (new_sender, _new_receiver) = mpsc::unbounded_channel::<ControlCommand>();
        sessions
            .lock()
            .await
            .insert("reused-session".into(), old_sender.clone());
        sessions
            .lock()
            .await
            .insert("reused-session".into(), new_sender.clone());

        remove_session_if_current(&sessions, "reused-session", &old_sender).await;

        let current = sessions
            .lock()
            .await
            .get("reused-session")
            .cloned()
            .expect("old cleanup must not remove newer run");
        assert!(current.same_channel(&new_sender));
    }

    #[tokio::test]
    async fn duplicate_active_session_id_keeps_existing_route_and_rejects_new_child() {
        let sessions = Mutex::new(HashMap::new());
        let (active_sender, active_receiver) = mpsc::unbounded_channel::<ControlCommand>();
        let (duplicate_sender, _duplicate_receiver) =
            mpsc::unbounded_channel::<ControlCommand>();

        register_session(&sessions, "native-session", active_sender.clone())
            .await
            .unwrap();
        register_session(&sessions, "native-session", active_sender.clone())
            .await
            .expect("re-registering the same native route must be idempotent");
        assert!(matches!(
            register_session(&sessions, "native-session", duplicate_sender).await,
            Err(RuntimeError::ProcessError(message)) if message == "Duplicate active Claude session ID"
        ));
        let current = sessions
            .lock()
            .await
            .get("native-session")
            .cloned()
            .expect("existing session route must remain installed");
        assert!(current.same_channel(&active_sender));

        drop(active_receiver);
        let (replacement_sender, _replacement_receiver) =
            mpsc::unbounded_channel::<ControlCommand>();
        register_session(&sessions, "native-session", replacement_sender.clone())
            .await
            .expect("closed stale sender may be replaced");
        assert!(
            sessions
                .lock()
                .await
                .get("native-session")
                .is_some_and(|current| current.same_channel(&replacement_sender))
        );
    }

    #[tokio::test]
    async fn control_ack_timeout_is_bounded_and_reports_failure() {
        let (_sender, receiver) = oneshot::channel::<Result<(), RuntimeError>>();
        assert!(matches!(
            await_control_ack(receiver, Duration::from_millis(10)).await,
            Err(RuntimeError::ProcessError(message))
                if message == "Bridge control command acknowledgement timed out"
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn cwd_sdk_resolves_and_permission_response_is_not_blocked_by_stdout_read() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let package = workspace
            .path()
            .join("node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"
export function query({ options }) {
  return (async function* () {
    yield { type: "system", subtype: "init", session_id: "fake-session", model: "fake", tools: [] };
    await options.canUseTool("FakeTool", { secret: "must-not-log" }, { signal: options.abortController.signal });
    yield { type: "result", subtype: "success", session_id: "fake-session", result: "done", duration_ms: 1, num_turns: 1, usage: {} };
  })();
}
"#,
        )
        .unwrap();

        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig::default());
        let request = RuntimeRequest::new("test").with_cwd(workspace.path());
        let mut events = adapter.execute(request).unwrap();
        let completed = tokio::time::timeout(Duration::from_secs(5), async {
            let mut session_id = None;
            while let Some(event) = events.next().await {
                match event {
                    RuntimeEvent::SessionStarted { session_id: id, .. } => session_id = Some(id),
                    RuntimeEvent::PermissionRequest { request_id, .. } => {
                        adapter
                            .respond_permission(
                                session_id.as_deref().expect("session must start first"),
                                &request_id,
                                PermissionDecision::Deny {
                                    message: "test denial".into(),
                                },
                            )
                            .await
                            .expect("permission response must be written immediately");
                    }
                    RuntimeEvent::Completed { .. } => return session_id,
                    RuntimeEvent::Error { message, .. } => panic!("bridge failed: {message}"),
                    _ => {}
                }
            }
            panic!("bridge stream ended before Completed")
        })
        .await
        .expect("permission response must not wait for the 60 second bridge timeout");

        let session_id = completed.expect("expected fake session ID");
        assert!(matches!(
            adapter.cancel(&session_id).await,
            Err(RuntimeError::SessionNotFound(id)) if id == session_id
        ));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn explicit_sdk_override_fails_closed_without_leaking_its_specifier() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let package = workspace
            .path()
            .join("node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"export function query() { throw new Error("ambient SDK must not load"); }"#,
        )
        .unwrap();

        let secret_specifier = "missing-sdk-private-token-123";
        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig {
            sdk_package_path: Some(secret_specifier.into()),
            ..Default::default()
        });
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(workspace.path()))
            .unwrap();

        let error = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                match event {
                    RuntimeEvent::Error { message, .. } => return message,
                    RuntimeEvent::SessionStarted { .. } => {
                        panic!("an explicit override failure must not fall back to cwd SDK")
                    }
                    _ => {}
                }
            }
            panic!("bridge stream ended before sanitized SDK error")
        })
        .await
        .expect("explicit SDK lookup must fail promptly");

        assert_eq!(
            error,
            "Claude Agent SDK load failed at explicit SDK override: resolve failed."
        );
        assert!(!error.contains(secret_specifier));
        assert!(!error.contains(workspace.path().to_string_lossy().as_ref()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn relative_explicit_sdk_path_resolves_against_request_cwd() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let sdk_dir = workspace.path().join("fake-sdk");
        std::fs::create_dir_all(&sdk_dir).unwrap();
        std::fs::write(
            sdk_dir.join("index.js"),
            r#"
export function query() {
  return (async function* () {
    yield { type: "system", subtype: "init", session_id: "relative-sdk-session", model: "fake", tools: [] };
    yield { type: "result", subtype: "success", session_id: "relative-sdk-session", result: "done", duration_ms: 1, num_turns: 1, usage: {} };
  })();
}
"#,
        )
        .unwrap();

        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig {
            sdk_package_path: Some("./fake-sdk/index.js".into()),
            ..Default::default()
        });
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(workspace.path()))
            .unwrap();
        let session_id = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                match event {
                    RuntimeEvent::SessionStarted { session_id, .. } => return session_id,
                    RuntimeEvent::Error { message, .. } => panic!("bridge failed: {message}"),
                    _ => {}
                }
            }
            panic!("bridge stream ended before relative SDK started")
        })
        .await
        .expect("relative explicit SDK path must resolve promptly");

        assert_eq!(session_id, "relative-sdk-session");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn resolved_ambient_sdk_import_failure_does_not_fall_back() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let package = workspace
            .path()
            .join("node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"throw new Error("private import failure must not escape");"#,
        )
        .unwrap();

        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig::default());
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(workspace.path()))
            .unwrap();
        let error = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                if let RuntimeEvent::Error { message, .. } = event {
                    return message;
                }
            }
            panic!("bridge stream ended before import error")
        })
        .await
        .expect("resolved SDK import failure must fail promptly");

        assert_eq!(
            error,
            "Claude Agent SDK load failed at request cwd node_modules: import failed."
        );
        assert!(!error.contains("private import failure"));
        assert!(!error.contains(workspace.path().to_string_lossy().as_ref()));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn bridge_done_without_result_emits_error_then_completed() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let package = workspace
            .path()
            .join("node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"export function query() { return (async function* () {})(); }"#,
        )
        .unwrap();

        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig::default());
        let events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(workspace.path()))
            .unwrap();
        let observed = tokio::time::timeout(Duration::from_secs(5), async {
            events.take(2).collect::<Vec<_>>().await
        })
        .await
        .expect("done-without-result must terminate promptly");

        assert!(matches!(
            observed.as_slice(),
            [
                RuntimeEvent::Error {
                    message,
                    recoverable: false,
                },
                RuntimeEvent::Completed { .. },
            ] if message == "Bridge completed without a result"
        ));
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn global_npm_root_sdk_resolution_loads_the_package_child() {
        use std::os::unix::fs::PermissionsExt;

        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let fixture = tempfile::tempdir().unwrap();
        let global_root = fixture.path().join("global-node-modules");
        let package = global_root.join("@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"
export function query() {
  return (async function* () {
    yield { type: "system", subtype: "init", session_id: "global-session", model: "fake", tools: [] };
    yield { type: "result", subtype: "success", session_id: "global-session", result: "global done", duration_ms: 1, num_turns: 1, usage: {} };
  })();
}
"#,
        )
        .unwrap();

        let fake_bin = fixture.path().join("bin");
        std::fs::create_dir_all(&fake_bin).unwrap();
        let fake_npm = fake_bin.join("npm");
        std::fs::write(
            &fake_npm,
            format!(
                "#!/bin/sh\nif [ \"$1\" = \"root\" ] && [ \"$2\" = \"-g\" ]; then\n  printf '%s\\n' '{}'\nfi\n",
                global_root.display()
            ),
        )
        .unwrap();
        let mut permissions = std::fs::metadata(&fake_npm).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&fake_npm, permissions).unwrap();

        let mut path = OsString::from(fake_bin.as_os_str());
        if let Some(existing) = std::env::var_os("PATH") {
            path.push(":");
            path.push(existing);
        }
        let _restore_path = EnvironmentRestore::replace("PATH", path);

        let request_workspace = fixture.path().join("request-workspace");
        std::fs::create_dir_all(&request_workspace).unwrap();
        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig::default());
        let mut events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(&request_workspace))
            .unwrap();

        let session_id = tokio::time::timeout(Duration::from_secs(5), async {
            while let Some(event) = events.next().await {
                match event {
                    RuntimeEvent::SessionStarted { session_id, .. } => return session_id,
                    RuntimeEvent::Error { message, .. } => panic!("bridge failed: {message}"),
                    _ => {}
                }
            }
            panic!("bridge stream ended before SessionStarted")
        })
        .await
        .expect("global npm root resolution must finish quickly");

        assert_eq!(session_id, "global-session");
    }

    #[cfg(unix)]
    #[tokio::test(flavor = "current_thread")]
    async fn dropped_consumer_before_first_event_kills_bridge_promptly() {
        let _environment_lock = node_environment_lock().lock().await;
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipping Node bridge integration test: node is not installed");
            return;
        }

        let workspace = tempfile::tempdir().unwrap();
        let probe = workspace.path().join("bridge-pid");
        let package = workspace
            .path()
            .join("node_modules/@anthropic-ai/claude-agent-sdk");
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(
            package.join("package.json"),
            r#"{"name":"@anthropic-ai/claude-agent-sdk","type":"module","exports":"./index.js"}"#,
        )
        .unwrap();
        std::fs::write(
            package.join("index.js"),
            r#"
import fs from "fs";
export function query({ options }) {
  fs.writeFileSync(options.env.ALVA_TEST_BRIDGE_PID, String(process.pid));
  return (async function* () {
    setInterval(() => {}, 1_000);
    await new Promise(() => {});
  })();
}
"#,
        )
        .unwrap();

        let adapter = ClaudeAdapter::new(ClaudeAdapterConfig {
            env: HashMap::from([(
                "ALVA_TEST_BRIDGE_PID".into(),
                probe.to_string_lossy().into_owned(),
            )]),
            ..Default::default()
        });
        let events = adapter
            .execute(RuntimeRequest::new("test").with_cwd(workspace.path()))
            .unwrap();

        tokio::time::timeout(Duration::from_secs(3), async {
            while !probe.exists() {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("fake bridge must begin before any stdout event");
        let pid: u32 = std::fs::read_to_string(&probe).unwrap().parse().unwrap();

        drop(events);
        tokio::time::timeout(Duration::from_secs(2), async {
            while Command::new("kill")
                .args(["-0", &pid.to_string()])
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|status| status.success())
            {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("dropping the receiver must kill the waiting bridge immediately");
    }
}
