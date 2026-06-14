//! Web tools: internet search, URL fetching.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct WebExtension;

#[async_trait]
impl Plugin for WebExtension {
    fn name(&self) -> &str { "web" }
    fn description(&self) -> &str { "Web tools" }
    async fn register(&self, r: &Registrar) { r.tools(crate::tool_presets::web()); }
}
