//! Web tools: internet search, URL fetching.

use alva_types::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct WebExtension;

#[async_trait]
impl Extension for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Web tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::web() }
}
