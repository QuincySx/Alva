//! Planning and worktree tools.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct PlanningExtension;

#[async_trait]
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning and worktree tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools = crate::tool_presets::planning();
        tools.extend(crate::tool_presets::worktree());
        tools
    }
}
