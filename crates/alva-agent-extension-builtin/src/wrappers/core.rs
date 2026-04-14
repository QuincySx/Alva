//! Core file I/O tools: read, write, edit, search, list.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct CoreExtension;

#[async_trait]
impl Extension for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::file_io() }
}
