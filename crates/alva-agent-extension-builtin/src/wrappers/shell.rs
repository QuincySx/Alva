//! Shell execution tool.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct ShellExtension;

#[async_trait]
impl Plugin for ShellExtension {
    fn name(&self) -> &str { "shell" }
    fn description(&self) -> &str { "Shell execution" }
    async fn register(&self, r: &Registrar) { r.tools(crate::tool_presets::shell()); }
}
