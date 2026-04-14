//! Browser automation extension.
//!
//! The actual chromiumoxide-based implementation lives in the dedicated
//! `alva-app-extension-browser` crate — heavy, native-only, intentionally
//! excluded from the agent/kernel layers. This file is just the thin
//! `Extension` wrapper that bridges the standalone crate's tools into
//! the app-core extension registry.

use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

use super::Extension;

pub struct BrowserExtension;

#[async_trait]
impl Extension for BrowserExtension {
    fn name(&self) -> &str { "browser" }
    fn description(&self) -> &str { "Browser automation tools (Chrome via CDP)" }
    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        alva_app_extension_browser::browser_tools()
    }
}
