//! Task management tools: create, update, get, list, output, stop.
//!
//! Owns a `TaskService` (default: `InMemoryTaskStore`) and publishes it
//! on the bus during `configure()` so the per-tool implementations can
//! pick it up via `ctx.bus().get::<dyn TaskService>()`. To swap the
//! backend (SQLite, Redis, …), register a replacement extension with
//! `name() == "task"` — `BaseAgent`'s default-replacement contract
//! makes this one skip.

use std::sync::Arc;

use alva_agent_core::extension::{Extension, ExtensionContext};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

use crate::services::{InMemoryTaskStore, TaskService};

pub struct TaskExtension {
    service: Arc<dyn TaskService>,
}

impl TaskExtension {
    /// Wrap a caller-supplied service. Use this to plug in a persistent
    /// backend without writing a whole new `Extension`.
    pub fn new(service: Arc<dyn TaskService>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &Arc<dyn TaskService> {
        &self.service
    }
}

impl Default for TaskExtension {
    fn default() -> Self {
        Self {
            service: Arc::new(InMemoryTaskStore::new()),
        }
    }
}

#[async_trait]
impl Extension for TaskExtension {
    fn name(&self) -> &str {
        "task"
    }
    fn description(&self) -> &str {
        "Task management"
    }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        crate::tool_presets::task_management()
    }
    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer
            .provide::<dyn TaskService>(self.service.clone());
    }
}
