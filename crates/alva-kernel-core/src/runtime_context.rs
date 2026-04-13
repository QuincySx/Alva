// INPUT:  std::any::Any, std::path::{Path, PathBuf}, std::sync::Arc,
//         alva_kernel_abi::{BusHandle, CancellationToken, ProgressEvent, ToolExecutionContext, ToolFs},
//         tokio::sync::mpsc, crate::event::AgentEvent
// OUTPUT: RuntimeExecutionContext
// POS:    Concrete ToolExecutionContext used by the agent run loop — bridges tool progress to agent events and exposes bus to tools.
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::tool::execution::{ProgressEvent, ToolExecutionContext};
use alva_kernel_abi::BusHandle;
use alva_kernel_abi::ToolFs;
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
    bus: Option<BusHandle>,
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
            bus: None,
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

    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
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

    fn tool_call_id(&self) -> Option<&str> {
        Some(&self.tool_call_id)
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

    fn bus(&self) -> Option<&BusHandle> {
        self.bus.as_ref()
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::tool::execution::ToolExecutionContext;

    /// Helper: create channel and return (sender, receiver).
    fn channel() -> (
        mpsc::UnboundedSender<AgentEvent>,
        mpsc::UnboundedReceiver<AgentEvent>,
    ) {
        mpsc::unbounded_channel()
    }

    #[test]
    fn new_defaults() {
        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        );
        assert!(ctx.workspace().is_none());
        assert!(!ctx.allow_dangerous());
        assert!(ctx.tool_fs().is_none());
        assert!(ctx.bus().is_none());
    }

    #[test]
    fn with_workspace_sets_workspace() {
        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        )
        .with_workspace(PathBuf::from("/tmp/project"));

        assert_eq!(ctx.workspace().unwrap(), Path::new("/tmp/project"));
    }

    #[test]
    fn with_bus_sets_and_returns_bus() {
        let bus = alva_kernel_abi::Bus::new();
        let handle = bus.handle();

        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        )
        .with_bus(handle);

        assert!(ctx.bus().is_some());
    }

    #[test]
    fn with_allow_dangerous_true() {
        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        )
        .with_allow_dangerous(true);

        assert!(ctx.allow_dangerous());
    }

    #[test]
    fn with_tool_fs_works() {
        use alva_kernel_abi::base::error::AgentError;
        use alva_kernel_abi::tool::{ToolFsDirEntry, ToolFsExecResult};
        use async_trait::async_trait;

        struct DummyFs;

        #[async_trait]
        impl ToolFs for DummyFs {
            async fn exec(
                &self,
                _cmd: &str,
                _cwd: Option<&str>,
                _timeout: u64,
            ) -> Result<ToolFsExecResult, AgentError> {
                unimplemented!()
            }
            async fn read_file(&self, _path: &str) -> Result<Vec<u8>, AgentError> {
                unimplemented!()
            }
            async fn write_file(&self, _path: &str, _content: &[u8]) -> Result<(), AgentError> {
                unimplemented!()
            }
            async fn list_dir(&self, _path: &str) -> Result<Vec<ToolFsDirEntry>, AgentError> {
                unimplemented!()
            }
            async fn exists(&self, _path: &str) -> Result<bool, AgentError> {
                unimplemented!()
            }
        }

        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        )
        .with_tool_fs(Arc::new(DummyFs));

        assert!(ctx.tool_fs().is_some());
    }

    #[test]
    fn cancel_token_returns_provided_token() {
        let token = CancellationToken::new();
        assert!(!token.is_cancelled());
        token.cancel();

        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(token, "call-1".into(), tx, "sess-1".into());

        assert!(ctx.cancel_token().is_cancelled());
    }

    #[test]
    fn session_id_returns_provided_string() {
        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "my-session-42".into(),
        );

        assert_eq!(ctx.session_id(), "my-session-42");
    }

    #[test]
    fn report_progress_sends_event() {
        let (tx, mut rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-99".into(),
            tx,
            "sess-1".into(),
        );

        ctx.report_progress(ProgressEvent::Status {
            message: "compiling...".into(),
        });

        let event = rx.try_recv().expect("should receive an event");
        match event {
            AgentEvent::ToolExecutionUpdate {
                tool_call_id,
                event,
            } => {
                assert_eq!(tool_call_id, "call-99");
                match event {
                    ProgressEvent::Status { message } => {
                        assert_eq!(message, "compiling...");
                    }
                    other => panic!("unexpected progress event variant: {:?}", other),
                }
            }
            other => panic!("unexpected agent event variant: {:?}", other),
        }
    }

    #[test]
    fn as_any_returns_self() {
        let (tx, _rx) = channel();
        let ctx = RuntimeExecutionContext::new(
            CancellationToken::new(),
            "call-1".into(),
            tx,
            "sess-1".into(),
        );

        let any = ctx.as_any();
        assert!(any.downcast_ref::<RuntimeExecutionContext>().is_some());
    }
}
