// INPUT:  alva_agent_core::middleware, alva_types::BusHandle, std::sync::Arc
// OUTPUT: CheckpointMiddleware, CheckpointCallback (trait), CheckpointCallbackRef
// POS:    Auto-checkpoints files before non-read-only tools execute — reads CheckpointCallbackRef from bus.
//! Auto-checkpoint middleware — saves file backups before write tools execute.
//!
//! Uses `tool.is_read_only(input)` from the Tool trait to determine whether
//! a checkpoint is needed. No hardcoded tool name list.

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

/// Middleware that auto-checkpoints files before non-read-only tools modify them.
pub struct CheckpointMiddleware {
    bus: Option<BusHandle>,
    /// Settings-based read_only patterns for dynamic tools.
    read_only_patterns: Vec<String>,
}

impl CheckpointMiddleware {
    pub fn new() -> Self {
        Self {
            bus: None,
            read_only_patterns: Vec::new(),
        }
    }

    /// Attach a bus handle so the middleware can look up capabilities (e.g. CheckpointCallbackRef).
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }

    /// Set read_only patterns from settings.
    pub fn with_read_only_patterns(mut self, patterns: Vec<String>) -> Self {
        self.read_only_patterns = patterns;
        self
    }

    fn matches_read_only_pattern(&self, tool_name: &str) -> bool {
        for pattern in &self.read_only_patterns {
            if pattern == tool_name || pattern == "*" {
                return true;
            }
            if pattern.ends_with('*') {
                let prefix = &pattern[..pattern.len() - 1];
                if tool_name.starts_with(prefix) {
                    return true;
                }
            }
        }
        false
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
        // Check read-only: settings patterns first, then Tool trait
        if self.matches_read_only_pattern(&tool_call.name) {
            return Ok(());
        }
        let is_read_only = state
            .tools
            .iter()
            .find(|t| t.name() == tool_call.name)
            .map(|t| t.is_read_only(&tool_call.arguments))
            .unwrap_or(false);
        if is_read_only {
            return Ok(());
        }

        if let Some(cb) = self.bus.as_ref().and_then(|b| b.get::<CheckpointCallbackRef>()) {
            let mut paths = Vec::new();

            // Extract file path from tool arguments
            if let Some(path_str) = tool_call
                .arguments
                .get("path")
                .or_else(|| tool_call.arguments.get("file_path"))
                .or_else(|| tool_call.arguments.get("notebook_path"))
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
    use alva_types::tool::Tool;
    use alva_types::{AgentError, Bus, ToolOutput};
    use std::sync::{Arc, Mutex as StdMutex};

    fn make_state_with_tools(tools: Vec<Arc<dyn Tool>>) -> AgentState {
        use alva_types::base::message::Message;
        use alva_types::base::stream::StreamEvent;
        use alva_types::model::LanguageModel;
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
            tools,
            session: Arc::new(InMemorySession::new()),
            extensions: Extensions::new(),
        }
    }

    struct ReadOnlyTool(&'static str);
    #[async_trait]
    impl Tool for ReadOnlyTool {
        fn name(&self) -> &str { self.0 }
        fn description(&self) -> &str { "read-only" }
        fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        fn is_read_only(&self, _: &serde_json::Value) -> bool { true }
        async fn execute(&self, _: serde_json::Value, _: &dyn alva_types::ToolExecutionContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    struct WriteTool(&'static str);
    #[async_trait]
    impl Tool for WriteTool {
        fn name(&self) -> &str { self.0 }
        fn description(&self) -> &str { "write" }
        fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        async fn execute(&self, _: serde_json::Value, _: &dyn alva_types::ToolExecutionContext) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    fn test_tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(ReadOnlyTool("grep_search")),
            Arc::new(WriteTool("create_file")),
        ]
    }

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
        let mut state = make_state_with_tools(test_tools());

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
        let mut state = make_state_with_tools(test_tools());

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
        let mut state = make_state_with_tools(test_tools());

        let tc = ToolCall {
            id: "3".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({ "path": "/tmp/test.txt" }),
        };

        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok());
    }
}
