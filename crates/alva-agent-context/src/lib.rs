// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, session, message_store, types
// OUTPUT: re-exports ContextHooks, ContextHandle, ContextHandleImpl, ContextStore, RulesContextHooks, DefaultContextHooks, DefaultHooksConfig, SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch, InMemorySession, MessageStore, InMemoryMessageStore, Turn
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
//! ContextHooks and SessionAccess are injected into the Agent as components.

pub mod types;
pub mod plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod store;
pub mod session;
pub mod chain;
pub mod context_system;
pub mod rules_plugin;
pub mod default_plugin;

pub use plugin::{ContextError, ContextHooks};
pub use sdk::ContextHandle;
pub use sdk_impl::ContextHandleImpl;
pub use store::ContextStore;
pub use chain::ContextHooksChain;
pub use context_system::ContextSystem;
pub use rules_plugin::RulesContextHooks;
pub use default_plugin::{DefaultContextHooks, DefaultHooksConfig};
pub use session::{SessionAccess, SessionEvent, SessionMessage, EventQuery, EventMatch, InMemorySession};
pub use types::*;
