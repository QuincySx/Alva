//! Team / multi-agent coordination tools.
//!
//! Owns a `TeamService` (default: `InMemoryTeamStore`) and publishes it
//! on the bus during `configure()`. Mirror of [`TaskExtension`] — replace
//! by registering an extension with `name() == "team"`.

use std::sync::Arc;

use alva_agent_core::extension::{Extension, ExtensionContext};
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

use crate::services::{InMemoryTeamStore, TeamService};

pub struct TeamExtension {
    service: Arc<dyn TeamService>,
}

impl TeamExtension {
    pub fn new(service: Arc<dyn TeamService>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &Arc<dyn TeamService> {
        &self.service
    }
}

impl Default for TeamExtension {
    fn default() -> Self {
        Self {
            service: Arc::new(InMemoryTeamStore::new()),
        }
    }
}

#[async_trait]
impl Extension for TeamExtension {
    fn name(&self) -> &str {
        "team"
    }
    fn description(&self) -> &str {
        "Team / multi-agent coordination"
    }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        crate::tool_presets::team()
    }
    async fn configure(&self, ctx: &ExtensionContext) {
        ctx.bus_writer
            .provide::<dyn TeamService>(self.service.clone());
    }
}
