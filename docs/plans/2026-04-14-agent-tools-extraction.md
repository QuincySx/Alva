## Agent Tools Extraction & Extension Layer Refactor Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Dissolve the `alva-agent-tools` crate. Split its contents across three new homes: `alva-agent-core` (new crate) for the Extension trait machinery + `MockToolFs`, `alva-host-native` for `LocalToolFs`, and `alva-agent-extension-builtin` (new crate) for all tool implementations and the built-in Extension wrappers. After this refactor, `alva-agent-tools` is deleted.

**Architecture:**
Today, `alva-agent-tools` is a legacy dumping ground that predates the Extension system. It mixes (a) `ToolFs` adapters (`LocalToolFs`, `MockToolFs`), (b) tool implementations (from `read_file` to `internet_search`), and (c) provides tool presets consumed by thin Extension wrappers in `alva-app-core/src/extension/`. After the refactor: `alva-agent-core` owns the Extension trait + runtime machinery + `MockToolFs` (pure in-memory, zero deps), `alva-host-native` owns `LocalToolFs` (the real-OS adapter), and `alva-agent-extension-builtin` owns every tool implementation, grouped by Cargo feature (`core`, `web`, `notebook`, `worktree`, `team`, `task`, `utility`, `schedule`). `alva-app-core` keeps `BaseAgent` and the protocol extensions (skills/mcp/hooks/evaluation/agent_spawn) — only its `extension/` infrastructure is extracted.

**Tech Stack:** Rust 1.80+, Cargo workspaces, tokio, async-trait. No new external dependencies.

**Non-goals for this plan:**
- Moving `BaseAgent` out of `alva-app-core` — it currently has hard-coded `alva-app-extension-memory` and `alva-host-native` imports that require a separate refactor.
- Renaming `alva-app-core` — it remains the app orchestration layer.
- Touching `alva-app-extension-browser` / `alva-app-extension-memory` — they already live at the correct layer.

---

## Phase 1 — Create `alva-agent-core` skeleton

### Task 1.1: Create the new crate directory and Cargo.toml

**Files:**
- Create: `crates/alva-agent-core/Cargo.toml`
- Create: `crates/alva-agent-core/src/lib.rs` (empty placeholder)

**Step 1: Create directory and files**

```bash
mkdir -p crates/alva-agent-core/src
```

Write `crates/alva-agent-core/Cargo.toml`:

```toml
[package]
name = "alva-agent-core"
version = "0.1.0"
edition = "2021"
description = "Agent-layer core: Extension trait, HostAPI, event dispatch, MockToolFs"

[dependencies]
alva-kernel-abi = { path = "../alva-kernel-abi" }
alva-kernel-core = { path = "../alva-kernel-core" }
async-trait = "0.1"
tracing = "0.1"
tokio = { version = "1", default-features = false, features = ["sync"] }

[dev-dependencies]
tokio = { version = "1", features = ["macros", "rt"] }
```

Write `crates/alva-agent-core/src/lib.rs`:

```rust
//! Agent-layer core: the Extension system and test-grade ToolFs.
//!
//! This crate holds the pure agent-internal extension machinery that used
//! to live inside `alva-app-core/src/extension/`, plus `MockToolFs` which
//! used to live in `alva-agent-tools`. It deliberately does NOT depend on
//! any protocol crate, LLM provider, persistence, or host-specific code.

pub mod extension;
pub mod mock_fs;

pub use extension::{
    Extension, ExtensionBridgeMiddleware, ExtensionContext, ExtensionEvent, ExtensionHost,
    EventResult, FinalizeContext, HostAPI, RegisteredCommand,
};
pub use mock_fs::MockToolFs;
```

**Step 2: Register in workspace**

Edit `Cargo.toml` at the workspace root, add `"crates/alva-agent-core",` to `[workspace] members`. Keep the list alphabetically sorted with the surrounding `alva-agent-*` entries.

**Step 3: Verify the empty crate compiles**

Run: `cargo check -p alva-agent-core`
Expected: error about missing modules `extension` and `mock_fs`. Leave as-is; next task fills them.

**Step 4: Commit**

```bash
git add crates/alva-agent-core Cargo.toml
git commit -m "scaffold: create alva-agent-core crate"
```

---

### Task 1.2: Move `MockToolFs` from `alva-agent-tools`

**Files:**
- Create: `crates/alva-agent-core/src/mock_fs.rs` (copy of `crates/alva-agent-tools/src/mock_fs.rs`)
- Modify: `crates/alva-agent-tools/src/lib.rs:18` (drop `pub mod mock_fs;` and the re-export at line 75)
- Delete: `crates/alva-agent-tools/src/mock_fs.rs`

**Step 1: Copy file**

```bash
cp crates/alva-agent-tools/src/mock_fs.rs crates/alva-agent-core/src/mock_fs.rs
```

**Step 2: Verify `alva-agent-core` compiles**

First comment out the `pub mod extension;` line in `crates/alva-agent-core/src/lib.rs` temporarily, plus the `pub use extension::*` block.

Run: `cargo check -p alva-agent-core`
Expected: PASS. `MockToolFs` only needs `alva-kernel-abi` + `async_trait` + std.

**Step 3: Delete old copy**

```bash
rm crates/alva-agent-tools/src/mock_fs.rs
```

Edit `crates/alva-agent-tools/src/lib.rs`:
- Remove `pub mod mock_fs;`
- Remove `pub use mock_fs::MockToolFs;`

**Step 4: Find all consumers of `alva_agent_tools::MockToolFs`**

Run: `rg -l 'alva_agent_tools::(MockToolFs|mock_fs)' --glob '*.rs'`

For each hit, update import to `alva_agent_core::MockToolFs`. Corresponding Cargo.toml dev-dependencies also need `alva-agent-core = { path = "..." }` added if not already present.

**Step 5: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: PASS (MockToolFs-only consumers now resolve against the new crate).

**Step 6: Commit**

```bash
git add -A
git commit -m "refactor: relocate MockToolFs to alva-agent-core"
```

---

### Task 1.3: Move Extension trait + HostAPI + events + bridge + context

**Files:**
- Create: `crates/alva-agent-core/src/extension/mod.rs` — holds the `Extension` trait and public re-exports
- Create: `crates/alva-agent-core/src/extension/host.rs` — copy of `crates/alva-app-core/src/extension/host.rs`
- Create: `crates/alva-agent-core/src/extension/bridge.rs` — copy of `crates/alva-app-core/src/extension/bridge.rs`
- Create: `crates/alva-agent-core/src/extension/context.rs` — copy of `crates/alva-app-core/src/extension/context.rs`
- Create: `crates/alva-agent-core/src/extension/events.rs` — copy of `crates/alva-app-core/src/extension/events.rs`

**Step 1: Copy files over**

```bash
mkdir -p crates/alva-agent-core/src/extension
cp crates/alva-app-core/src/extension/{host,bridge,context,events}.rs crates/alva-agent-core/src/extension/
```

**Step 2: Write minimal `mod.rs` holding just the trait**

Write `crates/alva-agent-core/src/extension/mod.rs` — contains the `Extension` trait and public re-exports. Copy lines 83–111 (the `use` imports and `Extension` trait body) out of `crates/alva-app-core/src/extension/mod.rs`. Do **not** import any of the built-in extension modules (`web`, `task`, `browser`, etc.) — those stay in `alva-app-core` for now.

Concrete content:

```rust
//! Extension system — the primary extensibility point for agents.
//!
//! Contains only the Extension trait + dispatch machinery. Built-in
//! Extension implementations (file-io, shell, task, team, web, etc.) live
//! in `alva-agent-extension-builtin`. App-layer protocol extensions
//! (skills, mcp, hooks, evaluation, agent_spawn) live in `alva-app-core`.

mod bridge;
mod context;
mod events;
mod host;

pub use bridge::ExtensionBridgeMiddleware;
pub use context::{ExtensionContext, FinalizeContext};
pub use events::{EventResult, ExtensionEvent};
pub use host::{ExtensionHost, HostAPI, RegisteredCommand};

use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    fn activate(&self, _api: &HostAPI) {}
    async fn configure(&self, _ctx: &ExtensionContext) {}
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
```

**Step 3: Update the `super::` imports in copied files**

In `host.rs`, `bridge.rs`, `context.rs`, `events.rs`, change any `super::events::` / `super::host::` etc. to local `super::` relative to the new `extension/` module. They should already be relative — scan and fix any absolute `crate::extension::` imports to `crate::extension::` (same path survives because the new lib.rs has `pub mod extension`).

**Step 4: Re-enable `pub mod extension;` in `crates/alva-agent-core/src/lib.rs`**

**Step 5: Verify compilation**

Run: `cargo check -p alva-agent-core`
Expected: PASS. If any imports are missing (e.g. `alva_kernel_core::pending_queue::PendingMessageQueue`), they should resolve through the existing `alva-kernel-core` dependency.

**Step 6: Commit**

```bash
git add -A
git commit -m "refactor: extract Extension trait and HostAPI into alva-agent-core"
```

---

### Task 1.4: Point `alva-app-core` at the new `alva-agent-core`

**Files:**
- Modify: `crates/alva-app-core/Cargo.toml` — add dependency
- Modify: `crates/alva-app-core/src/extension/mod.rs` — re-export from agent-core instead of owning these modules
- Delete: `crates/alva-app-core/src/extension/host.rs`
- Delete: `crates/alva-app-core/src/extension/bridge.rs`
- Delete: `crates/alva-app-core/src/extension/context.rs`
- Delete: `crates/alva-app-core/src/extension/events.rs`
- Modify: `crates/alva-app-core/src/lib.rs:69` — update `pub use crate::extension::{ExtensionEvent, ...}` to still work

**Step 1: Add dependency in `crates/alva-app-core/Cargo.toml`**

Add under `[dependencies]`:

```toml
alva-agent-core = { path = "../alva-agent-core" }
```

Place it just after `alva-kernel-core = ...` so agent-core appears right after kernel deps in the section.

**Step 2: Rewrite `crates/alva-app-core/src/extension/mod.rs`**

The file currently declares `mod context; mod events; mod host; mod bridge;` and has the `Extension` trait body. Replace those declarations with re-exports from `alva_agent_core::extension`:

```rust
//! Extension system — re-exported from `alva-agent-core`.
//!
//! The trait + dispatch machinery moved to `alva-agent-core`. Only the
//! built-in Extension implementations (skills, mcp, hooks, etc.) still
//! live in this crate.

pub use alva_agent_core::extension::{
    Extension, ExtensionBridgeMiddleware, ExtensionContext, ExtensionEvent, ExtensionHost,
    EventResult, FinalizeContext, HostAPI, RegisteredCommand,
};

pub mod skills;
pub mod mcp;
pub mod hooks;
pub mod evaluation;
pub mod agent_spawn;

mod core;
mod shell;
mod interaction;
mod task;
mod team;
mod planning;
mod utility;
mod web;
mod browser;
mod loop_detection;
mod dangling_tool_call;
mod tool_timeout;
mod compaction;
mod checkpoint;
mod plan_mode;
mod analytics;
mod auth;
mod lsp;

pub use skills::SkillsExtension;
pub use mcp::McpExtension;
pub use hooks::HooksExtension;
pub use evaluation::EvaluationExtension;
pub use agent_spawn::{ChildRunRecording, SubAgentExtension};

pub use core::CoreExtension;
pub use shell::ShellExtension;
pub use interaction::InteractionExtension;
pub use task::TaskExtension;
pub use team::TeamExtension;
pub use planning::PlanningExtension;
pub use utility::UtilityExtension;
pub use web::WebExtension;
pub use browser::BrowserExtension;
pub use loop_detection::LoopDetectionExtension;
pub use dangling_tool_call::DanglingToolCallExtension;
pub use tool_timeout::ToolTimeoutExtension;
pub use compaction::CompactionExtension;
pub use checkpoint::CheckpointExtension;
pub use plan_mode::PlanModeExtension;
pub use analytics::AnalyticsExtension;
pub use auth::AuthExtension;
pub use lsp::LspExtension;
```

**Step 3: Delete the four copied-out files**

```bash
rm crates/alva-app-core/src/extension/{host,bridge,context,events}.rs
```

**Step 4: Fix inter-file imports inside `alva-app-core/src/extension/`**

Files like `skills/mod.rs`, `mcp/mod.rs`, `agent_spawn.rs`, `hooks/mod.rs`, and each built-in extension wrapper probably use `super::host::ExtensionHost`, `super::events::ExtensionEvent`, `super::Extension`, etc. Find them:

```bash
rg -l 'super::(Extension|HostAPI|ExtensionEvent|EventResult|ExtensionHost|ExtensionContext|FinalizeContext|ExtensionBridgeMiddleware|RegisteredCommand)' crates/alva-app-core/src/extension
```

For each hit, the re-exports in the new `mod.rs` resolve the same paths — they should continue to compile without changes. Run `cargo check -p alva-app-core` and fix any leftover import errors that reference `super::host::` or `super::events::` directly (change them to `super::`).

**Step 5: Verify workspace compiles**

Run: `cargo check --workspace`
Expected: PASS.

**Step 6: Run existing extension tests**

Run: `cargo test -p alva-app-core --tests`
Expected: PASS.

**Step 7: Commit**

```bash
git add -A
git commit -m "refactor: re-export Extension trait from alva-agent-core in app-core"
```

---

## Phase 2 — Create `alva-agent-extension-builtin` skeleton

### Task 2.1: Create the new crate

**Files:**
- Create: `crates/alva-agent-extension-builtin/Cargo.toml`
- Create: `crates/alva-agent-extension-builtin/src/lib.rs`

**Step 1: Create directory and files**

```bash
mkdir -p crates/alva-agent-extension-builtin/src
```

Write `crates/alva-agent-extension-builtin/Cargo.toml`:

```toml
[package]
name = "alva-agent-extension-builtin"
version = "0.1.0"
edition = "2021"
description = "Built-in agent extensions: tool implementations and Extension wrappers (file I/O, shell, web, notebook, worktree, team, task, utility)"

[dependencies]
alva-kernel-abi = { path = "../alva-kernel-abi" }
alva-agent-core = { path = "../alva-agent-core" }
async-trait = "0.1"
tokio = { version = "1", default-features = false, features = ["sync", "time"] }
futures = "0.3"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
schemars = "1"
regex = "1"
glob = "0.3"
tracing = "0.1"
base64 = "0.22"

# Native-only: process + fs + walkdir
[target.'cfg(not(target_family = "wasm"))'.dependencies]
tokio = { version = "1", features = ["process", "fs", "io-util", "rt"] }
ignore = "0.4"
reqwest = { version = "0.12", features = ["json", "stream"], optional = true }

[dev-dependencies]
tempfile = "3"

[features]
default = ["core", "utility"]

# Always-on fundamentals
core = []          # file_io + shell + interaction + plan_mode primitives
utility = []       # sleep / config / skill / tool_search / view_image

# Opt-in bundles
web = ["dep:reqwest"]    # internet_search + read_url (native only by target gating)
notebook = []            # notebook_edit
worktree = []            # enter/exit_worktree
team = []                # team_create/delete + send_message
task = []                # task_create/update/get/list/output/stop
schedule = []            # schedule_cron + remote_trigger
```

Write `crates/alva-agent-extension-builtin/src/lib.rs`:

```rust
//! Built-in agent extensions.
//!
//! This crate consolidates every reference tool implementation (formerly
//! in `alva-agent-tools`) and every thin Extension wrapper (formerly in
//! `alva-app-core/src/extension/*.rs`). Callers compose only the features
//! they want. Heavy domain extensions (browser, memory) live in separate
//! `alva-app-extension-*` crates because they pull app-level concerns.

// Tool implementations and Extension wrappers are added task-by-task
// during the rest of the refactor.
```

**Step 2: Add to workspace**

Edit workspace root `Cargo.toml` members list, add `"crates/alva-agent-extension-builtin",` alphabetically among `alva-agent-*`.

**Step 3: Verify**

Run: `cargo check -p alva-agent-extension-builtin`
Expected: PASS (empty lib).

**Step 4: Commit**

```bash
git add -A
git commit -m "scaffold: create alva-agent-extension-builtin crate"
```

---

## Phase 3 — Migrate tool implementations into `alva-agent-extension-builtin`

### Task 3.1: Move support modules (`truncate`, `walk_dir` helpers)

**Files:**
- Create: `crates/alva-agent-extension-builtin/src/truncate.rs` (copy)
- Create: `crates/alva-agent-extension-builtin/src/walkdir.rs` (extract walk helpers from `local_fs.rs`)

**Step 1: Copy `truncate.rs`**

```bash
cp crates/alva-agent-tools/src/truncate.rs crates/alva-agent-extension-builtin/src/truncate.rs
```

Add `pub mod truncate;` at the top of `crates/alva-agent-extension-builtin/src/lib.rs`.

**Step 2: Extract `walk_dir` / `walk_dir_filtered` into `walkdir.rs`**

Open `crates/alva-agent-tools/src/local_fs.rs`. Find the two free functions `walk_dir` and `walk_dir_filtered` (exported at `crates/alva-agent-tools/src/lib.rs:73`). Copy them into a new file `crates/alva-agent-extension-builtin/src/walkdir.rs`. Keep the `ignore = "0.4"` dep resolution since extension-builtin already declares it.

Add `#[cfg(not(target_family = "wasm"))] pub mod walkdir;` to `crates/alva-agent-extension-builtin/src/lib.rs`.

**Step 3: Verify**

Run: `cargo check -p alva-agent-extension-builtin`
Expected: PASS.

**Step 4: Commit**

```bash
git add -A
git commit -m "refactor: move truncate + walkdir helpers into extension-builtin"
```

---

### Task 3.2: Migrate `core` feature tools (file I/O, shell, interaction, plan primitives)

**Files moved (each is a simple copy + update internal imports):**
- `read_file.rs`, `create_file.rs`, `file_edit.rs`, `list_files.rs`, `find_files.rs`, `grep_search.rs`
- `execute_shell.rs`
- `ask_human.rs`
- `view_image.rs`
- `enter_plan_mode.rs`, `exit_plan_mode.rs`, `todo_write.rs`
- `agent_tool.rs` (the placeholder)

**Step 1: Copy files**

```bash
for f in read_file create_file file_edit list_files find_files grep_search \
         execute_shell ask_human view_image \
         enter_plan_mode exit_plan_mode todo_write agent_tool; do
  cp "crates/alva-agent-tools/src/${f}.rs" "crates/alva-agent-extension-builtin/src/${f}.rs"
done
```

**Step 2: Rewrite imports inside each copied file**

Inside the newly-copied files, any `use crate::local_fs::` becomes `use crate::local_fs_facade::` (we will introduce this in Phase 4 as a `dyn ToolFs` handle), and `use crate::truncate::` stays the same (we just copied it). `use crate::walk_dir` / `walk_dir_filtered` → `use crate::walkdir::{walk_dir, walk_dir_filtered}`.

**Temporary bridge:** Until Phase 4 lands `LocalToolFs` in host-native, keep the old path working by adding a temporary re-export at the top of `crates/alva-agent-extension-builtin/src/lib.rs`:

```rust
#[cfg(not(target_family = "wasm"))]
pub use alva_agent_tools::local_fs::{LocalToolFs, walk_dir, walk_dir_filtered};
```

This lets the tool implementations compile before `LocalToolFs` is relocated. Phase 4 removes this bridge.

Also add a temporary dev/path dependency in `extension-builtin/Cargo.toml`:

```toml
[target.'cfg(not(target_family = "wasm"))'.dependencies]
alva-agent-tools = { path = "../alva-agent-tools", default-features = false }
```

This dependency is deleted in Phase 4.

**Step 3: Register modules in `lib.rs` under the `core` feature gate**

```rust
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod read_file;
// ... same pattern for create_file, file_edit, list_files, find_files,
//     grep_search, execute_shell, ask_human, view_image, todo_write
#[cfg(feature = "core")]
pub mod enter_plan_mode;
#[cfg(feature = "core")]
pub mod exit_plan_mode;
#[cfg(feature = "core")]
pub mod agent_tool;
```

**Step 4: Verify**

Run: `cargo check -p alva-agent-extension-builtin`
Expected: PASS.

**Step 5: Commit**

```bash
git add -A
git commit -m "refactor: migrate core tools into extension-builtin"
```

---

### Task 3.3: Migrate `utility` feature tools

**Files moved:** `config_tool.rs`, `skill_tool.rs`, `tool_search.rs`, `sleep_tool.rs`.

**Step 1: Copy**

```bash
for f in config_tool skill_tool tool_search sleep_tool; do
  cp "crates/alva-agent-tools/src/${f}.rs" "crates/alva-agent-extension-builtin/src/${f}.rs"
done
```

**Step 2: Register in lib.rs**

```rust
#[cfg(feature = "utility")]
pub mod config_tool;
#[cfg(feature = "utility")]
pub mod skill_tool;
#[cfg(feature = "utility")]
pub mod tool_search;
#[cfg(all(feature = "utility", not(target_family = "wasm")))]
pub mod sleep_tool;
```

**Step 3: Verify + commit**

```bash
cargo check -p alva-agent-extension-builtin --features utility
git add -A
git commit -m "refactor: migrate utility tools into extension-builtin"
```

---

### Task 3.4: Migrate `web` feature tools

**Files moved:** `internet_search.rs`, `read_url.rs`.

**Step 1: Copy**

```bash
cp crates/alva-agent-tools/src/{internet_search,read_url}.rs crates/alva-agent-extension-builtin/src/
```

**Step 2: Register**

```rust
#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod internet_search;
#[cfg(all(feature = "web", not(target_family = "wasm")))]
pub mod read_url;
```

**Step 3: Verify**

```bash
cargo check -p alva-agent-extension-builtin --features web
```

**Step 4: Commit**

```bash
git add -A
git commit -m "refactor: migrate web tools into extension-builtin"
```

---

### Task 3.5: Migrate `notebook` + `worktree` + `team` + `task` + `schedule` features

Each feature is a straight copy-and-register. Commit one at a time.

**Notebook:**

```bash
cp crates/alva-agent-tools/src/notebook_edit.rs crates/alva-agent-extension-builtin/src/
```

Add to lib.rs:
```rust
#[cfg(all(feature = "notebook", not(target_family = "wasm")))]
pub mod notebook_edit;
```

Verify: `cargo check -p alva-agent-extension-builtin --features notebook`
Commit: `git commit -m "refactor: migrate notebook tool into extension-builtin"`

**Worktree:**

```bash
cp crates/alva-agent-tools/src/{enter_worktree,exit_worktree}.rs crates/alva-agent-extension-builtin/src/
```

```rust
#[cfg(all(feature = "worktree", not(target_family = "wasm")))]
pub mod enter_worktree;
#[cfg(all(feature = "worktree", not(target_family = "wasm")))]
pub mod exit_worktree;
```

Commit.

**Team:**

```bash
cp crates/alva-agent-tools/src/{team_create,team_delete,send_message}.rs crates/alva-agent-extension-builtin/src/
```

```rust
#[cfg(feature = "team")]
pub mod team_create;
#[cfg(feature = "team")]
pub mod team_delete;
#[cfg(feature = "team")]
pub mod send_message;
```

Commit.

**Task:**

```bash
cp crates/alva-agent-tools/src/task_{create,update,get,list,output,stop}.rs crates/alva-agent-extension-builtin/src/
```

Register each under `#[cfg(feature = "task")]`. Commit.

**Schedule:**

```bash
cp crates/alva-agent-tools/src/{schedule_cron,remote_trigger}.rs crates/alva-agent-extension-builtin/src/
```

Register each under `#[cfg(feature = "schedule")]`. Commit.

**Step: Verify all features compile together**

```bash
cargo check -p alva-agent-extension-builtin --all-features
```

Expected: PASS.

---

### Task 3.6: Re-create the tool-preset helpers in `extension-builtin`

**Files:**
- Modify: `crates/alva-agent-extension-builtin/src/lib.rs` — add a `tool_presets` module

**Step 1: Add the presets module**

At the bottom of `lib.rs`, add the function group (copy the logic from `crates/alva-agent-tools/src/lib.rs:99-236` but path-update):

```rust
pub mod tool_presets {
    use alva_kernel_abi::tool::Tool;

    #[cfg(all(feature = "core", not(target_family = "wasm")))]
    pub fn file_io() -> Vec<Box<dyn Tool>> {
        vec![
            Box::new(crate::read_file::ReadFileTool),
            Box::new(crate::create_file::CreateFileTool),
            Box::new(crate::file_edit::FileEditTool),
            Box::new(crate::list_files::ListFilesTool),
            Box::new(crate::find_files::FindFilesTool),
            Box::new(crate::grep_search::GrepSearchTool),
            Box::new(crate::view_image::ViewImageTool),
        ]
    }
    #[cfg(not(all(feature = "core", not(target_family = "wasm"))))]
    pub fn file_io() -> Vec<Box<dyn Tool>> { Vec::new() }

    // Repeat for shell(), interaction(), planning(), task_management(),
    // team(), utility(), web(), worktree() — mirroring what
    // alva-agent-tools currently exports.
}
```

Copy over `all_standard()` too so host-native's `register_builtin_tools` has a drop-in replacement.

**Step 2: Verify**

```bash
cargo check -p alva-agent-extension-builtin --all-features
```

**Step 3: Commit**

```bash
git add -A
git commit -m "refactor: port tool_presets helpers into extension-builtin"
```

---

### Task 3.7: Move the 9 Extension wrappers out of `alva-app-core/src/extension/`

**Files moved (wrapper-only, 13–16 lines each):**
- `core.rs` → `crates/alva-agent-extension-builtin/src/wrappers/core.rs`
- `shell.rs` → `wrappers/shell.rs`
- `interaction.rs` → `wrappers/interaction.rs`
- `task.rs` → `wrappers/task.rs`
- `team.rs` → `wrappers/team.rs`
- `planning.rs` → `wrappers/planning.rs`
- `utility.rs` → `wrappers/utility.rs`
- `web.rs` → `wrappers/web.rs`
- `browser.rs` → `wrappers/browser.rs` (still forwards to `alva-app-extension-browser`)

**Step 1: Copy and rewrite imports**

```bash
mkdir -p crates/alva-agent-extension-builtin/src/wrappers
for f in core shell interaction task team planning utility web browser; do
  cp "crates/alva-app-core/src/extension/${f}.rs" "crates/alva-agent-extension-builtin/src/wrappers/${f}.rs"
done
```

In each copied file:
- `use alva_agent_tools::tool_presets;` → `use crate::tool_presets;`
- `use super::Extension;` → `use alva_agent_core::extension::Extension;`

`browser.rs` is special — it forwards to `alva-app-extension-browser`. Add that as an **optional** dep gated by a new `browser` feature:

Edit `crates/alva-agent-extension-builtin/Cargo.toml`:

```toml
[target.'cfg(not(target_family = "wasm"))'.dependencies]
alva-app-extension-browser = { path = "../alva-app-extension-browser", optional = true }

[features]
browser = ["dep:alva-app-extension-browser"]
```

**Step 2: Register `wrappers` module**

Add to `crates/alva-agent-extension-builtin/src/lib.rs`:

```rust
pub mod wrappers;
```

And create `crates/alva-agent-extension-builtin/src/wrappers/mod.rs`:

```rust
pub mod core;
pub mod shell;
pub mod interaction;
pub mod task;
pub mod team;
pub mod planning;
pub mod utility;
pub mod web;
#[cfg(feature = "browser")]
pub mod browser;

pub use core::CoreExtension;
pub use shell::ShellExtension;
pub use interaction::InteractionExtension;
pub use task::TaskExtension;
pub use team::TeamExtension;
pub use planning::PlanningExtension;
pub use utility::UtilityExtension;
pub use web::WebExtension;
#[cfg(feature = "browser")]
pub use browser::BrowserExtension;
```

**Step 3: Delete old wrappers from `alva-app-core`**

```bash
rm crates/alva-app-core/src/extension/{core,shell,interaction,task,team,planning,utility,web,browser}.rs
```

**Step 4: Update `alva-app-core/src/extension/mod.rs` to re-export from extension-builtin**

```rust
pub use alva_agent_extension_builtin::wrappers::{
    CoreExtension, ShellExtension, InteractionExtension, TaskExtension, TeamExtension,
    PlanningExtension, UtilityExtension, WebExtension,
};
#[cfg(feature = "browser")]
pub use alva_agent_extension_builtin::wrappers::BrowserExtension;
```

Remove the matching `mod core;` / `mod shell;` ... lines.

**Step 5: Add `alva-agent-extension-builtin` dep to `alva-app-core/Cargo.toml`**

```toml
alva-agent-extension-builtin = { path = "../alva-agent-extension-builtin", features = ["core", "utility", "web", "task", "team", "browser"] }
```

**Step 6: Verify workspace compiles**

```bash
cargo check --workspace
```

**Step 7: Run app-core tests**

```bash
cargo test -p alva-app-core --tests
```

Expected: PASS.

**Step 8: Commit**

```bash
git add -A
git commit -m "refactor: relocate built-in Extension wrappers to extension-builtin"
```

---

## Phase 4 — Relocate `LocalToolFs` to `alva-host-native`

### Task 4.1: Move `LocalToolFs` (and only the `ToolFs` impl, not the walk helpers)

**Files:**
- Create: `crates/alva-host-native/src/local_fs.rs` (copy of `crates/alva-agent-tools/src/local_fs.rs` minus `walk_dir` / `walk_dir_filtered`)
- Modify: `crates/alva-host-native/src/lib.rs` — declare `pub mod local_fs;` and re-export `LocalToolFs`

**Step 1: Copy + strip**

```bash
cp crates/alva-agent-tools/src/local_fs.rs crates/alva-host-native/src/local_fs.rs
```

Open the new file and delete the `walk_dir` and `walk_dir_filtered` free functions. They already live in `extension-builtin/src/walkdir.rs`.

**Step 2: Update `alva-host-native/src/lib.rs`**

Add:

```rust
pub mod local_fs;
pub use local_fs::LocalToolFs;
```

**Step 3: Update `alva-host-native/Cargo.toml`**

Add `ignore` dep if any retained code uses it (inspect first — if only `walk_dir` used `ignore`, LocalToolFs itself may not need it). If `LocalToolFs::exec` uses `tokio::process`, make sure `tokio = { features = ["process", ...] }` covers it.

**Step 4: Drop the temporary `alva-agent-tools` bridge in extension-builtin**

Edit `crates/alva-agent-extension-builtin/src/lib.rs` — remove the `pub use alva_agent_tools::local_fs::LocalToolFs;` temporary re-export. Tools inside extension-builtin consume `&dyn ToolFs` through `ToolExecutionContext`, not directly through `LocalToolFs`, so they should not be importing it at all.

Run `rg 'LocalToolFs' crates/alva-agent-extension-builtin/src` — every hit inside tool implementations should be an unused import. Delete those imports.

Drop the temporary `alva-agent-tools = { path = ... }` dep from `crates/alva-agent-extension-builtin/Cargo.toml`.

**Step 5: Update host-native builder.rs**

`crates/alva-host-native/src/builder.rs:264` currently calls `alva_agent_tools::register_builtin_tools(&mut registry);`. Replace with:

```rust
alva_agent_extension_builtin::tool_presets::all_standard()
    .into_iter()
    .for_each(|tool| { registry.register(tool); });
```

And update `alva-host-native/Cargo.toml`:

```toml
alva-agent-extension-builtin = { path = "../alva-agent-extension-builtin", features = ["core", "utility", "web", "task", "team", "schedule", "notebook", "worktree"] }
```

Remove the `alva-agent-tools = ...` line.

**Step 6: Verify**

```bash
cargo check --workspace
```

Expected: PASS.

**Step 7: Commit**

```bash
git add -A
git commit -m "refactor: relocate LocalToolFs to alva-host-native and retarget tool registration"
```

---

## Phase 5 — Delete `alva-agent-tools`

### Task 5.1: Final audit

**Step 1: Find remaining references**

```bash
rg 'alva[_-]agent[_-]tools' --glob '!target/*' --glob '!Cargo.lock'
```

Fix every hit:
- `Cargo.toml` deps → remove
- `use alva_agent_tools::` → retarget to `alva_agent_extension_builtin`, `alva_agent_core`, or `alva_host_native` as appropriate
- `AGENTS.md`, `FRACTAL-DOCS.md`, `docs/` — update prose references

**Step 2: Rebuild and test**

```bash
cargo check --workspace
cargo test --workspace --no-run
```

Expected: everything compiles.

**Step 3: Commit**

```bash
git add -A
git commit -m "refactor: retire all alva-agent-tools import references"
```

---

### Task 5.2: Delete the crate

**Step 1: Remove source**

```bash
rm -rf crates/alva-agent-tools
```

**Step 2: Remove from workspace**

Edit workspace root `Cargo.toml`, delete the `"crates/alva-agent-tools",` line.

**Step 3: Regenerate lockfile entries**

```bash
cargo check --workspace
```

Expected: PASS with no `alva-agent-tools` in the dependency graph.

**Step 4: Run full test suite**

```bash
cargo test --workspace
```

Expected: PASS.

**Step 5: wasm target check**

```bash
cargo check --workspace --target wasm32-unknown-unknown
```

Expected: PASS (same crates that compiled before still compile; extension-builtin's wasm-safe features still work).

**Step 6: Commit**

```bash
git add -A
git commit -m "refactor: delete alva-agent-tools crate"
```

---

## Phase 6 — Verification and documentation

### Task 6.1: Update AGENTS.md files

**Files to update:**
- `AGENTS.md` (workspace root)
- `crates/alva-agent-core/AGENTS.md` (new, brief)
- `crates/alva-agent-extension-builtin/AGENTS.md` (new, brief)
- `crates/alva-host-native/AGENTS.md` — note LocalToolFs new home
- `crates/alva-app-core/AGENTS.md` — note that extension trait is now re-exported

Each new AGENTS.md follows the existing format in other crates: one-line purpose, public surface, key files. Keep each under 40 lines.

**Step 1: Write**

Write each file using the template from `crates/alva-app-extension-browser/AGENTS.md` (the most recently-added extension crate) as a reference.

**Step 2: Commit**

```bash
git add -A
git commit -m "docs: update AGENTS.md for agent-tools refactor"
```

---

### Task 6.2: End-to-end smoke test

**Step 1: Run host-native example**

```bash
cargo run -p alva-host-native --example runtime_basic
```

Expected: runs to completion (or to the normal place it would stop if it needs live credentials), no `alva-agent-tools` anywhere in error traces.

**Step 2: Run app-core e2e test**

```bash
cargo test -p alva-app-core --test e2e_agent_test
```

Expected: PASS.

**Step 3: Run full workspace test once more**

```bash
cargo test --workspace
```

Expected: PASS.

**Step 4: Compile the eval binary**

```bash
cargo build -p alva-app-eval
```

Expected: PASS — this catches anyone consuming `tool_presets::*` via the old path.

**Step 5: Final commit (if docs or small fixes were needed)**

```bash
git add -A
git commit --allow-empty -m "refactor: agent-tools extraction complete"
```

---

## Rollback strategy

Each phase is a standalone commit set. To roll back:
- Phase 1–3: `git revert` the phase's commits — `alva-agent-tools` still exists as a parallel copy until Phase 5, so no runtime behaviour is lost.
- Phase 4–5: revert Phase 5 first (restores `alva-agent-tools` directory + workspace entry), then Phase 4 if needed (moves `LocalToolFs` back).

Between Phases 3 and 4, both `alva-agent-tools` and `alva-agent-extension-builtin` have working copies of every tool. The codebase is consistent at every commit.

---

## Out-of-scope follow-ups (to do as a separate plan)

1. **Move `BaseAgent` + `BaseAgentBuilder` to `alva-agent-core`.** Requires first decoupling from `alva-app-extension-memory::MemorySqlite` and `alva-host-native::middleware::SecurityMiddleware` by introducing trait-based hooks. Non-trivial.
2. **Move middleware-style extensions** (`loop_detection`, `dangling_tool_call`, `tool_timeout`, `compaction`, `checkpoint`, `plan_mode`) from `alva-app-core/src/extension/` into `alva-agent-core`. These are 13–18 lines each, mostly thin wrappers around things in `alva-kernel-core::builtins::*`, so this is mechanical — but it is cleanest to do after BaseAgent moves.
3. **Consider splitting `alva-agent-extension-builtin` further** once a specific feature outgrows the shared crate (e.g. if `web` grows to need retry/proxy/robots.txt support, or if `worktree` grows a full git backend).
