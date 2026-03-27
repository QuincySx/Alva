# Tool Sandbox Integration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make all file/exec tools sandbox-aware by introducing a `ToolFs` trait, so tools run identically on local FS, Docker, or cloud sandbox — no dual-path branching in tool code.

**Architecture:** Define `ToolFs` trait in alva-types (the shared vocabulary crate, zero new deps). Add `fn tool_fs(&self) -> Option<&dyn ToolFs>` to ToolContext with default `None`. Tools check `tool_fs()`: if `Some`, use it; if `None`, fall back to current local behavior. APP layer provides a `SandboxToolFs` that delegates to `dyn Sandbox`.

**Tech Stack:** Rust, async-trait, alva-types, alva-agent-tools, alva-sandbox (external repo)

---

## Design: Why ToolFs instead of Dual-Path

Dual-path (check sandbox, branch) requires every tool to have two code paths:
```rust
if let Some(sandbox) = get_sandbox(ctx) {
    sandbox.exec(cmd)   // path A
} else {
    Command::new(cmd)   // path B — duplicated logic
}
```

ToolFs gives tools ONE path:
```rust
let fs = ctx.tool_fs().unwrap_or(&local_fs);
fs.exec(cmd)  // works on any backend
```

No `alva-sandbox` dependency in alva-types or alva-agent-tools. The integration happens in alva-app-core.

## File Structure

```
Modify: crates/alva-types/src/tool.rs           — add ToolFs trait + tool_fs() method
Modify: crates/alva-agent-tools/src/execute_shell.rs    — use ToolFs
Modify: crates/alva-agent-tools/src/create_file.rs      — use ToolFs
Modify: crates/alva-agent-tools/src/file_edit.rs         — use ToolFs
Modify: crates/alva-agent-tools/src/grep_search.rs       — use ToolFs
Modify: crates/alva-agent-tools/src/list_files.rs        — use ToolFs
Modify: crates/alva-agent-tools/src/view_image.rs        — use ToolFs
No change: crates/alva-agent-tools/src/read_url.rs        (network only)
No change: crates/alva-agent-tools/src/internet_search.rs (network only)
No change: crates/alva-agent-tools/src/ask_human.rs       (human interaction)
```

---

### Task 1: Define ToolFs trait in alva-types

**Files:**
- Modify: `crates/alva-types/src/tool.rs`
- Modify: `crates/alva-types/src/lib.rs`

This task adds the `ToolFs` trait and a `tool_fs()` method to `ToolContext`. No tools change yet.

- [ ] **Step 1: Define ToolFs trait**

Add to `crates/alva-types/src/tool.rs`:

```rust
/// Abstract filesystem + command execution interface.
///
/// Tools call these methods instead of direct system APIs.
/// Implementations include:
/// - Local FS (tokio::fs + tokio::process)
/// - Sandbox delegate (alva-sandbox Sandbox trait)
/// - Mock (for testing)
#[async_trait]
pub trait ToolFs: Send + Sync {
    /// Execute a shell command. Returns (stdout, stderr, exit_code).
    async fn exec(
        &self,
        command: &str,
        cwd: Option<&str>,
        timeout_ms: u64,
    ) -> Result<ToolFsExecResult, AgentError>;

    /// Read a file's contents.
    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError>;

    /// Write content to a file (creates parent dirs as needed).
    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError>;

    /// List directory entries.
    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError>;

    /// Check if a path exists.
    async fn exists(&self, path: &str) -> Result<bool, AgentError>;
}

/// Result of ToolFs::exec().
#[derive(Debug, Clone)]
pub struct ToolFsExecResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Directory entry from ToolFs::list_dir().
#[derive(Debug, Clone)]
pub struct ToolFsDirEntry {
    pub name: String,
    pub is_dir: bool,
    pub size: u64,
}
```

- [ ] **Step 2: Add tool_fs() to ToolContext**

In the existing `ToolContext` trait:

```rust
pub trait ToolContext: Send + Sync {
    fn session_id(&self) -> &str;
    fn get_config(&self, key: &str) -> Option<String>;
    fn as_any(&self) -> &dyn Any;
    fn local(&self) -> Option<&dyn LocalToolContext> { None }
    /// Returns an abstract FS interface (sandbox, remote, or mock).
    /// When None, tools fall back to direct local operations.
    fn tool_fs(&self) -> Option<&dyn ToolFs> { None }
}
```

- [ ] **Step 3: Export new types from lib.rs**

Add to `crates/alva-types/src/lib.rs` re-exports:

```rust
pub use tool::{ToolFs, ToolFsExecResult, ToolFsDirEntry};
```

- [ ] **Step 4: Verify compilation**

Run: `cargo check -p alva-types -p alva-agent-tools -p alva-app-core`
Expected: PASS (no breakage, tool_fs() defaults to None)

- [ ] **Step 5: Commit**

```bash
git add crates/alva-types/
git commit -m "feat(alva-types): add ToolFs trait for sandbox-agnostic file operations"
```

---

### Task 2: Implement LocalToolFs

**Files:**
- Create: `crates/alva-agent-tools/src/local_fs.rs`
- Modify: `crates/alva-agent-tools/src/lib.rs`

A ToolFs implementation backed by the local OS. This is used as the fallback and for testing.

- [ ] **Step 1: Write tests for LocalToolFs**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn exec_echo() {
        let fs = LocalToolFs::new("/tmp");
        let result = fs.exec("echo hello", None, 5000).await.unwrap();
        assert_eq!(result.exit_code, 0);
        assert_eq!(result.stdout.trim(), "hello");
    }

    #[tokio::test]
    async fn exec_with_cwd() {
        let fs = LocalToolFs::new("/tmp");
        let result = fs.exec("pwd", Some("/usr"), 5000).await.unwrap();
        assert_eq!(result.stdout.trim(), "/usr");
    }

    #[tokio::test]
    async fn exec_timeout() {
        let fs = LocalToolFs::new("/tmp");
        let result = fs.exec("sleep 10", None, 100).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn write_and_read_file() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fs = LocalToolFs::new(tmp.path().to_str().unwrap());
        fs.write_file("test.txt", b"hello").await.unwrap();
        let content = fs.read_file("test.txt").await.unwrap();
        assert_eq!(content, b"hello");
    }

    #[tokio::test]
    async fn list_dir_and_exists() {
        let tmp = tempfile::TempDir::new().unwrap();
        let fs = LocalToolFs::new(tmp.path().to_str().unwrap());
        fs.write_file("a.txt", b"a").await.unwrap();
        assert!(fs.exists("a.txt").await.unwrap());
        assert!(!fs.exists("b.txt").await.unwrap());
        let entries = fs.list_dir(".").await.unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].name, "a.txt");
    }
}
```

- [ ] **Step 2: Implement LocalToolFs**

```rust
use std::path::{Path, PathBuf};
use alva_types::{AgentError, ToolFs, ToolFsExecResult, ToolFsDirEntry};
use async_trait::async_trait;

/// ToolFs backed by the local filesystem and process execution.
pub struct LocalToolFs {
    root: PathBuf,
}

impl LocalToolFs {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn resolve(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() { p.to_path_buf() } else { self.root.join(p) }
    }
}

#[async_trait]
impl ToolFs for LocalToolFs {
    async fn exec(&self, command: &str, cwd: Option<&str>, timeout_ms: u64) -> Result<ToolFsExecResult, AgentError> {
        let dir = cwd.map(|c| self.resolve(c)).unwrap_or_else(|| self.root.clone());
        let output = tokio::time::timeout(
            std::time::Duration::from_millis(timeout_ms),
            tokio::process::Command::new("sh")
                .arg("-c").arg(command)
                .current_dir(&dir)
                .output(),
        ).await
            .map_err(|_| AgentError::ToolError { tool: "exec".into(), message: format!("timed out after {timeout_ms}ms") })?
            .map_err(|e| AgentError::ToolError { tool: "exec".into(), message: e.to_string() })?;

        Ok(ToolFsExecResult {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            exit_code: output.status.code().unwrap_or(-1),
        })
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        tokio::fs::read(self.resolve(path)).await
            .map_err(|e| AgentError::ToolError { tool: "read_file".into(), message: e.to_string() })
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        let full = self.resolve(path);
        if let Some(parent) = full.parent() {
            tokio::fs::create_dir_all(parent).await
                .map_err(|e| AgentError::ToolError { tool: "write_file".into(), message: e.to_string() })?;
        }
        tokio::fs::write(full, content).await
            .map_err(|e| AgentError::ToolError { tool: "write_file".into(), message: e.to_string() })
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let full = self.resolve(path);
        let mut entries = Vec::new();
        let mut rd = tokio::fs::read_dir(&full).await
            .map_err(|e| AgentError::ToolError { tool: "list_dir".into(), message: e.to_string() })?;
        while let Some(entry) = rd.next_entry().await
            .map_err(|e| AgentError::ToolError { tool: "list_dir".into(), message: e.to_string() })? {
            let meta = entry.metadata().await
                .map_err(|e| AgentError::ToolError { tool: "list_dir".into(), message: e.to_string() })?;
            entries.push(ToolFsDirEntry {
                name: entry.file_name().to_string_lossy().into_owned(),
                is_dir: meta.is_dir(),
                size: meta.len(),
            });
        }
        Ok(entries)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        Ok(self.resolve(path).exists())
    }
}
```

- [ ] **Step 3: Export from lib.rs**

Add `pub mod local_fs;` and `pub use local_fs::LocalToolFs;` to `crates/alva-agent-tools/src/lib.rs`.

- [ ] **Step 4: Add tempfile dev-dependency if not present**

Check `crates/alva-agent-tools/Cargo.toml` for tempfile in dev-dependencies.

- [ ] **Step 5: Run tests**

Run: `cargo test -p alva-agent-tools -- local_fs`
Expected: all 5 tests PASS

- [ ] **Step 6: Commit**

```bash
git add crates/alva-agent-tools/
git commit -m "feat(alva-agent-tools): implement LocalToolFs for local filesystem"
```

---

### Task 3: Migrate simple tools (CreateFileTool, FileEditTool, ViewImageTool)

**Files:**
- Modify: `crates/alva-agent-tools/src/create_file.rs`
- Modify: `crates/alva-agent-tools/src/file_edit.rs`
- Modify: `crates/alva-agent-tools/src/view_image.rs`

These 3 tools only do read_file / write_file / exists. Straightforward migration.

**Pattern**: Each tool gets a helper to resolve the ToolFs:

```rust
fn get_fs<'a>(ctx: &'a dyn ToolContext, fallback: &'a dyn ToolFs) -> &'a dyn ToolFs {
    ctx.tool_fs().unwrap_or(fallback)
}
```

For the `fallback`, each tool creates a `LocalToolFs` from `ctx.local()?.workspace()`.

- [ ] **Step 1: Modify CreateFileTool**

Read current file first. Replace `tokio::fs::create_dir_all` + `tokio::fs::write` with `fs.write_file()` (which handles mkdir internally). Keep existing parameter parsing and path resolution.

Key change:
```rust
// Before:
tokio::fs::create_dir_all(&parent).await?;
tokio::fs::write(&file_path, &params.content).await?;

// After:
let local = ctx.local().ok_or_else(|| ...)?;
let fallback = LocalToolFs::new(local.workspace());
let fs = ctx.tool_fs().unwrap_or(&fallback);
fs.write_file(file_path.to_str().unwrap(), params.content.as_bytes()).await?;
```

- [ ] **Step 2: Modify FileEditTool**

Replace `tokio::fs::read_to_string` + `tokio::fs::write` with `fs.read_file()` + `fs.write_file()`.

Key change:
```rust
// Before:
let content = tokio::fs::read_to_string(&file_path).await?;
// ... string replacement ...
tokio::fs::write(&file_path, &new_content).await?;

// After:
let bytes = fs.read_file(file_path.to_str().unwrap()).await?;
let content = String::from_utf8_lossy(&bytes).into_owned();
// ... string replacement ...
fs.write_file(file_path.to_str().unwrap(), new_content.as_bytes()).await?;
```

- [ ] **Step 3: Modify ViewImageTool**

Replace `Path::exists()` + `tokio::fs::read` with `fs.exists()` + `fs.read_file()`.

- [ ] **Step 4: Run all tool tests**

Run: `cargo test -p alva-agent-tools`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/alva-agent-tools/src/create_file.rs crates/alva-agent-tools/src/file_edit.rs crates/alva-agent-tools/src/view_image.rs
git commit -m "feat(alva-agent-tools): migrate CreateFile/FileEdit/ViewImage to ToolFs"
```

---

### Task 4: Migrate ExecuteShellTool

**Files:**
- Modify: `crates/alva-agent-tools/src/execute_shell.rs`

ExecuteShellTool currently uses `tokio::process::Command`. Replace with `fs.exec()`.

- [ ] **Step 1: Read current implementation**

Understand: timeout handling, cwd resolution, stdout/stderr capture, exit code.

- [ ] **Step 2: Replace Command with ToolFs::exec()**

```rust
// Before:
let result = tokio::time::timeout(timeout, async {
    tokio::process::Command::new("sh")
        .arg("-c").arg(&params.command)
        .current_dir(&cwd)
        .output().await
}).await;

// After:
let local = ctx.local().ok_or_else(|| ...)?;
let fallback = LocalToolFs::new(local.workspace());
let fs = ctx.tool_fs().unwrap_or(&fallback);
let result = fs.exec(
    &params.command,
    params.cwd.as_deref(),
    params.timeout_secs.unwrap_or(30) as u64 * 1000,
).await?;
```

- [ ] **Step 3: Run tests**

Run: `cargo test -p alva-agent-tools`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-tools/src/execute_shell.rs
git commit -m "feat(alva-agent-tools): migrate ExecuteShellTool to ToolFs"
```

---

### Task 5: Migrate GrepSearchTool and ListFilesTool

**Files:**
- Modify: `crates/alva-agent-tools/src/grep_search.rs`
- Modify: `crates/alva-agent-tools/src/list_files.rs`

These are the most complex — they use `walkdir` for recursive directory traversal. With ToolFs, we need to implement recursive traversal using `list_dir()`.

- [ ] **Step 1: Add recursive list helper to LocalToolFs or as standalone function**

```rust
/// Recursively list all files under a directory via ToolFs.
pub async fn walk_dir(
    fs: &dyn ToolFs,
    root: &str,
    max_depth: Option<usize>,
    include_hidden: bool,
) -> Result<Vec<String>, AgentError> {
    let mut results = Vec::new();
    let mut stack: Vec<(String, usize)> = vec![(root.to_string(), 0)];

    while let Some((dir, depth)) = stack.pop() {
        if let Some(max) = max_depth {
            if depth > max { continue; }
        }
        let entries = fs.list_dir(&dir).await?;
        for entry in entries {
            if !include_hidden && entry.name.starts_with('.') { continue; }
            let full_path = format!("{}/{}", dir.trim_end_matches('/'), entry.name);
            if entry.is_dir {
                stack.push((full_path, depth + 1));
            } else {
                results.push(full_path);
            }
        }
    }
    Ok(results)
}
```

- [ ] **Step 2: Migrate ListFilesTool**

Replace `WalkDir::new()` + `spawn_blocking` with the async `walk_dir()` helper.

- [ ] **Step 3: Migrate GrepSearchTool**

Replace `WalkDir` + `std::fs::read_to_string` with `walk_dir()` + `fs.read_file()`. Keep regex matching and glob filtering logic unchanged.

- [ ] **Step 4: Run tests**

Run: `cargo test -p alva-agent-tools`
Expected: PASS

- [ ] **Step 5: Commit**

```bash
git add crates/alva-agent-tools/src/grep_search.rs crates/alva-agent-tools/src/list_files.rs crates/alva-agent-tools/src/local_fs.rs
git commit -m "feat(alva-agent-tools): migrate GrepSearch/ListFiles to ToolFs with async walk"
```

---

### Task 6: MockToolFs + integration tests

**Files:**
- Create: `crates/alva-agent-tools/src/mock_fs.rs`
- Modify: `crates/alva-agent-tools/src/lib.rs`

A mock ToolFs for testing tools without real filesystem access.

- [ ] **Step 1: Implement MockToolFs**

```rust
use std::collections::HashMap;
use std::sync::Mutex;
use alva_types::{AgentError, ToolFs, ToolFsExecResult, ToolFsDirEntry};
use async_trait::async_trait;

/// In-memory ToolFs for testing.
pub struct MockToolFs {
    files: Mutex<HashMap<String, Vec<u8>>>,
    exec_responses: Mutex<Vec<ToolFsExecResult>>,
}

impl MockToolFs {
    pub fn new() -> Self {
        Self {
            files: Mutex::new(HashMap::new()),
            exec_responses: Mutex::new(Vec::new()),
        }
    }

    pub fn with_file(self, path: &str, content: &[u8]) -> Self {
        self.files.lock().unwrap().insert(path.to_string(), content.to_vec());
        self
    }

    pub fn with_exec_response(self, result: ToolFsExecResult) -> Self {
        self.exec_responses.lock().unwrap().push(result);
        self
    }
}

#[async_trait]
impl ToolFs for MockToolFs {
    async fn exec(&self, _cmd: &str, _cwd: Option<&str>, _timeout_ms: u64) -> Result<ToolFsExecResult, AgentError> {
        self.exec_responses.lock().unwrap().pop()
            .ok_or_else(|| AgentError::ToolError { tool: "mock".into(), message: "no exec response queued".into() })
    }

    async fn read_file(&self, path: &str) -> Result<Vec<u8>, AgentError> {
        self.files.lock().unwrap().get(path).cloned()
            .ok_or_else(|| AgentError::ToolError { tool: "mock".into(), message: format!("file not found: {path}") })
    }

    async fn write_file(&self, path: &str, content: &[u8]) -> Result<(), AgentError> {
        self.files.lock().unwrap().insert(path.to_string(), content.to_vec());
        Ok(())
    }

    async fn list_dir(&self, path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
        let prefix = if path.ends_with('/') { path.to_string() } else { format!("{path}/") };
        let files = self.files.lock().unwrap();
        let entries: Vec<_> = files.keys()
            .filter(|k| k.starts_with(&prefix) && !k[prefix.len()..].contains('/'))
            .map(|k| ToolFsDirEntry {
                name: k[prefix.len()..].to_string(),
                is_dir: false,
                size: files[k].len() as u64,
            })
            .collect();
        Ok(entries)
    }

    async fn exists(&self, path: &str) -> Result<bool, AgentError> {
        Ok(self.files.lock().unwrap().contains_key(path))
    }
}
```

- [ ] **Step 2: Write tool tests using MockToolFs**

Test that CreateFileTool works with MockToolFs — verify it calls write_file.
Test that FileEditTool works — verify read → edit → write roundtrip.

- [ ] **Step 3: Run all tests**

Run: `cargo test -p alva-agent-tools`
Expected: PASS

- [ ] **Step 4: Commit**

```bash
git add crates/alva-agent-tools/
git commit -m "feat(alva-agent-tools): add MockToolFs for sandbox-free testing"
```

---

### Task 7: Final verification + cleanup

**Files:**
- Verify: all crates in workspace compile
- Verify: all tests pass

- [ ] **Step 1: Full workspace check**

Run: `cargo check --workspace`
Expected: PASS (ignoring pre-existing alva-app settings_model error)

- [ ] **Step 2: Full test suite**

Run: `cargo test -p alva-types -p alva-agent-tools -p alva-agent-core -p alva-app-core`
Expected: PASS

- [ ] **Step 3: Verify no direct fs/process calls remain in tools**

```bash
grep -n "tokio::fs::" crates/alva-agent-tools/src/*.rs
grep -n "tokio::process::" crates/alva-agent-tools/src/*.rs
grep -n "std::fs::" crates/alva-agent-tools/src/*.rs
```

Expected: Only in `local_fs.rs` (the fallback implementation). Zero in tool files.

- [ ] **Step 4: Commit any cleanup**

```bash
git commit -m "chore: verify tool sandbox integration complete"
```

---

## Summary

| Task | What | Tools affected | Commit |
|:---:|------|:-:|------|
| 1 | ToolFs trait in alva-types | 0 | trait definition |
| 2 | LocalToolFs implementation | 0 | local fallback |
| 3 | Simple tools migration | 3 | CreateFile, FileEdit, ViewImage |
| 4 | ExecuteShell migration | 1 | ExecuteShell |
| 5 | Recursive tools migration | 2 | GrepSearch, ListFiles |
| 6 | MockToolFs + tests | 0 | testing infrastructure |
| 7 | Final verification | 0 | cleanup |

After this, tools are fully sandbox-agnostic. APP layer just needs to provide a ToolContext where `tool_fs()` returns a `SandboxToolFs` wrapping `dyn alva_sandbox::Sandbox`.
