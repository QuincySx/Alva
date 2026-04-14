//! Extension system — the primary extensibility point for agents.
//!
//! Contains only the Extension trait + dispatch machinery. Built-in
//! Extension implementations (file-io, shell, task, team, web, etc.) live
//! in `alva-agent-extension-builtin`. App-layer protocol extensions
//! (skills, mcp, hooks, evaluation, agent_spawn) live in `alva-app-core`.

mod bridge;
mod context;
mod events;
mod host;

pub use bridge::ExtensionBridgeMiddleware;
pub use context::{ExtensionContext, FinalizeContext};
pub use events::{EventResult, ExtensionEvent};
pub use host::{ExtensionHost, HostAPI, RegisteredCommand};

use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;
use std::sync::Arc;

#[async_trait]
pub trait Extension: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str { "" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { vec![] }
    fn activate(&self, _api: &HostAPI) {}
    async fn configure(&self, _ctx: &ExtensionContext) {}
    async fn finalize(&self, _ctx: &FinalizeContext) -> Vec<Arc<dyn Tool>> { vec![] }
}
