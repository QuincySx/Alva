# Crate Reorganization Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Organize the crate structure so each crate has a clear responsibility, types are categorized, and infrastructure isn't mixed with app-level code.

**Architecture:** Two changes: (1) `alva-types` gets subdirectory grouping while keeping the same public API via re-exports; (2) scope/blackboard infrastructure extracts from `alva-app-core` into a new `alva-agent-scope` crate. Both changes preserve all existing imports.

**Tech Stack:** Rust workspace, `cargo` module system, `pub use` re-exports for backward compatibility.

---

## Current Problem

```
alva-types/src/          ← 20 flat files, no categories
├── cancel.rs            ┐
├── content.rs           │ Core Agent
├── error.rs             │ (mixed with
├── message.rs           │  multimodal
├── model.rs             │  and scope)
├── stream.rs            │
├── tool.rs              ┘
├── tool_guard.rs        ← execution control (no category)
├── context.rs           ← context management (no category)
├── scope.rs             ← scope types (no category)
├── provider.rs          ← provider (no category)
├── embedding.rs         ┐
├── transcription.rs     │
├── speech.rs            │ Multimodal
├── image.rs             │ (7 files mixed with core)
├── video.rs             │
├── reranking.rs         │
└── moderation.rs        ┘

alva-app-core/src/       ← 91 files, infrastructure mixed with app
├── scope/               ← should be lower (agent infrastructure)
├── plugins/blackboard/  ← should be lower (communication infra)
├── plugins/evaluation/  ← app-level (ok here)
├── plugins/team.rs      ← app-level (ok here)
└── ...everything else
```

## Target Structure

```
alva-types/src/
├── lib.rs               ← same public API (re-exports unchanged)
├── core/                ← Agent fundamentals
│   ├── mod.rs
│   ├── cancel.rs
│   ├── content.rs
│   ├── error.rs
│   ├── message.rs
│   └── stream.rs
├── model/               ← LLM model interface
│   ├── mod.rs
│   ├── model.rs
│   └── config.rs
├── tool/                ← Tool system
│   ├── mod.rs
│   ├── tool.rs
│   ├── tool_guard.rs
│   └── registry.rs
├── scope/               ← Execution scope
│   ├── mod.rs
│   ├── scope.rs
│   └── context.rs
├── provider/            ← Provider routing
│   ├── mod.rs
│   ├── provider.rs
│   └── provider_test.rs
└── multimodal/          ← Optional model interfaces
    ├── mod.rs
    ├── embedding.rs
    ├── transcription.rs
    ├── speech.rs
    ├── image.rs
    ├── video.rs
    ├── reranking.rs
    └── moderation.rs

NEW: alva-agent-scope/src/    ← extracted from alva-app-core
├── lib.rs
├── blackboard/               ← moved from alva-app-core/plugins/blackboard
│   ├── mod.rs
│   ├── board.rs
│   ├── message.rs
│   ├── profile.rs
│   └── plugin.rs
├── scope_impl.rs             ← moved from alva-app-core/scope/scope_impl.rs
├── board_registry.rs         ← moved from alva-app-core/scope/board_registry.rs
└── session_tracker.rs        ← moved from alva-app-core/scope/session_tracker.rs

alva-app-core/src/
├── plugins/                  ← only APP-level plugins remain
│   ├── evaluation/
│   ├── team.rs
│   └── agent_spawn.rs       ← updated imports
├── scope/ → DELETED          ← moved to alva-agent-scope
├── plugins/blackboard/ → DELETED  ← moved to alva-agent-scope
└── ...rest unchanged
```

## Key Principle: Public API stays the same

`alva-types` lib.rs keeps ALL existing `pub use` re-exports. Downstream code doesn't change:
```rust
// These all still work:
use alva_types::Message;
use alva_types::Tool;
use alva_types::ScopeId;
use alva_types::EmbeddingModel;
```

The change is INTERNAL only — files move into subdirectories.

---

## Phase 1: Organize alva-types (7 tasks)

### Task 1: Create core/ subdirectory

**Files:**
- Create: `crates/alva-types/src/core/mod.rs`
- Move: `cancel.rs`, `content.rs`, `error.rs`, `message.rs`, `stream.rs` → `core/`
- Modify: `crates/alva-types/src/lib.rs`

- [ ] **Step 1: Create core/mod.rs**

```rust
// crates/alva-types/src/core/mod.rs
pub mod cancel;
pub mod content;
pub mod error;
pub mod message;
pub mod stream;
```

- [ ] **Step 2: Move files**

```bash
mkdir -p crates/alva-types/src/core
mv crates/alva-types/src/cancel.rs crates/alva-types/src/core/
mv crates/alva-types/src/content.rs crates/alva-types/src/core/
mv crates/alva-types/src/error.rs crates/alva-types/src/core/
mv crates/alva-types/src/message.rs crates/alva-types/src/core/
mv crates/alva-types/src/stream.rs crates/alva-types/src/core/
```

- [ ] **Step 3: Update lib.rs**

Replace individual `pub mod cancel; pub mod content;` etc. with `pub mod core;` and update re-exports to point to `core::cancel`, `core::content`, etc.

**Important**: The `pub use` lines in lib.rs change their source path but NOT the public API:
```rust
// Before:
pub mod cancel;
pub use cancel::CancellationToken;

// After:
pub mod core;
pub use core::cancel::CancellationToken;
```

- [ ] **Step 4: Fix internal cross-references**

Files in `core/` that reference each other (e.g., `message.rs` uses `crate::content::ContentBlock`) need updating to `crate::core::content::ContentBlock`.

Files OUTSIDE core that reference core types (e.g., `model.rs` uses `crate::message::Message`) need updating to `crate::core::message::Message`.

- [ ] **Step 5: Run tests**

```bash
cargo test -p alva-types --lib
cargo check -p alva-agent-core -p alva-app-core  # downstream still compiles
```

- [ ] **Step 6: Commit**

```bash
git add crates/alva-types/src/
git commit -m "refactor(types): move core types into core/ subdirectory"
```

---

### Task 2: Create model/ subdirectory

**Files:**
- Create: `crates/alva-types/src/model/mod.rs`
- Move: `model.rs` → `model/model.rs`
- Modify: `crates/alva-types/src/lib.rs`

The current `model.rs` has both `ModelConfig` and `LanguageModel` trait. Move as-is.

- [ ] **Step 1: Move file**

```bash
mkdir -p crates/alva-types/src/model
mv crates/alva-types/src/model.rs crates/alva-types/src/model/model.rs
```

- [ ] **Step 2: Create model/mod.rs**

```rust
mod model;
pub use model::*;
```

- [ ] **Step 3: Update lib.rs re-exports**

```rust
// Before:
pub mod model;
pub use model::{LanguageModel, ModelConfig};

// After:
pub mod model;
pub use model::{LanguageModel, ModelConfig};
// (no change needed if mod.rs re-exports everything)
```

- [ ] **Step 4: Fix internal references** (`crate::model::...` stays the same since module name didn't change)

- [ ] **Step 5: Run tests + downstream check**

- [ ] **Step 6: Commit**

```bash
git commit -m "refactor(types): move model types into model/ subdirectory"
```

---

### Task 3: Create tool/ subdirectory

**Files:**
- Move: `tool.rs` → `tool/tool.rs`, `tool_guard.rs` → `tool/tool_guard.rs`
- Create: `tool/mod.rs`

- [ ] **Step 1: Move files**

```bash
mkdir -p crates/alva-types/src/tool
mv crates/alva-types/src/tool.rs crates/alva-types/src/tool/tool.rs
mv crates/alva-types/src/tool_guard.rs crates/alva-types/src/tool/tool_guard.rs
```

- [ ] **Step 2: Create tool/mod.rs**

```rust
mod tool;
pub mod tool_guard;
pub use tool::*;
```

- [ ] **Step 3: Update lib.rs**

```rust
pub mod tool;
pub use tool::tool::{Tool, ToolCall, ToolContext, ...};  // or via mod.rs re-export
pub use tool::tool_guard::{ToolGuard, GuardToken, GuardError};
```

- [ ] **Step 4: Fix internal references** (tool_guard.rs may reference `crate::error::AgentError` → now `crate::core::error::AgentError`)

- [ ] **Step 5: Run tests + downstream**

- [ ] **Step 6: Commit**

---

### Task 4: Create scope/ subdirectory

**Files:**
- Move: `scope.rs` → `scope/scope.rs`, `context.rs` → `scope/context.rs`
- Create: `scope/mod.rs`

`context.rs` is 958 lines — it's the context management types (ContextHooks, ContextHandle, four-layer model). It belongs with scope because scope manages context.

- [ ] Steps same as above pattern.

- [ ] **Commit**

```bash
git commit -m "refactor(types): move scope and context types into scope/ subdirectory"
```

---

### Task 5: Create provider/ subdirectory

**Files:**
- Move: `provider.rs` → `provider/provider.rs`, `provider_test.rs` → `provider/provider_test.rs`
- Create: `provider/mod.rs`

- [ ] Steps same as above pattern.

---

### Task 6: Create multimodal/ subdirectory

**Files:**
- Move: `embedding.rs`, `transcription.rs`, `speech.rs`, `image.rs`, `video.rs`, `reranking.rs`, `moderation.rs` → `multimodal/`
- Create: `multimodal/mod.rs`

7 files, all independent of each other. Simplest move.

- [ ] Steps same as above pattern.

- [ ] **Commit**

```bash
git commit -m "refactor(types): move multimodal model interfaces into multimodal/ subdirectory"
```

---

### Task 7: Verify all downstream crates compile

- [ ] **Step 1: Full workspace check**

```bash
cargo check --workspace
```

- [ ] **Step 2: Full test suite**

```bash
cargo test --workspace
```

Fix any remaining broken `crate::` references in alva-types internals.

- [ ] **Commit** (if any fixes needed)

---

## Phase 2: Extract alva-agent-scope crate (4 tasks)

### Task 8: Create alva-agent-scope crate

**Files:**
- Create: `crates/alva-agent-scope/Cargo.toml`
- Create: `crates/alva-agent-scope/src/lib.rs`
- Move: `alva-app-core/src/plugins/blackboard/` → `alva-agent-scope/src/blackboard/`
- Move: `alva-app-core/src/scope/board_registry.rs` → `alva-agent-scope/src/`
- Move: `alva-app-core/src/scope/session_tracker.rs` → `alva-agent-scope/src/`
- Move: `alva-app-core/src/scope/scope_impl.rs` → `alva-agent-scope/src/`
- Modify: `Cargo.toml` (workspace members)

```toml
# crates/alva-agent-scope/Cargo.toml
[package]
name = "alva-agent-scope"
version = "0.1.0"
edition = "2021"
description = "Agent execution scope — board isolation, session tree, depth control"

[dependencies]
alva-types = { path = "../alva-types" }
async-trait = "0.1"
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["sync"] }
uuid = { version = "1", features = ["v4"] }
chrono = "0.4"
tracing = "0.1"
```

- [ ] **Step 1: Create crate skeleton**
- [ ] **Step 2: Move blackboard files**
- [ ] **Step 3: Move scope files**
- [ ] **Step 4: Fix all `crate::` references** in moved files (e.g., `crate::plugins::blackboard::` → `crate::blackboard::`)
- [ ] **Step 5: Write lib.rs** with re-exports
- [ ] **Step 6: Add to workspace Cargo.toml**

- [ ] **Commit**

```bash
git commit -m "refactor: extract alva-agent-scope crate from alva-app-core"
```

---

### Task 9: Update alva-app-core to depend on alva-agent-scope

**Files:**
- Modify: `crates/alva-app-core/Cargo.toml` (add alva-agent-scope dep)
- Modify: `crates/alva-app-core/src/lib.rs` (re-export from alva-agent-scope)
- Modify: `crates/alva-app-core/src/plugins/agent_spawn.rs` (update imports)
- Modify: `crates/alva-app-core/src/plugins/team.rs` (update imports)
- Modify: `crates/alva-app-core/src/plugins/evaluation/criteria.rs` (update imports)
- Modify: `crates/alva-app-core/src/base_agent.rs` (update imports)
- Delete: `crates/alva-app-core/src/scope/` directory
- Delete: `crates/alva-app-core/src/plugins/blackboard/` directory

- [ ] **Step 1: Add dependency**

```toml
# In alva-app-core/Cargo.toml
alva-agent-scope = { path = "../alva-agent-scope" }
```

- [ ] **Step 2: Update lib.rs**

```rust
// Re-export alva-agent-scope
pub use alva_agent_scope;
```

- [ ] **Step 3: Update all imports** in remaining app-core files

Find-replace:
```
crate::plugins::blackboard:: → alva_agent_scope::blackboard::
crate::scope::              → alva_agent_scope::
```

- [ ] **Step 4: Delete moved directories**

- [ ] **Step 5: Compile check + tests**

- [ ] **Commit**

```bash
git commit -m "refactor(app-core): depend on alva-agent-scope, remove extracted code"
```

---

### Task 10: Update tests and integration

**Files:**
- Modify: `crates/alva-app-core/tests/scope_integration.rs` (update imports)
- Modify: `crates/alva-app-core/src/plugins/mod.rs` (remove blackboard)

- [ ] **Step 1: Update integration test imports**
- [ ] **Step 2: Update plugins/mod.rs** (remove `pub mod blackboard;`)
- [ ] **Step 3: Run full test suite**

```bash
cargo test -p alva-agent-scope --lib
cargo test -p alva-app-core --lib
cargo test -p alva-app-core --test scope_integration
```

- [ ] **Commit**

---

### Task 11: Final workspace verification

- [ ] **Step 1: Full workspace build**

```bash
cargo check --workspace
```

- [ ] **Step 2: Full test suite**

```bash
cargo test --workspace
```

- [ ] **Step 3: Verify crate counts**

```
Before: 23 crates
After:  24 crates (+alva-agent-scope)
```

- [ ] **Commit** (if fixes needed)

---

## Post-reorganization structure

```
crates/
├── alva-types/              ← organized into 6 subdirs
│   ├── core/                   (5 files: cancel, content, error, message, stream)
│   ├── model/                  (1 file: model trait + config)
│   ├── tool/                   (2 files: tool trait, tool_guard)
│   ├── scope/                  (2 files: scope types, context types)
│   ├── provider/               (2 files: provider, test)
│   └── multimodal/             (7 files: embedding, speech, image, ...)
│
├── alva-agent-core/         ← unchanged
├── alva-agent-scope/        ← NEW (extracted from app-core)
│   ├── blackboard/             (4 files: board, message, profile, plugin)
│   ├── scope_impl.rs
│   ├── board_registry.rs
│   └── session_tracker.rs
│
├── alva-agent-graph/        ← unchanged
├── alva-agent-tools/        ← unchanged
├── alva-agent-security/     ← unchanged
├── alva-agent-memory/       ← unchanged
├── alva-agent-context/      ← unchanged
├── alva-agent-runtime/      ← unchanged
│
├── alva-app-core/           ← lighter (scope + blackboard removed)
│   ├── plugins/
│   │   ├── evaluation/         (stays: app-level)
│   │   ├── team.rs             (stays: app-level)
│   │   └── agent_spawn.rs      (updated imports)
│   └── ...
│
└── ... (other crates unchanged)
```

## Dependency graph change

```
Before:                          After:
alva-types                       alva-types (organized internally)
  └── alva-agent-core              └── alva-agent-core
       └── alva-app-core                └── alva-agent-scope (NEW)
            (scope, blackboard               (scope, blackboard,
             inside here)                     board_registry,
                                              session_tracker)
                                         └── alva-app-core
                                              (uses alva-agent-scope)
```
