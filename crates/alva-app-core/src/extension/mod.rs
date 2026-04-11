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
use alva_agent_core::middleware::Middleware;

/// A capability package that participates in agent construction.
///
/// Extensions provide tools and middleware, and receive context
/// after all extensions are collected (via configure()).
///
/// This is the **only** public extensibility point for BaseAgent users.
#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    async fn middleware(&self) -> Vec<Arc<dyn Middleware>> { vec![] }
    /// Called after all extensions are collected and bus/workspace are ready.
    async fn configure(&self, _ctx: &ExtensionContext) {}
    /// Called after ALL tools/middleware from ALL extensions are collected.
    /// Can return additional tools that depend on the final tool list (e.g., AgentSpawnTool).
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
    /// Called after the agent is fully built. Register event handlers, commands, etc.
    fn activate(&self, _api: &HostAPI) {}
}
