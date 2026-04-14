//! Human interaction tool (ask_human).

use alva_agent_core::extension::Extension;
use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

pub struct InteractionExtension;

#[async_trait]
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { crate::tool_presets::interaction() }
}
