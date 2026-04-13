//! Team / multi-agent coordination tools.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct TeamExtension;

#[async_trait]
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Team / multi-agent coordination" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::team() }
}
