//! alva-agent-context — Context management hooks, SDK, and plugin system.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────┐
//! │  Plugin Layer (strategy)                     │
//! │  impl ContextPlugin — 21 hooks              │
//! ├─────────────────────────────────────────────┤
//! │  SDK Layer (operations)                      │
//! │  ContextSDKImpl → ContextManagementSDK      │
//! ├─────────────────────────────────────────────┤
//! │  Store Layer (data)                          │
//! │  ContextStore — five-layer CRUD              │
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
pub use sdk::ContextManagementSDK;
pub use sdk_impl::ContextSDKImpl;
pub use store::ContextStore;
pub use rules_plugin::RulesContextPlugin;
pub use default_plugin::{DefaultContextPlugin, DefaultPluginConfig};
pub use message_store::{MessageStore, InMemoryMessageStore, Turn};
pub use types::*;
