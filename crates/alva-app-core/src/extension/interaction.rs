//! Human interaction tool (ask_human).

use alva_types::tool::Tool;
use alva_agent_tools::tool_presets;
use async_trait::async_trait;

use super::Extension;

pub struct InteractionExtension;

#[async_trait]
impl Extension for InteractionExtension {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> { tool_presets::interaction() }
}
