// INPUT:  tokio::{fs,io,process,time}, OS process-group primitives, alva_kernel_abi::{ToolFs, ToolFsDirEntry, ToolFsExecResult, AgentError}
// OUTPUT: LocalToolFs (ToolFs impl backed by local OS), LocalToolFs::spawn_background
// POS:    Concrete native ToolFs implementation and the final resource boundary for host shell execution.

use async_trait::async_trait;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::timeout;

use alva_kernel_abi::{AgentError, ToolFs, ToolFsDirEntry, ToolFsExecResult};

const MAX_EXEC_STREAM_BYTES: usize = 1024 * 1024;

async fn read_capped<R>(mut reader: R) -> io::Result<(Vec<u8>, bool)>
where
    R: AsyncRead + Unpin,
{
    let mut captured = Vec::new();
    let mut exceeded = false;
    let mut chunk = [0_u8; 16 * 1024];
    loop {
        let read = reader.read(&mut chunk).await?;
        if read == 0 {
            return Ok((captured, exceeded));
        }
        let remaining = MAX_EXEC_STREAM_BYTES.saturating_sub(captured.len());
        let keep = remaining.min(read);
        captured.extend_from_slice(&chunk[..keep]);
        exceeded |= keep < read;
        // Continue draining after the cap. Stopping here would fill the pipe
        // and deadlock the child before its timeout can be observed.
    }
}

#[cfg(unix)]
fn configure_process_group(command: &mut Command) {
    command.process_group(0);
}

#[cfg(not(unix))]
fn configure_process_group(_command: &mut Command) {}

#[cfg(unix)]
fn kill_process_group(pid: Option<u32>) {
    if let Some(pid) = pid.and_then(|pid| i32::try_from(pid).ok()) {
        // SAFETY: a negative pid addresses the process group created with
        // process_group(0). SIGKILL is used only during cleanup/timeout.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
}

#[cfg(not(unix))]
fn kill_process_group(_pid: Option<u32>) {}

/// Ensures cancellation by an outer future still terminates the full process
/// group. `Child::kill_on_drop` only guarantees the direct shell is killed;
/// this guard covers descendants created by that shell as well.
struct ProcessGroupGuard(Option<u32>);

impl ProcessGroupGuard {
    fn kill(&mut self) {
        kill_process_group(self.0.take());
    }
}

impl Drop for ProcessGroupGuard {
    fn drop(&mut self) {
        self.kill();
    }
}

// ---------------------------------------------------------------------------
// Secret env scrubbing for spawned shells
// ---------------------------------------------------------------------------

/// Env var names that hold credentials and must never be inherited by a shell
/// command the model can drive. A child process inherits the parent's
/// environment by default, so without this an injected prompt could run
/// `printenv` / `echo $ANTHROPIC_API_KEY` and exfiltrate the key.
///
/// This is a deny-LIST (belt-and-suspenders): the app layer is expected to
/// also `remove_var` its own key from the agent process at startup (which
/// additionally closes the `/proc/<pid>/environ` vector). The list errs toward
/// well-known provider credentials and avoids names like `SSH_AUTH_SOCK` /
/// `XAUTHORITY` that legitimate commands rely on.
const SECRET_ENV_VARS: &[&str] = &[
    // Alva's own provider key (also scrubbed from the process by the host).
    "ALVA_API_KEY",
    "ALVA_AUTH_TOKEN",
    // Common LLM provider credentials a user may also have exported.
    "ANTHROPIC_API_KEY",
    "ANTHROPIC_AUTH_TOKEN",
    "OPENAI_API_KEY",
    "OPENAI_API_KEY_PATH",
    "GEMINI_API_KEY",
    "GOOGLE_API_KEY",
    "GOOGLE_APPLICATION_CREDENTIALS",
    "MOONSHOT_API_KEY",
    "KIMI_API_KEY",
    "DEEPSEEK_API_KEY",
    "GROQ_API_KEY",
    "MISTRAL_API_KEY",
    "COHERE_API_KEY",
    "OPENROUTER_API_KEY",
    // Cloud / VCS credentials commonly present in a dev environment.
    "AWS_SECRET_ACCESS_KEY",
    "AWS_SESSION_TOKEN",
    "GITHUB_TOKEN",
    "GH_TOKEN",
];

/// Strip credential-bearing env vars from a [`Command`] before it is spawned,
/// so a model-driven shell cannot read them back. Non-secret env (PATH, HOME,
/// locale, …) is left inherited so ordinary commands keep working.
pub(crate) fn scrub_secret_env(cmd: &mut Command) {
    for var in SECRET_ENV_VARS {
        cmd.env_remove(var);
    }
}

// ---------------------------------------------------------------------------
// LocalToolFs
// ---------------------------------------------------------------------------

/// A [`ToolFs`] implementation backed by the real local operating system.
///
/// Relative paths are resolved against `root`. Absolute paths are used as-is.
pub struct LocalToolFs {
    root: PathBuf,
}

impl LocalToolFs {
    /// Create a new `LocalToolFs` rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    /// Resolve `path` to an absolute path.
    ///
    /// - Absolute paths → used as-is.
    /// - Relative paths → joined with `self.root`.
    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }

    /// Spawn a deliberately detached shell command and return its process id.
    ///
    /// Unlike [`ToolFs::exec`], this method does not attach a process-tree
    /// cleanup guard: surviving the initiating tool call is the explicit
    /// contract of background execution. Standard streams are disconnected,
    /// credentials are scrubbed, and Unix children start in a separate process
    /// group so terminal interrupts aimed at the agent do not reach them.
    pub async fn spawn_background(
        &self,
        command: &str,
        cwd: Option<&str>,
    ) -> Result<u32, AgentError> {
        let mut cmd = Command::new("sh");
        cmd.kill_on_drop(false);
        cmd.arg("-c").arg(command);
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        configure_process_group(&mut cmd);
        scrub_secret_env(&mut cmd);

        if let Some(dir) = cwd {
            cmd.current_dir(self.resolve(dir));
        } else {
            cmd.current_dir(&self.root);
        }

        let mut child = cmd.spawn().map_err(|io_err| AgentError::ToolError {
            tool_name: "local_fs::spawn_background".to_string(),
            message: format!("failed to spawn background command: {io_err}"),
        })?;
        let pid = child.id().ok_or_else(|| AgentError::ToolError {
            tool_name: "local_fs::spawn_background".to_string(),
            message: "background command started without a process id".to_string(),
        })?;

        // Reap the process while this runtime is alive. If the runtime itself
        // shuts down, kill_on_drop(false) intentionally leaves the detached
        // command running instead of silently violating the background API.
        tokio::spawn(async move {
            if let Err(error) = child.wait().await {
                tracing::warn!(pid, %error, "failed to reap background command");
            }
        });
        Ok(pid)
    }
}

#[async_trait]
impl ToolFs for LocalToolFs {
    async fn exec(
        &self,
        command: &str,
        cwd: Option<&str>,
        timeout_ms: u64,
    ) -> Result<ToolFsExecResult, AgentError> {
        let mut cmd = Command::new("sh");
        cmd.kill_on_drop(true);
        cmd.arg("-c").arg(command);
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());
        configure_process_group(&mut cmd);
        scrub_secret_env(&mut cmd);

        if let Some(dir) = cwd {
            cmd.current_dir(self.resolve(dir));
        } else {
            cmd.current_dir(&self.root);
        }

        let duration = Duration::from_millis(timeout_ms);
        let started = std::time::Instant::now();
        let mut child = cmd.spawn().map_err(|io_err| AgentError::ToolError {
            tool_name: "local_fs::exec".to_string(),
            message: format!("failed to spawn command: {}", io_err),
        })?;
        let pid = child.id();
        let mut process_group = ProcessGroupGuard(pid);
        let stdout = child
            .stdout
            .take()
            .expect("stdout is piped before spawning the shell");
        let stderr = child
            .stderr
            .take()
            .expect("stderr is piped before spawning the shell");
        let mut stdout_task = tokio::spawn(read_capped(stdout));
        let mut stderr_task = tokio::spawn(read_capped(stderr));

        let status = match timeout(duration, child.wait()).await {
            Err(_elapsed) => {
                process_group.kill();
                let _ = child.kill().await;
                let _ = child.wait().await;
                stdout_task.abort();
                stderr_task.abort();
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(AgentError::ToolError {
                    tool_name: "local_fs::exec".to_string(),
                    message: format!("command timed out after {}ms: {}", timeout_ms, command),
                });
            }
            Ok(Err(io_err)) => {
                process_group.kill();
                let _ = child.kill().await;
                let _ = child.wait().await;
                stdout_task.abort();
                stderr_task.abort();
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(AgentError::ToolError {
                    tool_name: "local_fs::exec".to_string(),
                    message: format!("failed to wait for command: {}", io_err),
                });
            }
            Ok(Ok(status)) => status,
        };
        // The direct shell may exit after launching background descendants.
        // Terminate descendants that remain in its private group. The capture
        // deadline below still prevents an explicitly re-parented/re-grouped
        // process from holding these pipes and hanging the tool call forever.
        process_group.kill();

        let remaining = duration.saturating_sub(started.elapsed());
        let captures = match timeout(remaining, async {
            tokio::join!(&mut stdout_task, &mut stderr_task)
        })
        .await
        {
            Ok(captures) => captures,
            Err(_) => {
                stdout_task.abort();
                stderr_task.abort();
                let _ = stdout_task.await;
                let _ = stderr_task.await;
                return Err(AgentError::ToolError {
                    tool_name: "local_fs::exec".to_string(),
                    message: format!(
                        "command process tree did not close output before the {}ms timeout: {}",
                        timeout_ms, command
                    ),
                });
            }
        };
        let (stdout_capture, stderr_capture) = captures;
        let (stdout, stdout_exceeded) = stdout_capture
            .map_err(|error| AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("stdout capture task failed: {error}"),
            })?
            .map_err(|error| AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("failed to capture stdout: {error}"),
            })?;
        let (stderr, stderr_exceeded) = stderr_capture
            .map_err(|error| AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("stderr capture task failed: {error}"),
            })?
            .map_err(|error| AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("failed to capture stderr: {error}"),
            })?;
        if stdout_exceeded || stderr_exceeded {
            return Err(AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!(
                    "command output exceeded the {}-byte per-stream capture limit",
                    MAX_EXEC_STREAM_BYTES
                ),
            });
        }
        Ok(ToolFsExecResult {
            stdout: String::from_utf8_lossy(&stdout).trim_end().to_string(),
            stderr: String::from_utf8_lossy(&stderr).trim_end().to_string(),
            exit_code: status.code().unwrap_or(-1),
        })
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        let full = self.resolve(path);
        tokio::fs::read(&full)
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "local_fs::read_file".to_string(),
                message: format!("cannot read '{}': {}", full.display(), e),
            })
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| AgentError::ToolError {
                    tool_name: "local_fs::write_file".to_string(),
                    message: format!("cannot create parent dirs for '{}': {}", full.display(), e),
                })?;
        }
        tokio::fs::write(&full, content)
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "local_fs::write_file".to_string(),
                message: format!("cannot write '{}': {}", full.display(), e),
            })
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let full = self.resolve(path);
        let mut read_dir = tokio::fs::read_dir(&full)
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "local_fs::list_dir".to_string(),
                message: format!("cannot read dir '{}': {}", full.display(), e),
            })?;

        let mut entries = Vec::new();
        loop {
            match read_dir.next_entry().await {
                Err(e) => {
                    return Err(AgentError::ToolError {
                        tool_name: "local_fs::list_dir".to_string(),
                        message: format!("error reading entry in '{}': {}", full.display(), e),
                    })
                }
                Ok(None) => break,
                Ok(Some(entry)) => {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let meta = entry.metadata().await.map_err(|e| AgentError::ToolError {
                        tool_name: "local_fs::list_dir".to_string(),
                        message: format!("cannot read metadata for '{}': {}", name, e),
                    })?;
                    entries.push(ToolFsDirEntry {
                        name,
                        is_dir: meta.is_dir(),
                        size: meta.len(),
                    });
                }
            }
        }
        Ok(entries)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        let full = self.resolve(path);
        tokio::fs::try_exists(&full)
            .await
            .map_err(|e| AgentError::ToolError {
                tool_name: "local_fs::exists".to_string(),
                message: format!("cannot check existence of '{}': {}", full.display(), e),
            })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_fs() -> (LocalToolFs, TempDir) {
        let dir = TempDir::new().expect("create tempdir");
        let fs = LocalToolFs::new(dir.path().to_path_buf());
        (fs, dir)
    }

    #[test]
    fn scrub_secret_env_marks_credentials_for_removal() {
        // Pin the security property: after scrubbing, the Command is configured
        // to remove the well-known credential vars from the child's environment
        // (so an inherited ANTHROPIC_API_KEY / ALVA_API_KEY can't be read back
        // via `printenv`). We inspect the staged env mutations rather than
        // mutating the process environment (which would race other tests).
        let mut cmd = Command::new("sh");
        scrub_secret_env(&mut cmd);
        let removed: Vec<String> = cmd
            .as_std()
            .get_envs()
            .filter(|(_, v)| v.is_none())
            .map(|(k, _)| k.to_string_lossy().into_owned())
            .collect();
        for expected in ["ALVA_API_KEY", "ANTHROPIC_API_KEY", "OPENAI_API_KEY"] {
            assert!(
                removed.iter().any(|k| k == expected),
                "{expected} should be scrubbed from the child env; removed = {removed:?}"
            );
        }
        // A non-secret var must NOT be scrubbed (ordinary commands need PATH etc.)
        assert!(
            !removed.iter().any(|k| k == "PATH"),
            "PATH must not be scrubbed"
        );
    }

    #[tokio::test]
    async fn test_exec_echo() {
        let (fs, _dir) = make_fs();
        let result = fs
            .exec("echo hello", None, 5000)
            .await
            .expect("exec succeeded");
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "hello");
    }

    #[tokio::test]
    async fn test_exec_with_cwd() {
        let (fs, _dir) = make_fs();
        // Use an absolute path as cwd override
        let result = fs
            .exec("pwd", Some("/usr"), 5000)
            .await
            .expect("exec succeeded");
        assert_eq!(result.exit_code, 0);
        assert!(
            result.stdout.contains("/usr"),
            "stdout was: {}",
            result.stdout
        );
    }

    #[tokio::test]
    async fn test_exec_timeout() {
        let (fs, _dir) = make_fs();
        let err = fs
            .exec("sleep 10", None, 100)
            .await
            .expect_err("should time out");
        let msg = err.to_string();
        assert!(
            msg.contains("timed out") || msg.contains("timeout"),
            "unexpected error: {}",
            msg
        );
    }

    #[tokio::test]
    async fn test_exec_timeout_kills_child_process() {
        let (fs, dir) = make_fs();
        let marker = dir.path().join("timeout-marker.txt");
        let command = format!("sleep 1; touch '{}'", marker.display());

        let err = fs
            .exec(&command, None, 100)
            .await
            .expect_err("should time out");
        assert!(err.to_string().contains("timed out"));

        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            !marker.exists(),
            "timed out child process continued running and created {}",
            marker.display()
        );
    }

    #[tokio::test]
    async fn test_exec_normal_exit_kills_background_descendants() {
        let (fs, dir) = make_fs();
        let marker = dir.path().join("background-marker.txt");
        let command = format!("(sleep 1; touch '{}') &", marker.display());

        fs.exec(&command, None, 5000)
            .await
            .expect("direct shell exits successfully");
        tokio::time::sleep(Duration::from_millis(1200)).await;
        assert!(
            !marker.exists(),
            "background descendant outlived the tool call and created {}",
            marker.display()
        );
    }

    #[tokio::test]
    async fn spawn_background_survives_the_initiating_call() {
        let (fs, dir) = make_fs();
        let marker = dir.path().join("detached-marker.txt");
        let command = format!("sleep 0.1; touch '{}'", marker.display());

        let pid = fs
            .spawn_background(&command, None)
            .await
            .expect("background command should start");
        assert!(pid > 0);
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert!(
            marker.exists(),
            "detached command did not outlive its caller"
        );
    }

    #[tokio::test]
    async fn test_exec_rejects_output_above_capture_limit() {
        let (fs, _dir) = make_fs();
        let command = format!("head -c {} /dev/zero", MAX_EXEC_STREAM_BYTES + 1);
        let error = fs
            .exec(&command, None, 5000)
            .await
            .expect_err("oversized output must be rejected");
        assert!(error.to_string().contains("capture limit"), "{error}");
    }

    #[tokio::test]
    async fn test_write_and_read() {
        let (fs, _dir) = make_fs();
        let content = b"hello, world!";
        fs.write_file("test.txt", content)
            .await
            .expect("write succeeded");
        let read_back = fs.read_file("test.txt").await.expect("read succeeded");
        assert_eq!(read_back, content);
    }

    #[tokio::test]
    async fn test_list_dir_and_exists() {
        let (fs, _dir) = make_fs();

        // File does not yet exist
        assert!(!fs.exists("greet.txt").await.expect("exists check"));

        // Write a file
        fs.write_file("greet.txt", b"hi")
            .await
            .expect("write succeeded");

        // Now it should exist
        assert!(fs.exists("greet.txt").await.expect("exists check"));

        // List the root directory and find the file
        let entries = fs.list_dir(".").await.expect("list_dir succeeded");
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            names.contains(&"greet.txt"),
            "expected greet.txt in {:?}",
            names
        );
    }
}
