// INPUT:  std::collections::HashMap, std::sync::Mutex, alva_types::{AgentError, ToolFs, ToolFsDirEntry, ToolFsExecResult}, async_trait
// OUTPUT: MockToolFs — in-memory ToolFs for testing tools without real filesystem access
// POS:    Test-only ToolFs implementation that stores files in memory and serves pre-queued exec responses.

use std::collections::HashMap;
use std::sync::Mutex;

use async_trait::async_trait;
use alva_types::{AgentError, ToolFs, ToolFsDirEntry, ToolFsExecResult};

// ---------------------------------------------------------------------------
// MockToolFs
// ---------------------------------------------------------------------------

/// In-memory [`ToolFs`] for testing tools without real filesystem access.
///
/// Files are stored as raw bytes in a `HashMap<String, Vec<u8>>`.
/// Exec responses are pre-queued and consumed in FIFO order.
pub struct MockToolFs {
    files: Mutex<HashMap<String, Vec<u8>>>,
    /// Queued exec responses; the front is returned next (index 0).
    exec_responses: Mutex<Vec<ToolFsExecResult>>,
}

impl MockToolFs {
    /// Create a new, empty `MockToolFs`.
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            exec_responses: Mutex::new(Vec::new()),
        }
    }

    /// Pre-populate a file with the given byte content.
    pub fn with_file(self, path: &str, content: &[u8]) -> Self {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), content.to_vec());
        self
    }

    /// Queue an exec response. Responses are returned in FIFO order.
    pub fn with_exec_response(self, result: ToolFsExecResult) -> Self {
        self.exec_responses.lock().unwrap().push(result);
        self
    }
}

impl Default for MockToolFs {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ToolFs for MockToolFs {
    async fn exec(
        &self,
        _command: &str,
        _cwd: Option<&str>,
        _timeout_ms: u64,
    ) -> Result<ToolFsExecResult, AgentError> {
        let mut queue = self.exec_responses.lock().unwrap();
        if queue.is_empty() {
            return Err(AgentError::ToolError {
                tool_name: "mock_fs::exec".to_string(),
                message: "no more queued exec responses".to_string(),
            });
        }
        // FIFO: remove from front
        Ok(queue.remove(0))
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        let files = self.files.lock().unwrap();
        files.get(path).cloned().ok_or_else(|| AgentError::ToolError {
            tool_name: "mock_fs::read_file".to_string(),
            message: format!("file not found: {}", path),
        })
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        self.files
            .lock()
            .unwrap()
            .insert(path.to_string(), content.to_vec());
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let files = self.files.lock().unwrap();

        // Normalise: ensure the directory prefix ends with '/'
        let prefix = if path.ends_with('/') {
            path.to_string()
        } else {
            format!("{}/", path)
        };

        let mut entries: Vec<ToolFsDirEntry> = Vec::new();
        let mut seen_dirs: std::collections::HashSet<String> = std::collections::HashSet::new();

        for key in files.keys() {
            // Strip the directory prefix to get the relative portion
            let relative = if path.is_empty() || path == "." || path == "./" {
                // Root listing — every key is a candidate
                key.trim_start_matches("./")
            } else if let Some(rest) = key.strip_prefix(&prefix) {
                rest
            } else {
                continue;
            };

            if relative.is_empty() {
                continue;
            }

            // Split off the first path component
            if let Some(slash_pos) = relative.find('/') {
                // Entry lives in a sub-directory — record the immediate subdir
                let dir_name = &relative[..slash_pos];
                if seen_dirs.insert(dir_name.to_string()) {
                    entries.push(ToolFsDirEntry {
                        name: dir_name.to_string(),
                        is_dir: true,
                        size: 0,
                    });
                }
            } else {
                // Direct file child
                let size = files[key].len() as u64;
                entries.push(ToolFsDirEntry {
                    name: relative.to_string(),
                    is_dir: false,
                    size,
                });
            }
        }

        Ok(entries)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        let files = self.files.lock().unwrap();
        Ok(files.contains_key(path))
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a success exec result
    fn ok_exec(stdout: &str) -> ToolFsExecResult {
        ToolFsExecResult {
            stdout: stdout.to_string(),
            stderr: String::new(),
            exit_code: 0,
        }
    }

    // -----------------------------------------------------------------------
    // write_file / read_file roundtrip
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_read_write() {
        let fs = MockToolFs::new();

        let content = b"hello, world!";
        fs.write_file("foo.txt", content)
            .await
            .expect("write should succeed");

        let read_back = fs.read_file("foo.txt").await.expect("read should succeed");
        assert_eq!(read_back, content);
    }

    #[tokio::test]
    async fn test_mock_read_missing_returns_error() {
        let fs = MockToolFs::new();
        let err = fs.read_file("missing.txt").await.expect_err("should error");
        assert!(
            err.to_string().contains("file not found"),
            "unexpected error: {}",
            err
        );
    }

    // -----------------------------------------------------------------------
    // exec
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_exec() {
        let fs = MockToolFs::new().with_exec_response(ok_exec("output line"));

        let result = fs
            .exec("echo output line", None, 5000)
            .await
            .expect("exec should succeed");

        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout, "output line");
    }

    #[tokio::test]
    async fn test_mock_exec_fifo_order() {
        let fs = MockToolFs::new()
            .with_exec_response(ok_exec("first"))
            .with_exec_response(ok_exec("second"));

        let r1 = fs.exec("cmd1", None, 5000).await.expect("first exec");
        let r2 = fs.exec("cmd2", None, 5000).await.expect("second exec");

        assert_eq!(r1.stdout, "first");
        assert_eq!(r2.stdout, "second");
    }

    #[tokio::test]
    async fn test_mock_exec_empty_queue_returns_error() {
        let fs = MockToolFs::new();
        let err = fs.exec("cmd", None, 5000).await.expect_err("should error");
        assert!(
            err.to_string().contains("no more queued"),
            "unexpected error: {}",
            err
        );
    }

    // -----------------------------------------------------------------------
    // exists
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_exists() {
        let fs = MockToolFs::new().with_file("present.txt", b"data");

        assert!(
            fs.exists("present.txt").await.expect("exists check"),
            "file should exist"
        );
        assert!(
            !fs.exists("absent.txt").await.expect("exists check"),
            "file should not exist"
        );
    }

    // -----------------------------------------------------------------------
    // list_dir
    // -----------------------------------------------------------------------

    #[tokio::test]
    async fn test_mock_list_dir() {
        let fs = MockToolFs::new()
            .with_file("src/main.rs", b"fn main() {}")
            .with_file("src/lib.rs", b"pub mod foo;")
            .with_file("Cargo.toml", b"[package]");

        // List the src/ directory — should return main.rs and lib.rs
        let mut entries = fs.list_dir("src").await.expect("list_dir should succeed");
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["lib.rs", "main.rs"]);

        // None of them should be directories
        for entry in &entries {
            assert!(!entry.is_dir, "{} should not be a dir", entry.name);
        }
    }

    #[tokio::test]
    async fn test_mock_list_dir_subdirs() {
        let fs = MockToolFs::new()
            .with_file("a/b/file.txt", b"deep")
            .with_file("a/top.txt", b"top");

        // Listing "a" should yield "b" (subdir) and "top.txt" (file)
        let mut entries = fs.list_dir("a").await.expect("list_dir should succeed");
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"b"), "expected subdir 'b', got {:?}", names);
        assert!(
            names.contains(&"top.txt"),
            "expected 'top.txt', got {:?}",
            names
        );

        let b_entry = entries.iter().find(|e| e.name == "b").unwrap();
        assert!(b_entry.is_dir, "'b' should be a directory");
    }
}
