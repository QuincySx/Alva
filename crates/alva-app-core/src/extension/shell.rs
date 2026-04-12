//! Shell execution tool.

use alva_types::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct ShellExtension;

#[async_trait]
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell execution" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::shell() }
}
