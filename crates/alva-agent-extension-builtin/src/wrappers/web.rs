//! Web tools: internet search, URL fetching.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct WebExtension;

#[async_trait]
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Web tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::web() }
}
