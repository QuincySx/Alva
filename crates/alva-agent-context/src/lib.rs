// INPUT:  plugin, sdk, sdk_impl, store, rules_plugin, default_plugin, message_store, types
// OUTPUT: pub mod types/plugin/sdk/sdk_impl/store/message_store/rules_plugin/default_plugin; re-exports ContextPlugin, ContextPluginSDK, ContextSDKImpl, ContextStore, RulesContextPlugin, DefaultContextPlugin, DefaultPluginConfig, MessageStore, InMemoryMessageStore, Turn
// POS:    Crate root that declares submodules and re-exports all public types for the context management system.
//! alva-agent-context — Context management hooks, SDK, and plugin system.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Plugin Layer (strategy)                     │
//! │  impl ContextPlugin — 8 hooks               │
//! ├─────────────────────────────────────────────┤
//! │  SDK Layer (operations)                      │
//! │  ContextSDKImpl → ContextPluginSDK          │
//! ├─────────────────────────────────────────────┤
//! │  Store Layer (data)                          │
//! │  ContextStore — four-layer CRUD              │
//! │  MessageStore — turn-based persistence       │
//! └─────────────────────────────────────────────┘
//! ```
//!
//! ContextPlugin and MessageStore are injected into the Agent as components.
//! The agent loop calls plugin hooks directly — no middleware adapter needed.

pub mod types;
pub mod plugin;
pub mod sdk;
pub mod sdk_impl;
pub mod store;
pub mod message_store;
pub mod rules_plugin;
pub mod default_plugin;

pub use plugin::{ContextError, ContextPlugin};
pub use sdk::ContextPluginSDK;
pub use sdk_impl::ContextSDKImpl;
pub use store::ContextStore;
pub use rules_plugin::RulesContextPlugin;
pub use default_plugin::{DefaultContextPlugin, DefaultPluginConfig};
pub use message_store::{MessageStore, InMemoryMessageStore, Turn};
pub use types::*;
