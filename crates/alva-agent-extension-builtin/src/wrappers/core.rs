//! Core file I/O tools: read, write, edit, search, list.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct CoreExtension;

#[async_trait]
impl Plugin for CoreExtension {
    fn name(&self) -> &str { "core" }
    fn description(&self) -> &str { "Core file I/O tools" }
    async fn register(&self, r: &Registrar) { r.tools(crate::tool_presets::file_io()); }
}
