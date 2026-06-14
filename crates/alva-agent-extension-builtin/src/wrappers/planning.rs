//! Planning and worktree tools.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct PlanningPlugin;

#[async_trait]
impl Plugin for PlanningPlugin {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning and worktree tools" }
    async fn register(&self, r: &Registrar) {
        let mut tools = crate::tool_presets::planning();
        tools.extend(crate::tool_presets::worktree());
        r.tools(tools);
    }
}
