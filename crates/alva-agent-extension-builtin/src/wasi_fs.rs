// INPUT:  std::{fs, io, path}, glob, async_trait, alva_kernel_abi::{ToolFs, ToolFsDirEntry, ToolFsExecResult, AgentError}
// OUTPUT: WasiFs, WasiFsMetadata (sync WASI file facade plus ToolFs impl)
// POS:    Capability-confined WASI filesystem adapter shared by built-in tools and guest script bindings.

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

/// Script-friendly metadata returned by [`WasiFs::metadata_sync`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WasiFsMetadata {
    pub is_file: bool,
    pub is_dir: bool,
    pub size: u64,
    pub readonly: bool,
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

    /// Synchronous facade used by QuickJS callbacks. It deliberately lives on
    /// the WASI adapter so embedded runtimes never call `std::fs` themselves.
    pub fn read_file_sync(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        let full = self.resolve(path);
        std::fs::read(&full).map_err(|error| Self::io_error("read", &full, error))
    }

    pub fn write_file_sync(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Self::io_error("create parent directories for", &full, error))?;
        }
        std::fs::write(&full, content).map_err(|error| Self::io_error("write", &full, error))
    }

    pub fn append_file_sync(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        use std::io::Write;

        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Self::io_error("create parent directories for", &full, error))?;
        }
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&full)
            .map_err(|error| Self::io_error("open for append", &full, error))?;
        file.write_all(content)
            .map_err(|error| Self::io_error("append", &full, error))
    }

    pub fn exists_sync(&self, path: &str) -> Result<bool, AgentError> {
        let full = self.resolve(path);
        full.try_exists()
            .map_err(|error| Self::io_error("check existence of", &full, error))
    }

    pub fn remove_sync(&self, path: &str) -> Result<(), AgentError> {
        let full = self.resolve(path);
        let metadata = std::fs::metadata(&full)
            .map_err(|error| Self::io_error("read metadata for", &full, error))?;
        if metadata.is_dir() {
            std::fs::remove_dir_all(&full)
                .map_err(|error| Self::io_error("remove directory", &full, error))
        } else {
            std::fs::remove_file(&full).map_err(|error| Self::io_error("remove file", &full, error))
        }
    }

    pub fn rename_sync(&self, from: &str, to: &str) -> Result<(), AgentError> {
        let from = self.resolve(from);
        let to = self.resolve(to);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Self::io_error("create parent directories for", &to, error))?;
        }
        std::fs::rename(&from, &to).map_err(|error| AgentError::ToolError {
            tool_name: "wasi_fs::rename".to_string(),
            message: format!(
                "cannot rename '{}' to '{}': {error}",
                from.display(),
                to.display()
            ),
        })
    }

    pub fn create_dir_all_sync(&self, path: &str) -> Result<(), AgentError> {
        let full = self.resolve(path);
        std::fs::create_dir_all(&full)
            .map_err(|error| Self::io_error("create directory", &full, error))
    }

    pub fn list_dir_sync(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
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
        entries.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(entries)
    }

    pub fn metadata_sync(&self, path: &str) -> Result<WasiFsMetadata, AgentError> {
        let full = self.resolve(path);
        let metadata = std::fs::metadata(&full)
            .map_err(|error| Self::io_error("read metadata for", &full, error))?;
        Ok(WasiFsMetadata {
            is_file: metadata.is_file(),
            is_dir: metadata.is_dir(),
            size: metadata.len(),
            readonly: metadata.permissions().readonly(),
        })
    }

    pub fn copy_file_sync(&self, from: &str, to: &str) -> Result<u64, AgentError> {
        let from = self.resolve(from);
        let to = self.resolve(to);
        if let Some(parent) = to.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| Self::io_error("create parent directories for", &to, error))?;
        }
        std::fs::copy(&from, &to).map_err(|error| AgentError::ToolError {
            tool_name: "wasi_fs::copy".to_string(),
            message: format!(
                "cannot copy '{}' to '{}': {error}",
                from.display(),
                to.display()
            ),
        })
    }

    pub fn glob_sync(&self, pattern: &str) -> Result<Vec<String>, AgentError> {
        let full_pattern = self.resolve(pattern);
        let full_pattern = full_pattern.to_string_lossy();
        let paths = glob::glob(&full_pattern).map_err(|error| AgentError::ToolError {
            tool_name: "wasi_fs::glob".to_string(),
            message: format!("invalid glob pattern {pattern:?}: {error}"),
        })?;
        let mut matches = Vec::new();
        for path in paths {
            let path = path.map_err(|error| AgentError::ToolError {
                tool_name: "wasi_fs::glob".to_string(),
                message: format!("glob failed for {pattern:?}: {error}"),
            })?;
            matches.push(
                path.strip_prefix(&self.root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .into_owned(),
            );
        }
        matches.sort();
        Ok(matches)
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
        self.read_file_sync(path)
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        self.write_file_sync(path, content)
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        self.list_dir_sync(path)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        self.exists_sync(path)
    }
}
