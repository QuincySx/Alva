// INPUT:  std::process::Stdio, tokio async I/O/process/sync/time, tracing, RuntimeError, Bridge protocol
// OUTPUT: BridgeSpawnConfig, BridgeControl, BridgeProcess
// POS:    Duplex Node bridge process host with independently writable stdin and cancellable stdout reading.

use std::process::Stdio;
use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter, Lines};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};
use tokio::sync::Mutex;
use tokio::time::{timeout, Duration};
use tracing::{debug, warn};

use alva_engine_runtime::RuntimeError;

use crate::protocol::{BridgeMessage, BridgeOutbound};

const SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);
const CONTROL_WRITE_TIMEOUT: Duration = Duration::from_secs(2);

/// Configuration for spawning the Node bridge child process.
pub(crate) struct BridgeSpawnConfig {
    pub node_path: String,
    pub script_path: String,
    pub config_json: String,
    pub env: Vec<(String, String)>,
}

/// Cloneable, independently lockable writer for bridge stdin.
///
/// The adapter's control-forwarding task owns a clone while its main task is
/// blocked in `recv()`, so permission responses never wait for stdout traffic.
#[derive(Clone)]
pub(crate) struct BridgeControl {
    stdin: Arc<Mutex<BufWriter<ChildStdin>>>,
}

impl BridgeControl {
    pub(crate) async fn send(&self, msg: &BridgeOutbound) -> Result<(), RuntimeError> {
        let json = serde_json::to_string(msg)?;
        timeout(CONTROL_WRITE_TIMEOUT, async {
            let mut stdin = self.stdin.lock().await;
            stdin.write_all(json.as_bytes()).await?;
            stdin.write_all(b"\n").await?;
            stdin.flush().await
        })
        .await
        .map_err(|_| RuntimeError::ProcessError("Bridge control write timed out".into()))??;
        Ok(())
    }
}

/// Manages child lifecycle and the stdout half of the JSON-line protocol.
pub(crate) struct BridgeProcess {
    child: Child,
    control: BridgeControl,
    stdout_lines: Lines<BufReader<ChildStdout>>,
}

impl BridgeProcess {
    /// Spawn the bridge. The returned control handle is safe to clone and use
    /// while `recv()` awaits stdout.
    pub(crate) async fn spawn(config: BridgeSpawnConfig) -> Result<Self, RuntimeError> {
        let mut cmd = Command::new(&config.node_path);
        cmd.arg(&config.script_path)
            .arg(&config.config_json)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        for (key, value) in &config.env {
            cmd.env(key, value);
        }

        let mut child = cmd.spawn().map_err(|error| {
            RuntimeError::ProcessError(format!(
                "Failed to spawn Node.js bridge ({}): {error}",
                config.node_path
            ))
        })?;
        let stdin = child
            .stdin
            .take()
            .ok_or_else(|| RuntimeError::ProcessError("Failed to capture stdin".into()))?;
        let stdout = child
            .stdout
            .take()
            .ok_or_else(|| RuntimeError::ProcessError("Failed to capture stdout".into()))?;
        monitor_stderr(child.stderr.take());

        Ok(Self {
            child,
            control: BridgeControl {
                stdin: Arc::new(Mutex::new(BufWriter::new(stdin))),
            },
            stdout_lines: BufReader::new(stdout).lines(),
        })
    }

    pub(crate) fn control(&self) -> BridgeControl {
        self.control.clone()
    }

    /// Await one stdout line. `Lines::next_line` is cancellation-safe, so this
    /// future may be used in a select loop if a caller needs to stop reading.
    pub(crate) async fn recv(&mut self) -> Result<Option<BridgeMessage>, RuntimeError> {
        match self.stdout_lines.next_line().await? {
            Some(line) => serde_json::from_str(&line).map(Some).map_err(|error| {
                RuntimeError::ProtocolError(format!(
                    "Invalid JSON from bridge: {error} — line: {line}"
                ))
            }),
            None => Ok(None),
        }
    }

    pub(crate) async fn shutdown(&mut self) -> Result<(), RuntimeError> {
        let _ = self.control.send(&BridgeOutbound::Shutdown).await;
        match timeout(SHUTDOWN_TIMEOUT, self.child.wait()).await {
            Ok(Ok(_)) => Ok(()),
            _ => self.kill().await,
        }
    }

    pub(crate) async fn kill(&mut self) -> Result<(), RuntimeError> {
        self.child.kill().await.map_err(|error| {
            RuntimeError::ProcessError(format!("Failed to kill bridge process: {error}"))
        })
    }
}

fn monitor_stderr(stderr: Option<tokio::process::ChildStderr>) {
    if let Some(stderr) = stderr {
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if is_fatal_stderr(&line) {
                    warn!(target: "claude_bridge", "Fatal stderr: {line}");
                } else {
                    debug!(target: "claude_bridge", "stderr: {line}");
                }
            }
        });
    }
}

fn is_fatal_stderr(line: &str) -> bool {
    let lower = line.to_lowercase();
    [
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
    ]
    .iter()
    .any(|pattern| lower.contains(pattern))
}

#[cfg(test)]
mod tests {
    //! Tests for `is_fatal_stderr` + `BridgeProcess::spawn` error-path
    //! diagnostic format.
    //!
    //! `is_fatal_stderr` decides which stderr lines from the Node.js
    //! bridge go to `warn!` (user-visible) vs `debug!` (silently
    //! filtered). The 11-pattern list is silent state — a typo like
    //! "rate_limt" would let real rate-limit errors silently downgrade
    //! to debug and the user sees a black-box hang with no alert.
    //!
    //! `BridgeProcess::spawn` ProcessError messages MUST include the
    //! attempted `node_path` so users can diagnose "node not found"
    //! type errors without re-running with strace.
    use super::*;

    // -- is_fatal_stderr: 11 patterns (table-driven) --------------------

    #[test]
    fn is_fatal_stderr_detects_each_pattern_in_realistic_stderr_lines() {
        // Each row exercises one of the 11 patterns embedded in a
        // realistic stderr line (substring match, not exact). Both
        // underscore and space forms covered for the dual-emit
        // patterns (Anthropic SDK emits both depending on layer).
        // ECONNREFUSED row also exercises case-insensitivity.
        let positive_cases = [
            ("authentication_error", "authentication_error: invalid key"),
            (
                "authentication error",
                "Got: authentication error from server",
            ),
            ("invalid_api_key", "invalid_api_key"),
            ("invalid api key", "Got an invalid API key for request"),
            ("unauthorized", "401 Unauthorized"),
            ("rate_limit", "rate_limit exceeded"),
            ("rate limit", "hit a rate limit at provider"),
            ("quota_exceeded", "quota_exceeded for org-id"),
            ("billing", "Your billing account is suspended"),
            ("overloaded", "overloaded — please retry"),
            ("connection_refused", "connection_refused on socket"),
            ("econnrefused", "Error: ECONNREFUSED 127.0.0.1:443"),
        ];
        for (pattern, line) in positive_cases {
            assert!(
                is_fatal_stderr(line),
                "pattern {pattern:?} must classify {line:?} as fatal"
            );
        }
    }

    // -- is_fatal_stderr: behavior pins (separate contracts) ------------

    #[test]
    fn is_fatal_stderr_is_case_insensitive() {
        // Pin: the function lowercases the input first. A refactor
        // that dropped the .to_lowercase() would silently let
        // "AUTHENTICATION_ERROR" (e.g. screaming-snake from a JS
        // throw) bypass the filter.
        assert!(is_fatal_stderr("AUTHENTICATION_ERROR"));
        assert!(is_fatal_stderr("Authentication_Error"));
        assert!(is_fatal_stderr("UNAUTHORIZED"));
    }

    #[test]
    fn is_fatal_stderr_uses_substring_contains_not_exact_match() {
        // Pin: patterns are matched via `.contains()`, so the pattern
        // can appear anywhere in the line (typical for "Error: <json
        // blob with rate_limit field>"). A refactor to `.starts_with`
        // or `==` would silently drop most real-world errors.
        assert!(is_fatal_stderr("prefix middle rate_limit suffix"));
    }

    #[test]
    fn is_fatal_stderr_returns_false_for_unrelated_lines() {
        // Negative pin: regular debug/info lines must NOT be promoted
        // to warn. This is the noise floor — failure means spam.
        assert!(!is_fatal_stderr(""));
        assert!(!is_fatal_stderr("[bridge] connected successfully"));
        assert!(!is_fatal_stderr("Stream chunk received: 42 bytes"));
        assert!(!is_fatal_stderr("Tool execution completed"));
    }

    // -- BridgeProcess::spawn error-diagnostic pin -----------------------

    #[tokio::test]
    async fn spawn_with_nonexistent_node_path_returns_process_error_naming_path() {
        // Pin: ProcessError diagnostic MUST include the attempted
        // `node_path` AND the OS error text so users can diagnose
        // (e.g.) "node not installed" without re-running with strace.
        //
        // Use a path that definitely doesn't exist as a binary.
        let cfg = BridgeSpawnConfig {
            node_path: "/nonexistent/bin/this-node-does-not-exist-xyz123".into(),
            script_path: "/tmp/never-read.js".into(),
            config_json: "{}".into(),
            env: vec![],
        };
        let result = BridgeProcess::spawn(cfg).await;
        match result {
            Err(RuntimeError::ProcessError(msg)) => {
                assert!(
                    msg.contains("Failed to spawn Node.js bridge"),
                    "diagnostic prefix missing: {msg}"
                );
                assert!(
                    msg.contains("/nonexistent/bin/this-node-does-not-exist-xyz123"),
                    "node_path must appear in diagnostic for debuggability: {msg}"
                );
            }
            Err(other) => panic!("expected ProcessError, got {other:?}"),
            // BridgeProcess has no Debug impl, so Ok variant can't
            // print its inner — bare message is fine.
            Ok(_) => panic!("expected spawn to fail on nonexistent node path"),
        }
    }
}
