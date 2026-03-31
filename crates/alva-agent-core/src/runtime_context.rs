// INPUT:  std::any::Any, std::path::{Path, PathBuf}, std::sync::Arc,
//         alva_types::{CancellationToken, ProgressEvent, ToolExecutionContext, ToolFs},
//         tokio::sync::mpsc, crate::event::AgentEvent
// OUTPUT: RuntimeExecutionContext
// POS:    Concrete ToolExecutionContext used by the agent run loop — bridges tool progress to agent events.
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_types::base::cancel::CancellationToken;
use alva_types::tool::execution::{ProgressEvent, ToolExecutionContext};
use alva_types::ToolFs;
use tokio::sync::mpsc;

use crate::event::AgentEvent;

/// Concrete `ToolExecutionContext` used by the agent run loop.
///
/// Forwards `report_progress` calls as `AgentEvent::ToolExecutionUpdate`
/// events so the UI layer can display real-time tool output without
/// waiting for the final `ToolOutput`.
pub struct RuntimeExecutionContext {
    cancel: CancellationToken,
    tool_call_id: String,
    event_tx: mpsc::UnboundedSender<AgentEvent>,
    session_id: String,
    workspace: Option<PathBuf>,
    allow_dangerous: bool,
    tool_fs: Option<Arc<dyn ToolFs>>,
}

impl RuntimeExecutionContext {
    pub fn new(
        cancel: CancellationToken,
        tool_call_id: String,
        event_tx: mpsc::UnboundedSender<AgentEvent>,
        session_id: String,
    ) -> Self {
        Self {
            cancel,
            tool_call_id,
            event_tx,
            session_id,
            workspace: None,
            allow_dangerous: false,
            tool_fs: None,
        }
    }

    pub fn with_workspace(mut self, path: PathBuf) -> Self {
        self.workspace = Some(path);
        self
    }

    pub fn with_allow_dangerous(mut self, allow: bool) -> Self {
        self.allow_dangerous = allow;
        self
    }

    pub fn with_tool_fs(mut self, fs: Arc<dyn ToolFs>) -> Self {
        self.tool_fs = Some(fs);
        self
    }
}

impl ToolExecutionContext for RuntimeExecutionContext {
    fn cancel_token(&self) -> &CancellationToken {
        &self.cancel
    }

    fn report_progress(&self, event: ProgressEvent) {
        let _ = self.event_tx.send(AgentEvent::ToolExecutionUpdate {
            tool_call_id: self.tool_call_id.clone(),
            event,
        });
    }

    fn session_id(&self) -> &str {
        &self.session_id
    }

    fn workspace(&self) -> Option<&Path> {
        self.workspace.as_deref()
    }

    fn allow_dangerous(&self) -> bool {
        self.allow_dangerous
    }

    fn tool_fs(&self) -> Option<&dyn ToolFs> {
        self.tool_fs.as_deref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
