// INPUT:  tokio::process, manifest::Runtime
// OUTPUT: SubprocessRuntime, ReadHalf, WriteHalf, ShutdownHandle, SubprocessError
// POS:    Phase 2 — spawn / read / write / shutdown a plugin subprocess.

//! Subprocess runtime for AEP plugins.
//!
//! Spawns a plugin binary (Python or Node) with piped stdio, forwards
//! its stderr into the host tracing system, and exposes split read /
//! write halves plus a shutdown handle. Phase 2 scope: **transport
//! only** — no JSON-RPC semantics live here; that is `dispatcher.rs`.
//!
//! ## Launcher commands
//!
//! The real v1 Python SDK launcher will be `python -m alva_sdk <entry>`,
//! but the SDK does not exist yet (Phase 4). Until then we spawn
//! `python3 -u <entry>` so test plugins can be plain stdlib scripts.
//! The caller can override both programs via `LauncherOverride` — this
//! keeps tests hermetic and gives operators an escape hatch for
//! non-default Python / Node installations.

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStderr, ChildStdin, ChildStdout, Command};
use tokio::task::JoinHandle;

use crate::manifest::Runtime;

/// How long to wait for a subprocess to exit gracefully after we
/// close its stdin before we escalate to `kill`.
const GRACEFUL_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(3);

/// How long to wait after `kill` before we give up and return an
/// error. This is only hit if the OS refuses to reap the child.
const HARD_KILL_TIMEOUT: Duration = Duration::from_secs(2);

/// Caller-supplied override for the launcher command.
///
/// If `None`, a sensible per-runtime default is used
/// (`python3 -u` for Python, `node --enable-source-maps` for JS).
/// Tests pass an override to avoid depending on the future Phase 4 SDK.
#[derive(Debug, Clone, Default)]
pub struct LauncherOverride {
    /// Program name or absolute path, e.g. `"python3"`.
    pub program: String,
    /// Args to prepend before the entry file path.
    pub prepend_args: Vec<String>,
    /// Extra environment variables for the child process. Merged on
    /// top of the inherited parent environment. Typical use: `PYTHONPATH`
    /// so a test or install can find a locally-built SDK.
    pub env: Vec<(String, String)>,
}

/// A spawned plugin subprocess with piped stdio.
///
/// Phase 2 does not implement the JSON-RPC protocol — `SubprocessRuntime`
/// is purely a transport. Use `RpcDispatcher::spawn` to wrap one of
/// these in a fully-working dispatcher.
pub struct SubprocessRuntime {
    name: String,
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: Option<BufReader<ChildStdout>>,
    stderr_task: Option<JoinHandle<()>>,
}

impl SubprocessRuntime {
    /// Spawn a plugin subprocess.
    ///
    /// `name` is a human-friendly label used only for logging. `entry`
    /// is the script file path handed to the launcher. `cwd` sets the
    /// working directory (defaults to inherit).
    pub async fn spawn(
        name: impl Into<String>,
        runtime: Runtime,
        entry: impl Into<PathBuf>,
        cwd: Option<PathBuf>,
        launcher_override: Option<LauncherOverride>,
    ) -> Result<Self, SubprocessError> {
        let name = name.into();
        let entry = entry.into();
        let launcher = launcher_override.unwrap_or_else(|| default_launcher(runtime));

        let mut cmd = Command::new(&launcher.program);
        cmd.args(&launcher.prepend_args)
            .arg(&entry)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = cwd {
            cmd.current_dir(cwd);
        }
        for (k, v) in &launcher.env {
            cmd.env(k, v);
        }

        tracing::debug!(
            plugin = %name,
            program = %launcher.program,
            entry = %entry.display(),
            "spawning plugin subprocess"
        );

        let mut child = cmd.spawn().map_err(SubprocessError::Spawn)?;

        let stdin = child
            .stdin
            .take()
            .expect("stdin was piped and not yet taken");
        let stdout = BufReader::new(
            child
                .stdout
                .take()
                .expect("stdout was piped and not yet taken"),
        );
        let stderr = child
            .stderr
            .take()
            .expect("stderr was piped and not yet taken");

        let stderr_task = spawn_stderr_drain(name.clone(), stderr);

        Ok(Self {
            name,
            child,
            stdin: Some(stdin),
            stdout: Some(stdout),
            stderr_task: Some(stderr_task),
        })
    }

    /// Plugin name (for logging).
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Read one newline-delimited message from the subprocess's
    /// stdout, without the trailing newline. Returns `Ok(None)` on
    /// clean EOF.
    pub async fn read_message(&mut self) -> Result<Option<String>, SubprocessError> {
        let stdout = self
            .stdout
            .as_mut()
            .ok_or(SubprocessError::AlreadyClosed)?;
        read_one_line(stdout).await
    }

    /// Write one message to the subprocess's stdin, appending a
    /// newline and flushing.
    pub async fn write_message(&mut self, line: &str) -> Result<(), SubprocessError> {
        let stdin = self
            .stdin
            .as_mut()
            .ok_or(SubprocessError::AlreadyClosed)?;
        write_one_line(stdin, line).await
    }

    /// Consume this runtime and split it into independently-owned
    /// read, write, and shutdown halves. After splitting, the halves
    /// can safely live in different async tasks.
    pub fn split(mut self) -> (ReadHalf, WriteHalf, ShutdownHandle) {
        let stdin = self.stdin.take().expect("stdin present until split");
        let stdout = self.stdout.take().expect("stdout present until split");
        let stderr_task = self.stderr_task.take();
        (
            ReadHalf {
                name: self.name.clone(),
                stdout,
            },
            WriteHalf {
                name: self.name.clone(),
                stdin,
            },
            ShutdownHandle {
                name: self.name,
                child: self.child,
                stderr_task,
            },
        )
    }

    /// Shut down the subprocess without first splitting. Closes stdin
    /// then waits for exit with a grace period, escalating to `kill`.
    pub async fn shutdown(self) -> Result<std::process::ExitStatus, SubprocessError> {
        let (_read, write, handle) = self.split();
        // Dropping `write` closes stdin — this is the EOF signal that
        // well-behaved plugins use to exit cleanly.
        drop(write);
        handle.shutdown().await
    }
}

/// Read half of a split subprocess. Owned by exactly one task.
pub struct ReadHalf {
    name: String,
    stdout: BufReader<ChildStdout>,
}

impl ReadHalf {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn read_message(&mut self) -> Result<Option<String>, SubprocessError> {
        read_one_line(&mut self.stdout).await
    }
}

/// Write half of a split subprocess. Owned by exactly one task.
pub struct WriteHalf {
    name: String,
    stdin: ChildStdin,
}

impl WriteHalf {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub async fn write_message(&mut self, line: &str) -> Result<(), SubprocessError> {
        write_one_line(&mut self.stdin, line).await
    }
}

/// Shutdown half of a split subprocess — retains the child handle so
/// we can wait for exit, escalate to kill, and collect the exit code.
pub struct ShutdownHandle {
    name: String,
    child: Child,
    stderr_task: Option<JoinHandle<()>>,
}

impl ShutdownHandle {
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Wait up to `GRACEFUL_SHUTDOWN_TIMEOUT` for the child to exit
    /// on its own. If it does not, send a kill signal and wait up to
    /// `HARD_KILL_TIMEOUT` for reap.
    pub async fn shutdown(mut self) -> Result<std::process::ExitStatus, SubprocessError> {
        let name = self.name.clone();
        let status = match tokio::time::timeout(
            GRACEFUL_SHUTDOWN_TIMEOUT,
            self.child.wait(),
        )
        .await
        {
            Ok(Ok(status)) => {
                tracing::debug!(plugin = %name, ?status, "plugin exited gracefully");
                status
            }
            Ok(Err(e)) => return Err(SubprocessError::Io(e)),
            Err(_) => {
                tracing::warn!(
                    plugin = %name,
                    "plugin did not exit within grace period, sending kill"
                );
                self.child.start_kill().map_err(SubprocessError::Io)?;
                match tokio::time::timeout(HARD_KILL_TIMEOUT, self.child.wait()).await {
                    Ok(Ok(status)) => status,
                    Ok(Err(e)) => return Err(SubprocessError::Io(e)),
                    Err(_) => {
                        return Err(SubprocessError::ShutdownTimeout(
                            (GRACEFUL_SHUTDOWN_TIMEOUT + HARD_KILL_TIMEOUT).as_secs(),
                        ))
                    }
                }
            }
        };

        // Reap the stderr draining task so it does not outlive the child.
        if let Some(handle) = self.stderr_task.take() {
            handle.abort();
            let _ = handle.await;
        }

        Ok(status)
    }
}

// ===========================================================
// Internals
// ===========================================================

async fn read_one_line(
    stdout: &mut BufReader<ChildStdout>,
) -> Result<Option<String>, SubprocessError> {
    let mut line = String::new();
    let n = stdout.read_line(&mut line).await?;
    if n == 0 {
        return Ok(None);
    }
    // Strip trailing newline(s).
    if line.ends_with('\n') {
        line.pop();
        if line.ends_with('\r') {
            line.pop();
        }
    }
    Ok(Some(line))
}

async fn write_one_line(stdin: &mut ChildStdin, line: &str) -> Result<(), SubprocessError> {
    stdin.write_all(line.as_bytes()).await?;
    stdin.write_all(b"\n").await?;
    stdin.flush().await?;
    Ok(())
}

fn default_launcher(runtime: Runtime) -> LauncherOverride {
    match runtime {
        Runtime::Python => LauncherOverride {
            program: "python3".to_string(),
            prepend_args: vec!["-u".to_string()],
            env: vec![],
        },
        Runtime::Javascript => LauncherOverride {
            program: "node".to_string(),
            prepend_args: vec!["--enable-source-maps".to_string()],
            env: vec![],
        },
    }
}

fn spawn_stderr_drain(plugin: String, stderr: ChildStderr) -> JoinHandle<()> {
    tokio::spawn(async move {
        let mut reader = BufReader::new(stderr).lines();
        loop {
            match reader.next_line().await {
                Ok(Some(line)) => {
                    tracing::warn!(
                        target: "aep.plugin.stderr",
                        plugin = %plugin,
                        "{}",
                        line
                    );
                }
                Ok(None) => break,
                Err(e) => {
                    tracing::error!(
                        target: "aep.plugin.stderr",
                        plugin = %plugin,
                        error = %e,
                        "stderr read error"
                    );
                    break;
                }
            }
        }
    })
}

// ===========================================================
// Error
// ===========================================================

#[derive(Debug, thiserror::Error)]
pub enum SubprocessError {
    #[error("failed to spawn subprocess: {0}")]
    Spawn(#[source] std::io::Error),

    #[error("subprocess I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("invalid UTF-8 from subprocess stdout: {0}")]
    Utf8(#[from] std::string::FromUtf8Error),

    #[error("subprocess did not exit within {0}s after shutdown signal")]
    ShutdownTimeout(u64),

    #[error("subprocess handle already consumed")]
    AlreadyClosed,
}

// Unused import guard — `Path` is referenced only in the public
// signature of `spawn`'s ergonomic conversion; `impl Into<PathBuf>`
// covers `&Path` so we do not need the bare type beyond that.
#[allow(dead_code)]
fn _assert_path_available(_: &Path) {}
