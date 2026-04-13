//! Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct UtilityExtension;

#[async_trait]
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::utility() }
}
