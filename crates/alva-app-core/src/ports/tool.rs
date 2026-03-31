// INPUT:  alva_types (Tool, ToolDefinition, ToolRegistry, ToolExecutionContext, ToolOutput)
// OUTPUT: SrowToolContext, re-exports of alva_types tool types
// POS:    Port re-exports + SrowToolContext concrete impl that bridges ToolExecutionContext for the alva application layer.
#[allow(unused_imports)]
pub use alva_types::{Tool, ToolDefinition, ToolExecutionContext, ToolOutput, ToolRegistry};

use alva_types::CancellationToken;

#[allow(dead_code)]
#[derive(Clone)]
pub struct SrowToolContext {
    pub session_id: String,
    pub workspace: std::path::PathBuf,
    pub allow_dangerous: bool,
    pub cancel: CancellationToken,
}

impl std::fmt::Debug for SrowToolContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SrowToolContext")
            .field("session_id", &self.session_id)
            .field("workspace", &self.workspace)
            .field("allow_dangerous", &self.allow_dangerous)
            .finish()
    }
}

impl SrowToolContext {
    pub fn new(session_id: impl Into<String>, workspace: impl Into<std::path::PathBuf>) -> Self {
        Self {
            session_id: session_id.into(),
            workspace: workspace.into(),
            allow_dangerous: false,
            cancel: CancellationToken::new(),
        }
    }
}

impl alva_types::ToolExecutionContext for SrowToolContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }
    fn session_id(&self) -> &str {
        &self.session_id
    }
    fn workspace(&self) -> Option<&std::path::Path> {
        Some(&self.workspace)
    }
    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
