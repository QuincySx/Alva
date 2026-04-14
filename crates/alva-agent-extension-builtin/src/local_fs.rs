// INPUT:  tokio::fs, tokio::process, tokio::time, alva_kernel_abi::{ToolFs, ToolFsDirEntry, ToolFsExecResult, AgentError}
// OUTPUT: LocalToolFs (ToolFs impl backed by local OS)
// POS:    Concrete ToolFs implementation for the local filesystem and shell execution.

use async_trait::async_trait;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tokio::time::timeout;
use std::time::Duration;

use alva_kernel_abi::{AgentError, ToolFs, ToolFsDirEntry, ToolFsExecResult};

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

        if let Some(dir) = cwd {
            cmd.current_dir(self.resolve(dir));
        } else {
            cmd.current_dir(&self.root);
        }

        let duration = Duration::from_millis(timeout_ms);
        let result = timeout(duration, cmd.output()).await;

        match result {
            Err(_elapsed) => Err(AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("command timed out after {}ms: {}", timeout_ms, command),
            }),
            Ok(Err(io_err)) => Err(AgentError::ToolError {
                tool_name: "local_fs::exec".to_string(),
                message: format!("failed to spawn command: {}", io_err),
            }),
            Ok(Ok(output)) => {
                let stdout = String::from_utf8_lossy(&output.stdout).trim_end().to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).trim_end().to_string();
                let exit_code = output.status.code().unwrap_or(-1);
                Ok(ToolFsExecResult {
                    stdout,
                    stderr,
                    exit_code,
                })
            }
        }
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        let full = self.resolve(path);
        tokio::fs::read(&full).await.map_err(|e| AgentError::ToolError {
            tool_name: "local_fs::read_file".to_string(),
            message: format!("cannot read '{}': {}", full.display(), e),
        })
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| AgentError::ToolError {
                tool_name: "local_fs::write_file".to_string(),
                message: format!("cannot create parent dirs for '{}': {}", full.display(), e),
            })?;
        }
        tokio::fs::write(&full, content).await.map_err(|e| AgentError::ToolError {
            tool_name: "local_fs::write_file".to_string(),
            message: format!("cannot write '{}': {}", full.display(), e),
        })
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let full = self.resolve(path);
        let mut read_dir =
            tokio::fs::read_dir(&full).await.map_err(|e| AgentError::ToolError {
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
        tokio::fs::try_exists(&full).await.map_err(|e| AgentError::ToolError {
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

    #[tokio::test]
    async fn test_exec_echo() {
        let (fs, _dir) = make_fs();
        let result = fs.exec("echo hello", None, 5000).await.expect("exec succeeded");
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
        assert!(result.stdout.contains("/usr"), "stdout was: {}", result.stdout);
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
