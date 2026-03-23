pub use agent_types::{Tool, ToolContext, ToolDefinition, ToolRegistry, ToolResult};

#[derive(Debug, Clone)]
pub struct SrowToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
}

impl agent_types::ToolContext for SrowToolContext {
    fn workspace(&self) -> &std::path::Path {
        &self.workspace
    }
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }
}
