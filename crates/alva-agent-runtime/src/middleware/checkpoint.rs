//! Auto-checkpoint middleware — saves file backups before write tools execute.

use std::path::PathBuf;
use std::sync::Arc;

use alva_agent_core::middleware::{Middleware, MiddlewareError};
use alva_agent_core::shared::MiddlewarePriority;
use alva_agent_core::state::AgentState;
use alva_types::{BusHandle, ToolCall};
use async_trait::async_trait;

/// Callback trait for checkpoint creation — implemented by CLI or app.
pub trait CheckpointCallback: Send + Sync {
    /// Create a checkpoint for the given files with a description.
    fn create_checkpoint(&self, description: &str, file_paths: &[PathBuf]);
}

/// Wrapper type stored in Extensions so the middleware can find the callback.
pub struct CheckpointCallbackRef(pub Arc<dyn CheckpointCallback>);

/// Tools that modify files — checkpoint before they run.
const WRITE_TOOLS: &[&str] = &["create_file", "file_edit"];

/// Middleware that auto-checkpoints files before write tools modify them.
pub struct CheckpointMiddleware {
    bus: Option<BusHandle>,
}

impl CheckpointMiddleware {
    pub fn new() -> Self {
        Self { bus: None }
    }

    /// Attach a bus handle so the middleware can look up capabilities (e.g. CheckpointCallbackRef).
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }
}

impl Default for CheckpointMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl Middleware for CheckpointMiddleware {
    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if !WRITE_TOOLS.contains(&tool_call.name.as_str()) {
            return Ok(());
        }

        if let Some(cb) = self.bus.as_ref().and_then(|b| b.get::<CheckpointCallbackRef>()) {
            let mut paths = Vec::new();

            // Extract file path from tool arguments
            if let Some(path_str) = tool_call
                .arguments
                .get("path")
                .or_else(|| tool_call.arguments.get("file_path"))
                .and_then(|v| v.as_str())
            {
                paths.push(PathBuf::from(path_str));
            }

            if !paths.is_empty() {
                let desc = format!(
                    "before {} on {}",
                    tool_call.name,
                    paths
                        .iter()
                        .map(|p| p.display().to_string())
                        .collect::<Vec<_>>()
                        .join(", ")
                );
                cb.0.create_checkpoint(&desc, &paths);
            }
        }

        Ok(()) // Never block — checkpoint is best-effort
    }

    fn name(&self) -> &str {
        "checkpoint"
    }

    fn priority(&self) -> i32 {
        // After security (which may block), before actual execution
        MiddlewarePriority::SECURITY - 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_agent_core::shared::Extensions;
    use alva_types::session::InMemorySession;
    use alva_types::Bus;
    use std::sync::{Arc, Mutex as StdMutex};

    fn make_state() -> AgentState {
        use alva_types::base::error::AgentError;
        use alva_types::base::message::Message;
        use alva_types::base::stream::StreamEvent;
        use alva_types::model::LanguageModel;
        use alva_types::tool::Tool;
        use alva_types::ModelConfig;

        struct StubModel;
        #[async_trait]
        impl LanguageModel for StubModel {
            async fn complete(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> Result<Message, AgentError> {
                unreachable!()
            }
            fn stream(
                &self,
                _: &[Message],
                _: &[&dyn Tool],
                _: &ModelConfig,
            ) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> {
                Box::pin(tokio_stream::empty())
            }
            fn model_id(&self) -> &str {
                "stub"
            }
        }

        AgentState {
            model: Arc::new(StubModel),
            tools: vec![],
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        }
    }

    /// Mock checkpoint callback for testing.
    #[derive(Clone)]
    struct MockCheckpointCallback {
        calls: Arc<StdMutex<Vec<(String, Vec<PathBuf>)>>>,
    }

    impl MockCheckpointCallback {
        fn new() -> Self {
            Self {
                calls: Arc::new(StdMutex::new(Vec::new())),
            }
        }
        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl CheckpointCallback for MockCheckpointCallback {
        fn create_checkpoint(&self, desc: &str, paths: &[PathBuf]) {
            self.calls
                .lock()
                .unwrap()
                .push((desc.to_string(), paths.to_vec()));
        }
    }

    #[tokio::test]
    async fn checkpoint_triggers_on_write_tool() {
        let mock_cb = MockCheckpointCallback::new();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(CheckpointCallbackRef(Arc::new(mock_cb.clone()))));
        let bus_handle = bus.handle();

        let mw = CheckpointMiddleware::new().with_bus(bus_handle);
        let mut state = make_state();

        let tc = ToolCall {
            id: "1".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({ "path": "/tmp/test.txt", "content": "hello" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok());
        assert_eq!(mock_cb.call_count(), 1);
    }

    #[tokio::test]
    async fn checkpoint_skips_read_tools() {
        let mock_cb = MockCheckpointCallback::new();
        let bus = Bus::new();
        let bus_writer = bus.writer();
        bus_writer.provide(Arc::new(CheckpointCallbackRef(Arc::new(mock_cb.clone()))));
        let bus_handle = bus.handle();

        let mw = CheckpointMiddleware::new().with_bus(bus_handle);
        let mut state = make_state();

        let tc = ToolCall {
            id: "2".into(),
            name: "grep_search".into(),
            arguments: serde_json::json!({ "pattern": "test" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok());
        assert_eq!(mock_cb.call_count(), 0);
    }

    #[tokio::test]
    async fn checkpoint_works_without_callback() {
        let mw = CheckpointMiddleware::new();
        let mut state = make_state();
        // No callback registered — should still succeed (best-effort)

        let tc = ToolCall {
            id: "3".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({ "path": "/tmp/test.txt" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok());
    }
}
