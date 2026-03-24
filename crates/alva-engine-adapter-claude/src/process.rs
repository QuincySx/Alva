use std::process::Stdio;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use alva_engine_runtime::RuntimeError;

use crate::protocol::{BridgeMessage, BridgeOutbound};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

/// Configuration for spawning the bridge process.
pub(crate) struct BridgeSpawnConfig {
    pub node_path: String,
    pub script_path: String,
    pub config_json: String,
    pub env: Vec<(String, String)>,
}

/// Manages the Node.js bridge child process lifecycle.
pub(crate) struct BridgeProcess {
    child: Child,
    stdin: BufWriter<ChildStdin>,
    stdout_lines: Lines<BufReader<ChildStdout>>,
}

impl BridgeProcess {
    /// Spawn the Node.js bridge process.
    pub async fn spawn(config: BridgeSpawnConfig) -> Result<Self, RuntimeError> {
        let mut cmd = Command::new(&config.node_path);
        cmd.arg(&config.script_path)
            .arg(&config.config_json)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);

        for (key, val) in &config.env {
            cmd.env(key, val);
        }

        let mut child = cmd.spawn().map_err(|e| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Node.js bridge ({}): {}",
                config.node_path, e
            ))
        })?;

        let stdin = child.stdin.take().ok_or_else(|| {
            RuntimeError::ProcessError("Failed to capture stdin".into())
        })?;
        let stdout = child.stdout.take().ok_or_else(|| {
            RuntimeError::ProcessError("Failed to capture stdout".into())
        })?;
        let stderr = child.stderr.take();

        // Spawn stderr monitoring task
        if let Some(stderr) = stderr {
            tokio::spawn(async move {
                let reader = BufReader::new(stderr);
                let mut lines = reader.lines();
                while let Ok(Some(line)) = lines.next_line().await {
                    if is_fatal_stderr(&line) {
                        warn!(target: "claude_bridge", "Fatal stderr: {}", line);
                    } else {
                        debug!(target: "claude_bridge", "stderr: {}", line);
                    }
                }
            });
        }

        Ok(Self {
            child,
            stdin: BufWriter::new(stdin),
            stdout_lines: BufReader::new(stdout).lines(),
        })
    }

    /// Send a JSON-line control message to the bridge via stdin.
    pub async fn send(&mut self, msg: &BridgeOutbound) -> Result<(), RuntimeError> {
        let json = serde_json::to_string(msg)?;
        self.stdin.write_all(json.as_bytes()).await?;
        self.stdin.write_all(b"\n").await?;
        self.stdin.flush().await?;
        Ok(())
    }

    /// Read the next JSON-line message from stdout.
    /// Returns None when stdout is closed (process exited).
    pub async fn recv(&mut self) -> Result<Option<BridgeMessage>, RuntimeError> {
        match self.stdout_lines.next_line().await? {
            Some(line) => {
                let msg = serde_json::from_str(&line).map_err(|e| {
                    RuntimeError::ProtocolError(format!(
                        "Invalid JSON from bridge: {e} — line: {line}"
                    ))
                })?;
                Ok(Some(msg))
            }
            None => Ok(None),
        }
    }

    /// Graceful shutdown: send shutdown message, wait, then kill.
    pub async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        let _ = self.send(&BridgeOutbound::Shutdown).await;
        match timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => self.kill().await,
        }
    }

    /// Force-kill the process.
    pub async fn kill(&mut self) -> Result<(), RuntimeError> {
        self.child.kill().await.map_err(|e| {
            RuntimeError::ProcessError(format!("Failed to kill bridge process: {e}"))
        })
    }
}

fn is_fatal_stderr(line: &str) -> bool {
    let lower = line.to_lowercase();
    let patterns = [
        "authentication_error",
        "authentication error",
        "invalid_api_key",
        "invalid api key",
        "unauthorized",
        "rate_limit",
        "rate limit",
        "quota_exceeded",
        "billing",
        "overloaded",
        "connection_refused",
        "econnrefused",
    ];
    patterns.iter().any(|p| lower.contains(p))
}
