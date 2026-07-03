//! `Plugin` wrapper bridging this crate's browser tools into the plugin
//! registry.
//!
//! Lives HERE (the app extension layer) and not in
//! `alva-agent-extension-builtin`: the SDK tool crate must never depend on
//! an `alva-app-*` crate, not even behind an optional feature — app-core
//! turns those features on in every real build, which silently breached the
//! SDK→app boundary until the dependency firewall learned `--all-features`.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct BrowserPlugin;

#[async_trait]
impl Plugin for BrowserPlugin {
    fn name(&self) -> &str {
        "browser"
    }
    fn description(&self) -> &str {
        "Browser automation tools (Chrome via CDP)"
    }
    async fn register(&self, r: &Registrar) {
        r.tools(crate::browser_tools());
    }
}
