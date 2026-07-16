//! Shell execution and host-escalation request tools.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct ShellPlugin;

#[async_trait]
impl Plugin for ShellPlugin {
    fn name(&self) -> &str {
        "shell"
    }
    fn description(&self) -> &str {
        "Shell execution and permission-gated host escalation"
    }
    async fn register(&self, r: &Registrar) {
        r.tools(crate::tool_presets::shell());
    }
}
