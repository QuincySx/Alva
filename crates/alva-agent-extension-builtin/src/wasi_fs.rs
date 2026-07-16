// INPUT:  std::fs, std::path, async_trait, alva_kernel_abi::{ToolFs, ToolFsDirEntry, ToolFsExecResult, AgentError}
// OUTPUT: WasiFs (ToolFs impl backed by WASI std::fs)
// POS:    WASI filesystem adapter using synchronous I/O and rejecting subprocess execution.

use std::path::{Path, PathBuf};

use alva_kernel_abi::{AgentError, ToolFs, ToolFsDirEntry, ToolFsExecResult};
use async_trait::async_trait;

/// A [`ToolFs`] implementation backed by WASI's synchronous filesystem APIs.
///
/// Relative paths are resolved against `root`. Absolute paths are passed to
/// WASI unchanged, where the runtime's preopened-directory capabilities decide
/// whether they are accessible.
pub struct WasiFs {
    root: PathBuf,
}

impl WasiFs {
    /// Create a new `WasiFs` rooted at `root`.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let path = Path::new(path);
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        }
    }

    fn io_error(operation: &str, path: &Path, error: std::io::Error) -> AgentError {
        AgentError::ToolError {
            tool_name: format!("wasi_fs::{operation}"),
            message: format!("cannot {operation} '{}': {error}", path.display()),
        }
    }
}

#[async_trait]
impl ToolFs for WasiFs {
    async fn exec(
        &self,
        _command: &str,
        _cwd: Option<&str>,
        _timeout_ms: u64,
    ) -> Result<ToolFsExecResult, AgentError> {
        Err(AgentError::ToolError {
            tool_name: "wasi_fs::exec".to_string(),
            message: "no subprocess on wasi".to_string(),
        })
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        let full = self.resolve(path);
        std::fs::read(&full).map_err(|error| Self::io_error("read", &full, error))
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Self::io_error("create parent directories for", &full, error))?;
        }
        std::fs::write(&full, content).map_err(|error| Self::io_error("write", &full, error))
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let full = self.resolve(path);
        let read_dir = std::fs::read_dir(&full)
            .map_err(|error| Self::io_error("read directory", &full, error))?;

        let mut entries = Vec::new();
        for entry in read_dir {
            let entry = entry.map_err(|error| Self::io_error("read entry in", &full, error))?;
            let metadata = entry
                .metadata()
                .map_err(|error| Self::io_error("read metadata for", &entry.path(), error))?;
            entries.push(ToolFsDirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: metadata.is_dir(),
                size: metadata.len(),
            });
        }
        Ok(entries)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        let full = self.resolve(path);
        full.try_exists()
            .map_err(|error| Self::io_error("check existence of", &full, error))
    }
}
