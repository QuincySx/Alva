//! Extension system — the primary extensibility point for agents.

mod context;
mod builtins;
mod events;
mod host;
mod bridge;

pub use context::{ExtensionContext, FinalizeContext};
pub use builtins::*;
pub use events::{ExtensionEvent, EventResult};
pub use host::{ExtensionHost, HostAPI, RegisteredCommand};
pub use bridge::ExtensionBridgeMiddleware;

use std::sync::Arc;
use async_trait::async_trait;
use alva_types::tool::Tool;

/// A capability package that participates in agent construction and runtime.
///
/// Lifecycle:
///   1. `tools()`     — build phase: provide tools
///   2. `activate()`  — build phase: register middleware, event handlers, commands via HostAPI
///   3. `configure()`  — build phase: setup with bus/workspace context
///   4. `finalize()`  — build phase: add tools that depend on the final tool list
///   5. runtime       — event handlers fire, steer/follow_up/shutdown available
///
/// This is the **only** public extensibility point for BaseAgent users.
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    /// Provide tools during build phase.
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    /// Register middleware, event handlers, commands via the HostAPI.
    /// Called after tools are collected but before middleware stack is built.
    fn activate(&self, _api: &HostAPI) {}
    /// Called after all extensions are collected and bus/workspace are ready.
    async fn configure(&self, _ctx: &ExtensionContext) {}
    /// Called after ALL tools/middleware from ALL extensions are collected.
    /// Can return additional tools that depend on the final tool list.
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
