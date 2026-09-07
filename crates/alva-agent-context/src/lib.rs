// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, types, chain, context_system
// OUTPUT: context hooks/handle/store implementations and default ContextSystem
// POS:    Crate root that declares submodules and re-exports all public types for the context management system.
//! alva-agent-context — Context management hooks, session storage, and plugin system.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Hooks Layer (strategy)                      │
//! │  impl ContextHooks — 8 hooks                │
//! ├─────────────────────────────────────────────┤
//! │  Handle Layer (operations)                   │
//! │  ContextHandleImpl → ContextHandle           │
//! ├─────────────────────────────────────────────┤
//! │  Storage Layer (data)                        │
//! │  ContextStore — four-layer runtime context   │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! Traits and value types are defined in `alva_kernel_abi::context`. This crate provides
//! the concrete implementations: `RulesContextHooks`, `DefaultContextHooks`,
//! `ContextHooksChain`, `ContextHandleImpl`, and `ContextStore`.

pub mod chain;
pub mod context_apply;
pub mod context_system;
pub mod default_plugin;
pub mod plugin;
pub mod rules_plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod store;
pub mod types;
// system_context scans git + CLAUDE.md from the real filesystem, which
// has no meaning on wasm32 (and pulls tokio process/fs features that
// don't compile for wasm). Gated out on wasm targets.
pub mod middleware;
pub mod scope;
#[cfg(not(target_family = "wasm"))]
pub mod system_context;

/// Small UTF-8-safe display utilities shared across modules.
pub(crate) mod util;

pub use middleware::CompactionMiddleware;

pub use chain::ContextHooksChain;
pub use context_apply::{apply_compressions, apply_injections, DefaultContextApply};
pub use context_system::{default_context_system, ContextSystem};
pub use default_plugin::{DefaultContextHooks, DefaultHooksConfig, DefaultSummarizeFn};
pub use plugin::{ContextError, ContextHooks};
pub use rules_plugin::RulesContextHooks;
pub use sdk::ContextHandle;
pub use sdk_impl::{ContextHandleImpl, MemoryBackend, SummarizeFn, Summarizer};
pub use store::ContextStore;
pub use types::*;
