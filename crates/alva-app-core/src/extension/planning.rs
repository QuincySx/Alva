//! Planning and worktree tools.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct PlanningExtension;

#[async_trait]
impl Extension for PlanningExtension {
    fn name(&self) -> &str { "planning" }
    fn description(&self) -> &str { "Planning and worktree tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        let mut tools = tool_presets::planning();
        tools.extend(tool_presets::worktree());
        tools
    }
}
