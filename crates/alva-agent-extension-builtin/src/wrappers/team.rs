//! Team / multi-agent coordination tools.
//!
//! Owns a `TeamService` (default: `InMemoryTeamStore`) and publishes it
//! on the bus during `configure()`. Mirror of [`TaskPlugin`] — replace
//! by registering an extension with `name() == "team"`.

use std::sync::Arc;

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

use crate::services::{InMemoryTeamStore, TeamService};

pub struct TeamPlugin {
    service: Arc<dyn TeamService>,
}

impl TeamPlugin {
    pub fn new(service: Arc<dyn TeamService>) -> Self {
        Self { service }
    }

    pub fn service(&self) -> &Arc<dyn TeamService> {
        &self.service
    }
}

impl Default for TeamPlugin {
    fn default() -> Self {
        Self {
            service: Arc::new(InMemoryTeamStore::new()),
        }
    }
}

#[async_trait]
impl Plugin for TeamPlugin {
    fn name(&self) -> &str {
        "team"
    }
    fn description(&self) -> &str {
        "Team / multi-agent coordination"
    }
    async fn register(&self, r: &Registrar) {
        r.tools(crate::tool_presets::team());
        r.provide::<dyn TeamService>(self.service.clone());
    }
}
