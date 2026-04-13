// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, session, types, chain, context_system, apply
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

pub mod types;
pub mod plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod store;
pub mod session;
pub mod chain;
pub mod context_system;
pub mod apply;
pub mod rules_plugin;
pub mod default_plugin;
pub mod compact;
pub mod auto_compact;
pub mod system_context;

pub use plugin::{ContextError, ContextHooks};
pub use sdk::ContextHandle;
pub use sdk_impl::{ContextHandleImpl, MemoryBackend, SummarizeFn, Summarizer};
pub use store::ContextStore;
pub use chain::ContextHooksChain;
pub use context_system::{ContextSystem, default_context_system};
pub use rules_plugin::RulesContextHooks;
pub use default_plugin::{DefaultContextHooks, DefaultHooksConfig, DefaultSummarizeFn};
pub use session::{SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch, InMemorySession};
pub use compact::{CompactionConfig, CompactionResult, MicroCompactResult, compact_messages, micro_compact_messages, should_compact};
pub use auto_compact::AutoCompactState;
pub use types::*;
