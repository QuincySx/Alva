//! Browser automation extension wrapper.
//!
//! The actual chromiumoxide-based implementation lives in the dedicated
//! `alva-app-extension-browser` crate — heavy, native-only, intentionally
//! excluded from the agent/kernel layers. This file is the thin
//! `Plugin` wrapper that bridges the standalone crate's tools into
//! the plugin registry.

use alva_agent_core::extension::{Plugin, Registrar};
use async_trait::async_trait;

pub struct BrowserExtension;

#[async_trait]
impl Plugin for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation tools (Chrome via CDP)" }
    async fn register(&self, r: &Registrar) {
        r.tools(alva_app_extension_browser::browser_tools());
    }
}
