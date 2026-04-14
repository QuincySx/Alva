//! Utility tools: sleep, config, notebook, skill, tool_search, schedule, remote.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct UtilityExtension;

#[async_trait]
impl Extension for UtilityExtension {
    fn name(&self) -> &str { "utility" }
    fn description(&self) -> &str { "Utility tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::utility() }
}
