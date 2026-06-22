//! Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct UtilityPlugin;

#[async_trait]
impl Plugin for UtilityPlugin {
    fn name(&self) -> &str {
        "utility"
    }
    fn description(&self) -> &str {
        "Utility tools"
    }
    async fn register(&self, r: &Registrar) {
        r.tools(crate::tool_presets::utility());
    }
}
