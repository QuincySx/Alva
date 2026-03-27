# Cross-Platform Portability Fixes

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Make the alva-agent framework crates compilable on wasm32-wasi / V8 targets by removing or feature-gating platform-specific dependencies.

**Architecture:** Replace `parking_lot` with `std::sync::Mutex` in agent-core. Feature-gate `dirs` in agent-security. Feature-gate native-only deps (tokio/process, tokio/fs, walkdir, rusqlite) in agent-tools, protocol-skill, protocol-mcp, and agent-runtime. Add `InMemorySkillRepository` and `McpConfigFile::from_str()` for non-native targets.

**Tech Stack:** Rust, Cargo features, `#[cfg]` conditional compilation

---

### Task 1: Remove parking_lot from alva-agent-core

**Files:**
- Modify: `crates/alva-agent-core/Cargo.toml`
- Modify: `crates/alva-agent-core/src/agent_loop.rs`
- Modify: `crates/alva-agent-core/src/middleware/mod.rs`

**Step 1: Remove parking_lot from Cargo.toml**

In `crates/alva-agent-core/Cargo.toml`, delete line 15:
```toml
parking_lot = "0.12"
```

**Step 2: Replace all parking_lot::Mutex with std::sync::Mutex in test code**

In `crates/alva-agent-core/src/agent_loop.rs`, replace:
```rust
observed: Arc<parking_lot::Mutex<Option<u32>>>,
```
with:
```rust
observed: Arc<std::sync::Mutex<Option<u32>>>,
```

And replace:
```rust
let observed = Arc::new(parking_lot::Mutex::new(None));
```
with:
```rust
let observed = Arc::new(std::sync::Mutex::new(None));
```

And replace all `.lock()` calls on these with `.lock().unwrap()`.

In `crates/alva-agent-core/src/middleware/mod.rs`, apply the same pattern to all 6 occurrences:
- Lines ~347, 351: `Arc<parking_lot::Mutex<Vec<String>>>` → `Arc<std::sync::Mutex<Vec<String>>>`
- Lines ~486, 490: same
- Lines ~600, 616: `Arc<parking_lot::Mutex<Option<u32>>>` → `Arc<std::sync::Mutex<Option<u32>>>`

Replace all `.lock()` with `.lock().unwrap()`.

**Step 3: Update lib.rs INPUT comment**

Remove `parking_lot` from the INPUT comment in `crates/alva-agent-core/src/lib.rs` line 1.

**Step 4: Verify**

Run: `cargo test -p alva-agent-core`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/alva-agent-core/
git commit -m "refactor(alva-agent-core): replace parking_lot with std::sync::Mutex for wasm portability"
```

---

### Task 2: Remove dirs from alva-agent-security, fix hardcoded /tmp

**Files:**
- Modify: `crates/alva-agent-security/Cargo.toml`
- Modify: `crates/alva-agent-security/src/sensitive_paths.rs`
- Modify: `crates/alva-agent-security/src/guard.rs`
- Modify: `crates/alva-agent-security/src/sandbox.rs`

**Step 1: Make dirs an optional dependency**

In `crates/alva-agent-security/Cargo.toml`, change:
```toml
dirs = "5"
```
to:
```toml
dirs = { version = "5", optional = true }
```

Add features section:
```toml
[features]
default = ["native"]
native = ["dirs"]
```

**Step 2: Conditionally compile dirs usage in sensitive_paths.rs**

Replace line 25:
```rust
let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
```
with:
```rust
let home = Self::resolve_home_dir();
```

Add helper method to `SensitivePathFilter`:
```rust
fn resolve_home_dir() -> PathBuf {
    #[cfg(feature = "native")]
    {
        dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"))
    }
    #[cfg(not(feature = "native"))]
    {
        PathBuf::from("/home/agent")
    }
}
```

**Step 3: Conditionally compile dirs usage in guard.rs**

Replace `dirs::home_dir()` calls at lines ~201 and ~236 with a helper:
```rust
fn expand_home(path: &str) -> PathBuf {
    #[cfg(feature = "native")]
    {
        dirs::home_dir().unwrap_or_default().join(path)
    }
    #[cfg(not(feature = "native"))]
    {
        PathBuf::from("/home/agent").join(path)
    }
}
```

Then replace:
```rust
dirs::home_dir().unwrap_or_default().join(rest)
```
with:
```rust
expand_home(rest)
```

**Step 4: Fix hardcoded /tmp in sandbox.rs**

In `crates/alva-agent-security/src/sandbox.rs`, replace lines ~131-135:
```rust
if let Ok(tmp) = std::env::var("TMPDIR") {
    config.add_writable_dir(std::path::PathBuf::from(tmp));
} else {
    config.add_writable_dir(std::path::PathBuf::from("/tmp"));
}
```
with:
```rust
config.add_writable_dir(std::env::temp_dir());
```

**Step 5: Update test cfg guards**

In `sensitive_paths.rs` tests, wrap dirs-dependent tests:
```rust
#[cfg(feature = "native")]
#[test]
fn blocks_gnupg_directory() {
    let filter = SensitivePathFilter::default_rules();
    let home = dirs::home_dir().unwrap();
    let path = home.join(".gnupg").join("pubring.kbx");
    assert!(filter.check(&path).is_some());
}

#[cfg(feature = "native")]
#[test]
fn blocks_ssh_directory() {
    let filter = SensitivePathFilter::default_rules();
    let home = dirs::home_dir().unwrap();
    let path = home.join(".ssh").join("config");
    assert!(filter.check(&path).is_some());
}
```

**Step 6: Verify**

Run: `cargo test -p alva-agent-security`
Expected: All tests pass

Run: `cargo check -p alva-agent-security --no-default-features`
Expected: Compiles without dirs

**Step 7: Commit**

```bash
git add crates/alva-agent-security/
git commit -m "refactor(alva-agent-security): feature-gate dirs crate, fix hardcoded /tmp"
```

---

### Task 3: Feature-gate native deps in alva-agent-tools

**Files:**
- Modify: `crates/alva-agent-tools/Cargo.toml`
- Modify: `crates/alva-agent-tools/src/lib.rs`

**Step 1: Restructure Cargo.toml dependencies**

Replace the current `[dependencies]` tokio line:
```toml
tokio = { version = "1", features = ["process", "time", "fs", "sync", "io-util"] }
```
with:
```toml
tokio = { version = "1", features = ["sync", "time"] }

[target.'cfg(not(target_family = "wasm"))'.dependencies]
tokio = { version = "1", features = ["process", "fs", "io-util"] }
```

Note: `reqwest` also needs gating:
```toml
reqwest = { version = "0.12", features = ["json", "stream"], optional = true }
```

Update features:
```toml
[features]
browser = ["chromiumoxide"]
native = ["reqwest"]
default = ["browser", "native"]
```

**Step 2: Gate LocalToolFs module in lib.rs**

In `crates/alva-agent-tools/src/lib.rs`, wrap local_fs with cfg:
```rust
#[cfg(not(target_family = "wasm"))]
pub mod local_fs;
#[cfg(not(target_family = "wasm"))]
pub use local_fs::{walk_dir, LocalToolFs};
```

Similarly gate any tools that directly depend on native capabilities (internet_search, read_url if they use reqwest):
```rust
#[cfg(feature = "native")]
pub mod internet_search;
#[cfg(feature = "native")]
pub mod read_url;
```

Update `register_builtin_tools` to conditionally register:
```rust
pub fn register_builtin_tools(registry: &mut ToolRegistry) {
    register_tools!(
        registry,
        execute_shell::ExecuteShellTool,
        create_file::CreateFileTool,
        file_edit::FileEditTool,
        grep_search::GrepSearchTool,
        list_files::ListFilesTool,
        ask_human::AskHumanTool,
        view_image::ViewImageTool,
    );

    #[cfg(feature = "native")]
    register_tools!(
        registry,
        internet_search::InternetSearchTool,
        read_url::ReadUrlTool,
    );
}
```

**Step 3: Verify**

Run: `cargo test -p alva-agent-tools`
Expected: All tests pass

Run: `cargo check -p alva-agent-tools --no-default-features`
Expected: Compiles (tools that use ToolFs still work, LocalToolFs excluded)

**Step 4: Commit**

```bash
git add crates/alva-agent-tools/
git commit -m "refactor(alva-agent-tools): feature-gate native-only deps (process, fs, reqwest)"
```

---

### Task 4: Add InMemorySkillRepository to alva-protocol-skill

**Files:**
- Create: `crates/alva-protocol-skill/src/memory.rs`
- Modify: `crates/alva-protocol-skill/src/lib.rs`
- Modify: `crates/alva-protocol-skill/Cargo.toml`

**Step 1: Feature-gate fs deps in Cargo.toml**

Replace:
```toml
walkdir = "2"
tokio = { version = "1", features = ["fs", "sync"] }
```
with:
```toml
walkdir = { version = "2", optional = true }
tokio = { version = "1", features = ["sync"] }

[target.'cfg(not(target_family = "wasm"))'.dependencies]
tokio = { version = "1", features = ["fs"] }

[features]
default = ["fs"]
fs = ["walkdir"]
```

**Step 2: Gate FsSkillRepository module**

In `crates/alva-protocol-skill/src/lib.rs`, change:
```rust
pub mod fs;
```
to:
```rust
#[cfg(feature = "fs")]
pub mod fs;
pub mod memory;
```

**Step 3: Write test for InMemorySkillRepository**

Create `crates/alva-protocol-skill/src/memory.rs`:
```rust
// INPUT:  crate::types, crate::error, crate::repository, async_trait, std::sync
// OUTPUT: InMemorySkillRepository
// POS:    In-memory SkillRepository for wasm/V8 targets where filesystem is unavailable.
use std::sync::Arc;
use tokio::sync::RwLock;

use async_trait::async_trait;

use crate::{
    error::SkillError,
    repository::{SkillInstallSource, SkillRepository},
    types::{Skill, SkillBody, SkillKind, SkillMeta, SkillResource},
};

/// In-memory [`SkillRepository`] for environments without filesystem access.
///
/// Skills are provided at construction time (e.g. compiled-in, injected by host,
/// or fetched from an API). No disk I/O is performed.
pub struct InMemorySkillRepository {
    skills: Arc<RwLock<Vec<InMemorySkill>>>,
}

/// A skill stored entirely in memory.
pub struct InMemorySkill {
    pub meta: SkillMeta,
    pub kind: SkillKind,
    pub body: String,
    pub resources: Vec<(String, Vec<u8>)>,
    pub enabled: bool,
}

impl InMemorySkillRepository {
    /// Create from a pre-built list of skills.
    pub fn new(skills: Vec<InMemorySkill>) -> Self {
        Self {
            skills: Arc::new(RwLock::new(skills)),
        }
    }

    /// Create an empty repository.
    pub fn empty() -> Self {
        Self::new(vec![])
    }
}

#[async_trait]
impl SkillRepository for InMemorySkillRepository {
    async fn list_skills(&self) -> Result<Vec<Skill>, SkillError> {
        let skills = self.skills.read().await;
        Ok(skills
            .iter()
            .map(|s| Skill {
                meta: s.meta.clone(),
                kind: s.kind.clone(),
                root_path: std::path::PathBuf::new(), // no filesystem path
                enabled: s.enabled,
            })
            .collect())
    }

    async fn find_skill(&self, name: &str) -> Result<Option<Skill>, SkillError> {
        let skills = self.list_skills().await?;
        Ok(skills.into_iter().find(|s| s.meta.name == name))
    }

    async fn load_body(&self, name: &str) -> Result<SkillBody, SkillError> {
        let skills = self.skills.read().await;
        let skill = skills
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        let markdown = skill.body.clone();
        let estimated_tokens = (markdown.len() / 4) as u32;
        Ok(SkillBody {
            markdown,
            estimated_tokens,
        })
    }

    async fn load_resource(
        &self,
        name: &str,
        relative_path: &str,
    ) -> Result<SkillResource, SkillError> {
        let skills = self.skills.read().await;
        let skill = skills
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        let (_, content) = skill
            .resources
            .iter()
            .find(|(p, _)| p == relative_path)
            .ok_or_else(|| {
                SkillError::Io(format!("resource not found: {relative_path}"))
            })?;

        Ok(SkillResource {
            relative_path: relative_path.to_string(),
            content: content.clone(),
            content_type: crate::types::ResourceContentType::Markdown,
        })
    }

    async fn list_resources(&self, name: &str) -> Result<Vec<String>, SkillError> {
        let skills = self.skills.read().await;
        let skill = skills
            .iter()
            .find(|s| s.meta.name == name)
            .ok_or_else(|| SkillError::SkillNotFound(name.to_string()))?;

        Ok(skill.resources.iter().map(|(p, _)| p.clone()).collect())
    }

    async fn install(&self, _source: SkillInstallSource) -> Result<SkillMeta, SkillError> {
        Err(SkillError::Io(
            "install not supported in InMemorySkillRepository".to_string(),
        ))
    }

    async fn remove(&self, _name: &str) -> Result<(), SkillError> {
        Err(SkillError::Io(
            "remove not supported in InMemorySkillRepository".to_string(),
        ))
    }

    async fn set_enabled(&self, name: &str, enabled: bool) -> Result<(), SkillError> {
        let mut skills = self.skills.write().await;
        if let Some(skill) = skills.iter_mut().find(|s| s.meta.name == name) {
            skill.enabled = enabled;
            Ok(())
        } else {
            Err(SkillError::SkillNotFound(name.to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_skill(name: &str, body: &str) -> InMemorySkill {
        InMemorySkill {
            meta: SkillMeta {
                name: name.to_string(),
                description: format!("{name} skill"),
                ..Default::default()
            },
            kind: SkillKind::Bundled,
            body: body.to_string(),
            resources: vec![],
            enabled: true,
        }
    }

    #[tokio::test]
    async fn list_returns_all_skills() {
        let repo = InMemorySkillRepository::new(vec![
            make_skill("tdd", "# TDD\nWrite tests first."),
            make_skill("debug", "# Debug\nSystematic debugging."),
        ]);
        let skills = repo.list_skills().await.unwrap();
        assert_eq!(skills.len(), 2);
    }

    #[tokio::test]
    async fn find_by_name() {
        let repo = InMemorySkillRepository::new(vec![
            make_skill("tdd", "# TDD"),
        ]);
        assert!(repo.find_skill("tdd").await.unwrap().is_some());
        assert!(repo.find_skill("nonexistent").await.unwrap().is_none());
    }

    #[tokio::test]
    async fn load_body_returns_markdown() {
        let repo = InMemorySkillRepository::new(vec![
            make_skill("tdd", "# TDD\nWrite tests first."),
        ]);
        let body = repo.load_body("tdd").await.unwrap();
        assert_eq!(body.markdown, "# TDD\nWrite tests first.");
    }

    #[tokio::test]
    async fn load_body_not_found() {
        let repo = InMemorySkillRepository::empty();
        assert!(repo.load_body("missing").await.is_err());
    }

    #[tokio::test]
    async fn set_enabled_toggles() {
        let repo = InMemorySkillRepository::new(vec![
            make_skill("tdd", "# TDD"),
        ]);
        repo.set_enabled("tdd", false).await.unwrap();
        let skills = repo.list_skills().await.unwrap();
        assert!(!skills[0].enabled);
    }
}
```

**Step 4: Verify**

Run: `cargo test -p alva-protocol-skill`
Expected: All tests pass (including new InMemorySkillRepository tests)

Run: `cargo check -p alva-protocol-skill --no-default-features`
Expected: Compiles without fs/walkdir

**Step 5: Commit**

```bash
git add crates/alva-protocol-skill/
git commit -m "feat(alva-protocol-skill): add InMemorySkillRepository, feature-gate filesystem deps"
```

---

### Task 5: Add McpConfigFile::from_str() and feature-gate fs in alva-protocol-mcp

**Files:**
- Modify: `crates/alva-protocol-mcp/Cargo.toml`
- Modify: `crates/alva-protocol-mcp/src/config.rs`

**Step 1: Feature-gate tokio/fs and tokio/process**

In `crates/alva-protocol-mcp/Cargo.toml`, replace:
```toml
tokio = { version = "1", features = ["sync", "rt", "process", "io-util", "time", "fs"] }
```
with:
```toml
tokio = { version = "1", features = ["sync", "rt", "time"] }

[target.'cfg(not(target_family = "wasm"))'.dependencies]
tokio = { version = "1", features = ["process", "io-util", "fs"] }

[features]
default = ["native"]
native = []
```

**Step 2: Add from_str constructor and gate load/save**

In `crates/alva-protocol-mcp/src/config.rs`, add:
```rust
impl McpConfigFile {
    /// Parse config from a JSON string. Works on all platforms.
    pub fn from_str(json: &str) -> Result<Self, McpError> {
        serde_json::from_str(json)
            .map_err(|e| McpError::Serialization(format!("Invalid config JSON: {e}")))
    }
```

Gate `load` and `save` methods:
```rust
    /// Load config from a specific path. Returns empty config if file doesn't exist.
    /// Only available on native platforms with filesystem access.
    #[cfg(not(target_family = "wasm"))]
    pub async fn load(path: &Path) -> Result<Self, McpError> {
        // ... existing code unchanged
    }

    /// Save config to a specific path.
    /// Only available on native platforms with filesystem access.
    #[cfg(not(target_family = "wasm"))]
    pub async fn save(&self, path: &Path) -> Result<(), McpError> {
        // ... existing code unchanged
    }
```

**Step 3: Verify**

Run: `cargo test -p alva-protocol-mcp`
Expected: All tests pass

Run: `cargo check -p alva-protocol-mcp --no-default-features`
Expected: Compiles without fs/process

**Step 4: Commit**

```bash
git add crates/alva-protocol-mcp/
git commit -m "refactor(alva-protocol-mcp): add from_str config, feature-gate filesystem and process deps"
```

---

### Task 6: Feature-gate native deps in alva-agent-runtime

**Files:**
- Modify: `crates/alva-agent-runtime/Cargo.toml`

**Step 1: Make heavy deps optional**

Replace current dependencies:
```toml
alva-agent-tools = { path = "../alva-agent-tools" }
alva-agent-security = { path = "../alva-agent-security" }
alva-agent-memory = { path = "../alva-agent-memory" }
```
with:
```toml
alva-agent-tools = { path = "../alva-agent-tools", default-features = false }
alva-agent-security = { path = "../alva-agent-security", default-features = false }
alva-agent-memory = { path = "../alva-agent-memory", optional = true }
```

Update features:
```toml
[features]
default = ["native"]
native = [
    "alva-agent-tools/default",
    "alva-agent-security/native",
    "alva-agent-memory",
]
browser = ["alva-agent-tools/browser"]
```

**Step 2: Gate memory-dependent code in builder.rs and middleware/**

Wrap any `use alva_agent_memory::` with `#[cfg(feature = "native")]` and make memory registration conditional in the builder.

**Step 3: Verify**

Run: `cargo test -p alva-agent-runtime`
Expected: All tests pass

Run: `cargo check -p alva-agent-runtime --no-default-features`
Expected: Compiles without native deps

**Step 4: Commit**

```bash
git add crates/alva-agent-runtime/
git commit -m "refactor(alva-agent-runtime): feature-gate memory, security native, and browser deps"
```

---

### Task 7: Full workspace verification

**Step 1: Verify default build (native)**

Run: `cargo check --workspace`
Expected: No errors

**Step 2: Run all tests**

Run: `cargo test --workspace`
Expected: All tests pass

**Step 3: Verify no-default-features builds for key crates**

Run:
```bash
cargo check -p alva-agent-core && \
cargo check -p alva-agent-tools --no-default-features && \
cargo check -p alva-agent-security --no-default-features && \
cargo check -p alva-protocol-skill --no-default-features && \
cargo check -p alva-protocol-mcp --no-default-features && \
cargo check -p alva-agent-runtime --no-default-features
```
Expected: All compile

**Step 4: Commit (if any fixes needed)**

```bash
git add -A
git commit -m "fix: resolve workspace-wide build issues from feature gating"
```

---

## Summary

| Task | Crate | What | Priority |
|------|-------|------|----------|
| 1 | alva-agent-core | Replace parking_lot → std::sync::Mutex | P0 |
| 2 | alva-agent-security | Feature-gate dirs, fix /tmp | P1 |
| 3 | alva-agent-tools | Feature-gate tokio/process, fs, reqwest | P1 |
| 4 | alva-protocol-skill | Add InMemorySkillRepository, gate fs | P1 |
| 5 | alva-protocol-mcp | Add from_str config, gate fs/process | P1 |
| 6 | alva-agent-runtime | Feature-gate memory/security/browser | P1 |
| 7 | workspace | Full verification | P1 |

After this, the core crates (`alva-types`, `alva-agent-core`, `alva-agent-graph`, `alva-engine-runtime`) are fully portable. Subsystem crates compile on all platforms with `--no-default-features`, native features opt-in via `default = ["native"]`.
