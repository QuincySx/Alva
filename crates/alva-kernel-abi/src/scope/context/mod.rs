// INPUT:  alva_kernel_abi types (AgentMessage, ContentBlock, Message, MessageRole), alva_kernel_bus::BusEvent, async_trait, serde, serde_json, thiserror, chrono, uuid, std
// OUTPUT: ContextHooks, ContextHandle, SessionAccess, ContextSystem, TokenBudgetExceeded, ContextCompacted, MemoryExtracted, value types
// POS:    Shared context management vocabulary — traits, value types, container, and bus events for context observability.
//! Context management shared vocabulary.
//!
//! This module provides the **trait definitions**, **value types**, the **ContextSystem**
//! container, and pure **apply** helpers that multiple crates need. Concrete implementations
//! (e.g., `DefaultContextHooks`, `RulesContextHooks`, `ContextHooksChain`,
//! `ContextHandleImpl`, `ContextStore`, `InMemorySession`) live in `alva-agent-context`.
//!
//! By placing traits and types here in `alva-kernel-abi`, the core agent crate can depend on
//! types alone, making the full context system an optional plugin.

mod apply;
mod error;
mod events;
mod noop;
mod system;
mod traits;
mod types;

// Re-export everything so `alva_kernel_abi::scope::context::*` continues to work.
pub use apply::{apply_compressions, apply_injections};
pub use error::*;
pub use events::*;
pub use noop::*;
pub use system::*;
pub use traits::*;
pub use types::*;
