//! Browser automation tools.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct BrowserExtension;

#[async_trait]
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::browser_tools() }
}
