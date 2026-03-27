// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, message_store, types
// OUTPUT: pub mod types/plugin/sdk/sdk_impl/store/message_store/rules_plugin/default_plugin; re-exports ContextHooks, ContextHooksSDK, ContextSDKImpl, ContextStore, RulesContextHooks, DefaultContextHooks, DefaultHooksConfig, MessageStore, InMemoryMessageStore, Turn
// POS:    Crate root that declares submodules and re-exports all public types for the context management system.
//! alva-agent-context — Context management hooks, SDK, and plugin system.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Plugin Layer (strategy)                     │
//! │  impl ContextHooks — 8 hooks               │
//! ├─────────────────────────────────────────────┤
//! │  SDK Layer (operations)                      │
//! │  ContextSDKImpl → ContextHooksSDK          │
//! ├─────────────────────────────────────────────┤
//! │  Store Layer (data)                          │
//! │  ContextStore — four-layer CRUD              │
//! │  MessageStore — turn-based persistence       │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ContextHooks and MessageStore are injected into the Agent as components.
//! The agent loop calls plugin hooks directly — no middleware adapter needed.

pub mod types;
pub mod plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod store;
pub mod message_store;
pub mod rules_plugin;
pub mod default_plugin;

pub use plugin::{ContextError, ContextHooks};
pub use sdk::ContextHooksSDK;
pub use sdk_impl::ContextSDKImpl;
pub use store::ContextStore;
pub use rules_plugin::RulesContextHooks;
pub use default_plugin::{DefaultContextHooks, DefaultHooksConfig};
pub use message_store::{MessageStore, InMemoryMessageStore, Turn};
pub use types::*;
