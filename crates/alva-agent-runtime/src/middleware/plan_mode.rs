// INPUT:  alva_agent_core::middleware, alva_agent_core::shared, alva_types, async_trait
// OUTPUT: PlanModeMiddleware
// POS:    Blocks non-read-only tools when plan mode is active — read-only analysis only.

use std::sync::atomic::{AtomicBool, Ordering};

use alva_agent_core::middleware::{Middleware, MiddlewareError};
use alva_agent_core::shared::MiddlewarePriority;
use alva_agent_core::state::AgentState;
use alva_types::ToolCall;
use async_trait::async_trait;

/// Middleware that blocks non-read-only tools when plan mode is active.
/// When enabled, the agent can only read and analyze — no modifications.
///
/// Uses `tool.is_read_only(input)` from the Tool trait to determine whether
/// a tool is allowed. No hardcoded tool name list needed.
pub struct PlanModeMiddleware {
    enabled: AtomicBool,
}

impl PlanModeMiddleware {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
        }
    }

    /// Enable or disable plan mode at runtime.
    pub fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }

    /// Check if plan mode is currently enabled.
    pub fn is_enabled(&self) -> bool {
        self.enabled.load(Ordering::Relaxed)
    }
}

#[async_trait]
impl Middleware for PlanModeMiddleware {
    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if !self.enabled.load(Ordering::Relaxed) {
            return Ok(());
        }

        // Look up the tool in the registry to check is_read_only
        let is_read_only = state
            .tools
            .iter()
            .find(|t| t.name() == tool_call.name)
            .map(|t| t.is_read_only(&tool_call.arguments))
            .unwrap_or(false); // unknown tool → treat as write

        if is_read_only {
            Ok(())
        } else {
            Err(MiddlewareError::Blocked {
                reason: format!(
                    "tool '{}' is blocked in Plan mode (read-only). Use /plan to switch to Ask mode.",
                    tool_call.name
                ),
            })
        }
    }

    fn name(&self) -> &str {
        "plan_mode"
    }

    fn priority(&self) -> i32 {
        // Run just after security middleware (which has SECURITY priority)
        MiddlewarePriority::SECURITY + 10
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_agent_core::shared::Extensions;
    use alva_types::session::InMemorySession;
    use alva_types::tool::Tool;
    use alva_types::{AgentError, ToolOutput};
    use std::sync::Arc;

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

    /// A tool that declares itself read-only.
    struct ReadOnlyTool {
        tool_name: &'static str,
    }
    #[async_trait]
    impl Tool for ReadOnlyTool {
        fn name(&self) -> &str {
            self.tool_name
        }
        fn description(&self) -> &str {
            "read-only test tool"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        fn is_read_only(&self, _input: &serde_json::Value) -> bool {
            true
        }
        async fn execute(
            &self,
            _: serde_json::Value,
            _: &dyn alva_types::ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    /// A tool that is NOT read-only (default).
    struct WriteTool {
        tool_name: &'static str,
    }
    #[async_trait]
    impl Tool for WriteTool {
        fn name(&self) -> &str {
            self.tool_name
        }
        fn description(&self) -> &str {
            "write test tool"
        }
        fn parameters_schema(&self) -> serde_json::Value {
            serde_json::json!({})
        }
        async fn execute(
            &self,
            _: serde_json::Value,
            _: &dyn alva_types::ToolExecutionContext,
        ) -> Result<ToolOutput, AgentError> {
            Ok(ToolOutput::text("ok"))
        }
    }

    fn test_tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(ReadOnlyTool { tool_name: "grep_search" }),
            Arc::new(ReadOnlyTool { tool_name: "list_files" }),
            Arc::new(ReadOnlyTool { tool_name: "read_file" }),
            Arc::new(WriteTool { tool_name: "execute_shell" }),
            Arc::new(WriteTool { tool_name: "create_file" }),
            Arc::new(WriteTool { tool_name: "file_edit" }),
            Arc::new(WriteTool { tool_name: "notebook_edit" }),
        ]
    }

    #[tokio::test]
    async fn plan_mode_blocks_write_tools() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());

        for tool_name in &["execute_shell", "create_file", "file_edit", "notebook_edit"] {
            let tc = ToolCall {
                id: "1".into(),
                name: tool_name.to_string(),
                arguments: serde_json::json!({}),
            };
            let result = mw.before_tool_call(&mut state, &tc).await;
            assert!(result.is_err(), "plan mode should block {}", tool_name);
        }
    }

    #[tokio::test]
    async fn plan_mode_allows_read_tools() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());

        for tool_name in &["grep_search", "list_files", "read_file"] {
            let tc = ToolCall {
                id: "2".into(),
                name: tool_name.to_string(),
                arguments: serde_json::json!({}),
            };
            let result = mw.before_tool_call(&mut state, &tc).await;
            assert!(result.is_ok(), "plan mode should allow {}", tool_name);
        }
    }

    #[tokio::test]
    async fn plan_mode_disabled_allows_everything() {
        let mw = PlanModeMiddleware::new(false);
        let mut state = make_state_with_tools(test_tools());

        let tc = ToolCall {
            id: "3".into(),
            name: "execute_shell".into(),
            arguments: serde_json::json!({}),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_ok(), "disabled plan mode should allow write tools");
    }

    #[tokio::test]
    async fn plan_mode_toggle() {
        let mw = PlanModeMiddleware::new(false);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall {
            id: "4".into(),
            name: "create_file".into(),
            arguments: serde_json::json!({}),
        };

        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());

        mw.set_enabled(true);
        assert!(mw.before_tool_call(&mut state, &tc).await.is_err());

        mw.set_enabled(false);
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
    }

    #[tokio::test]
    async fn unknown_tool_treated_as_write() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());

        let tc = ToolCall {
            id: "5".into(),
            name: "unknown_tool".into(),
            arguments: serde_json::json!({}),
        };
        let result = mw.before_tool_call(&mut state, &tc).await;
        assert!(result.is_err(), "unknown tool should be blocked in plan mode");
    }
}
