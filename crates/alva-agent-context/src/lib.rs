// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, session, types, chain, context_system
// OUTPUT: re-exports ContextHooks, ContextHandle, ContextHandleImpl, ContextStore, RulesContextHooks, DefaultContextHooks, DefaultHooksConfig, SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch, InMemorySession, ContextSystem, default_context_system
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
//! │  SessionAccess — append-only event log       │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! Traits and value types are defined in `alva_kernel_abi::context`. This crate provides
//! the concrete implementations: `RulesContextHooks`, `DefaultContextHooks`,
//! `ContextHooksChain`, `ContextHandleImpl`, `ContextStore`, `InMemorySession`.

pub mod auto_compact;
pub mod chain;
pub mod compact;
pub mod context_system;
pub mod default_plugin;
pub mod plugin;
pub mod rules_plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod session;
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

pub use auto_compact::AutoCompactState;
pub use chain::ContextHooksChain;
pub use compact::{
    compact_messages, micro_compact_messages, should_compact, CompactionConfig, CompactionResult,
    MicroCompactResult,
};
pub use context_system::{default_context_system, ContextSystem};
pub use default_plugin::{DefaultContextHooks, DefaultHooksConfig, DefaultSummarizeFn};
pub use plugin::{ContextError, ContextHooks};
pub use rules_plugin::RulesContextHooks;
pub use sdk::ContextHandle;
pub use sdk_impl::{ContextHandleImpl, MemoryBackend, SummarizeFn, Summarizer};
pub use session::{
    EventMatch, EventQuery, InMemorySession, SessionAccess, SessionEvent, SessionMessage,
};
pub use store::ContextStore;
pub use types::*;
