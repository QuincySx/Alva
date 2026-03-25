// INPUT:  std::path, std::sync, tokio::io, tokio::process, tokio::sync, crate::agent::agent_client::protocol::message, crate::agent::agent_client::AcpError, serde_json
// OUTPUT: ProcessState, AcpProcessHandle
// POS:    Manages a single ACP child process: stdin/stdout I/O, state tracking, and graceful shutdown.
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Child;
use tokio::sync::{mpsc, Mutex};

use crate::agent::agent_client::{
    protocol::message::{AcpInboundMessage, AcpOutboundMessage},
    AcpError,
};

use super::discovery::AgentCliCommand;

/// Child process lifecycle state
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    /// Running
    Running,
    /// Normal exit (exit code = 0)
    Exited,
    /// Abnormal exit (exit code != 0 or signal)
    Crashed { exit_code: Option<i32> },
    /// Restarting (automatic retry after crash)
    Restarting { attempt: u32 },
}

/// Handle for a single ACP child process
pub struct AcpProcessHandle {
    /// Process ID
    pub pid: u32,
    /// Agent type identifier
    pub agent_kind: String,
    /// Working directory
    pub workspace: PathBuf,
    /// Child process state
    state: Arc<Mutex<ProcessState>>,
    /// Channel to send messages to child process (stdin writer wrapper)
    stdin_tx: mpsc::Sender<AcpOutboundMessage>,
    /// Inbound sender (kept for reference; actual receiving done externally)
    #[allow(dead_code)]
    inbound_tx: mpsc::Sender<AcpInboundMessage>,
}

impl AcpProcessHandle {
    /// Spawn child process, write bootstrap, start reader/writer tasks
    pub async fn spawn(
        agent_cmd: &AgentCliCommand,
        bootstrap: crate::agent::agent_client::protocol::bootstrap::BootstrapPayload,
        inbound_tx: mpsc::Sender<AcpInboundMessage>,
    ) -> Result<Self, AcpError> {
        use super::orphan::SROW_PARENT_PID_ENV;

        let mut cmd = tokio::process::Command::new(&agent_cmd.executable);
        cmd.args(&agent_cmd.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Inject parent PID (orphan detection)
            .env(SROW_PARENT_PID_ENV, super::orphan::parent_pid_env_value())
            // Inject workspace
            .env("SROW_WORKSPACE", &bootstrap.workspace)
            // Disable color escape sequences (ensure stdout is pure JSON)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");

        let mut child: Child = cmd.spawn().map_err(|e| AcpError::SpawnFailed {
            agent: agent_cmd.executable.display().to_string(),
            reason: e.to_string(),
        })?;

        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Write bootstrap (one JSON line)
        let bootstrap_json =
            serde_json::to_string(&bootstrap).map_err(|e| AcpError::Serialization(e.to_string()))?;

        let mut writer = BufWriter::new(stdin);
        writer
            .write_all(bootstrap_json.as_bytes())
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;

        // Wrap stdin write into a channel
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<AcpOutboundMessage>(64);
        let state = Arc::new(Mutex::new(ProcessState::Running));
        let state_clone = state.clone();

        // Task 1: stdin writer
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                let line = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("acp serialize outbound: {e}");
                        continue;
                    }
                };
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        // Task 2: stdout reader (parse AcpInboundMessage line by line)
        let inbound_tx_clone = inbound_tx.clone();
        let state_for_reader = state.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<AcpInboundMessage>(&line) {
                    Ok(msg) => {
                        let _ = inbound_tx_clone.send(msg).await;
                    }
                    Err(e) => {
                        tracing::warn!("acp parse inbound: {e}, raw: {line}");
                    }
                }
            }
            // stdout closed = process exited
            *state_for_reader.lock().await = ProcessState::Exited;
        });

        // Task 3: stderr logger
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[acp-stderr][pid={pid}] {line}");
            }
        });

        // Task 4: process wait (detect crash)
        tokio::spawn(async move {
            let status = child.wait().await;
            let exit_code = status.ok().and_then(|s| s.code());
            let new_state = match exit_code {
                Some(0) => ProcessState::Exited,
                code => ProcessState::Crashed { exit_code: code },
            };
            *state_clone.lock().await = new_state;
        });

        Ok(Self {
            pid,
            agent_kind: format!("{:?}", agent_cmd.kind),
            workspace: PathBuf::from(&bootstrap.workspace),
            state,
            stdin_tx,
            inbound_tx,
        })
    }

    /// Get current process state
    pub async fn state(&self) -> ProcessState {
        self.state.lock().await.clone()
    }

    /// Send message to child process (write to stdin)
    pub async fn send(&self, msg: AcpOutboundMessage) -> Result<(), AcpError> {
        self.stdin_tx
            .send(msg)
            .await
            .map_err(|_| AcpError::ProcessDead { pid: self.pid })
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        let _ = self.send(AcpOutboundMessage::Shutdown).await;
    }
}
