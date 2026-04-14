//! Built-in agent extensions.
//!
//! This crate consolidates every reference tool implementation (formerly
//! in `alva-agent-tools`) and every thin Extension wrapper (formerly in
//! `alva-app-core/src/extension/*.rs`). Callers compose only the features
//! they want. Heavy domain extensions (browser, memory) live in separate
//! `alva-app-extension-*` crates because they pull app-level concerns.

// Tool implementations and Extension wrappers are added task-by-task
// during the rest of the refactor.

pub mod truncate;

#[cfg(not(target_family = "wasm"))]
pub mod walkdir;

/// Transitional bridge until `LocalToolFs` lands in `alva-host-native`
/// in Phase 4. Do not add new code here — this module exists solely to
/// keep migrated tool implementations compiling during the refactor.
#[cfg(not(target_family = "wasm"))]
pub mod local_fs {
    pub use alva_agent_tools::local_fs::LocalToolFs;
}

// MockToolFs re-exported for test modules inside migrated tools.
pub use alva_agent_core::MockToolFs;

// ---- core feature tools ----

#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod read_file;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod create_file;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod file_edit;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod list_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod find_files;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod grep_search;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod execute_shell;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod ask_human;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod view_image;
#[cfg(all(feature = "core", not(target_family = "wasm")))]
pub mod todo_write;

// Plan mode primitives are pure signaling — wasm-safe.
#[cfg(feature = "core")]
pub mod enter_plan_mode;
#[cfg(feature = "core")]
pub mod exit_plan_mode;

// Placeholder agent_tool — pure struct, wasm-safe.
#[cfg(feature = "core")]
pub mod agent_tool;

// ---- utility feature tools ----

#[cfg(feature = "utility")]
pub mod config_tool;
#[cfg(feature = "utility")]
pub mod skill_tool;
#[cfg(feature = "utility")]
pub mod tool_search;
#[cfg(all(feature = "utility", not(target_family = "wasm")))]
pub mod sleep_tool;
