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
