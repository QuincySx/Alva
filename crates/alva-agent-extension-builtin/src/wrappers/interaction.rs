//! Human interaction tool (ask_human).

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct InteractionPlugin;

#[async_trait]
impl Plugin for InteractionPlugin {
    fn name(&self) -> &str { "interaction" }
    fn description(&self) -> &str { "Human interaction" }
    async fn register(&self, r: &Registrar) { r.tools(crate::tool_presets::interaction()); }
}
