// INPUT:  std::any::Any, std::path::{Path, PathBuf}, std::sync::Arc,
//         alva_kernel_abi::{BusHandle, CancellationToken, ProgressEvent, ToolExecutionContext, ToolFs},
//         crate::event::{AgentEvent, AgentEventSender}
// OUTPUT: RuntimeExecutionContext
// POS:    Tool context bridge; progress uses non-blocking best-effort event delivery while terminal events stay lossless.
use std::any::Any;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::event::{AgentEvent, AgentEventSender};
use alva_kernel_abi::agent_session::ScopedSession;
use alva_kernel_abi::base::cancel::CancellationToken;
use alva_kernel_abi::tool::execution::{ProgressEvent, ToolExecutionContext};
use alva_kernel_abi::BusHandle;
use alva_kernel_abi::StoreSessionCell;
use alva_kernel_abi::ToolFs;

/// Concrete `ToolExecutionContext` used by the agent run loop.
///
/// Forwards `report_progress` calls as `AgentEvent::ToolExecutionUpdate`
/// events so the UI layer can display real-time tool output without
/// waiting for the final `ToolOutput`.
pub struct RuntimeExecutionContext {
    cancel: CancellationToken,
    tool_call_id: String,
    event_tx: AgentEventSender,
    session_id: String,
    workspace: Option<PathBuf>,
    allow_dangerous: bool,
    tool_fs: Option<Arc<dyn ToolFs>>,
    bus: Option<BusHandle>,
    pub session: Option<ScopedSession>,
    /// R05/R09/R10: 完整会话（SessionStore 能力）——装配层经共享 cell 显式
    /// 提供；需要 create_child session / 查询 / 压缩的工具从这里取，且总是
    /// 拿到**当前**会话（swap 原地更新 cell）。最小消费者不设置。
    pub store_session: Option<Arc<StoreSessionCell>>,
}

impl RuntimeExecutionContext {
    pub fn new<E>(
        cancel: CancellationToken,
        tool_call_id: String,
        event_tx: E,
        session_id: String,
    ) -> Self
    where
        E: Into<AgentEventSender>,
    {
        Self {
            store_session: None,
            cancel,
            tool_call_id,
            event_tx: event_tx.into(),
            session_id,
            workspace: None,
            allow_dangerous: false,
            tool_fs: None,
            bus: None,
            session: None,
        }
    }

    /// R05/R09: attach the shared full-session cell (SessionStore 能力)。
    pub fn with_store_session(mut self, cell: Arc<StoreSessionCell>) -> Self {
        self.store_session = Some(cell);
        self
    }

    /// 当前完整会话——cell 读取时解析，swap 后即为最新。
    pub fn store_session(&self) -> Option<Arc<dyn alva_kernel_abi::agent_session::AgentSession>> {
        self.store_session.as_ref().map(|cell| cell.get())
    }

    /// 共享 cell 本体——嵌套 spawn 原样传给 child extensions。
    pub fn store_session_cell(&self) -> Option<Arc<StoreSessionCell>> {
        self.store_session.clone()
    }

    pub fn with_session(mut self, session: ScopedSession) -> Self {
        self.session = Some(session);
        self
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
        if self
            .event_tx
            .try_send(AgentEvent::ToolExecutionUpdate {
                tool_call_id: self.tool_call_id.clone(),
                event,
            })
            .is_err()
        {
            tracing::trace!(
                tool_call_id = %self.tool_call_id,
                "tool progress event dropped because the bounded event stream is full or closed"
            );
        }
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

    fn store_session(
        &self,
    ) -> Option<std::sync::Arc<dyn alva_kernel_abi::agent_session::AgentSession>> {
        self.store_session.as_ref().map(|cell| cell.get())
    }

    fn session(&self) -> Option<&alva_kernel_abi::agent_session::ScopedSession> {
        self.session.as_ref()
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
    use tokio::sync::mpsc;

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
