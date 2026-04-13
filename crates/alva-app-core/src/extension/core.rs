//! Core file I/O tools: read, write, edit, search, list.

use alva_kernel_abi::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct CoreExtension;

#[async_trait]
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::file_io() }
}
