// INPUT:  alva_types (LocalToolContext, Tool, ToolContext, ToolDefinition, ToolRegistry, ToolResult)
// OUTPUT: SrowToolContext, re-exports of alva_types tool types
// POS:    Port re-exports + SrowToolContext concrete impl that bridges both ToolContext and LocalToolContext for the srow application layer.
pub use alva_types::{LocalToolContext, Tool, ToolContext, ToolDefinition, ToolRegistry, ToolResult};

#[derive(Debug, Clone)]
pub struct SrowToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
}

impl alva_types::ToolContext for SrowToolContext {
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn get_config(&self, _key: &str) -> Option<String> {
        None
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn local(&self) -> Option<&dyn alva_types::LocalToolContext> {
        Some(self)
    }
}

impl alva_types::LocalToolContext for SrowToolContext {
    fn workspace(&self) -> &std::path::Path {
        &self.workspace
    }
    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }
}
