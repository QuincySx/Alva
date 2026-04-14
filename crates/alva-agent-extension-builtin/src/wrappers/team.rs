//! Team / multi-agent coordination tools.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct TeamExtension;

#[async_trait]
impl Extension for TeamExtension {
    fn name(&self) -> &str { "team" }
    fn description(&self) -> &str { "Team / multi-agent coordination" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::team() }
}
