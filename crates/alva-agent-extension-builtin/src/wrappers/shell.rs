//! Shell execution tool.

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct ShellExtension;

#[async_trait]
impl Extension for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell execution" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::shell() }
}
