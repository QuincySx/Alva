//! Language Server Protocol management.

use async_trait::async_trait;

use super::Extension;

pub struct LspExtension;

#[async_trait]
impl Extension for LspExtension {
    fn name(&self) -> &str { "lsp" }
    fn description(&self) -> &str { "Language server management and diagnostics" }
}
