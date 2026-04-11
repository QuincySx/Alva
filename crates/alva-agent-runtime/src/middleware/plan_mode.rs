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
/// Read-only determination (checked in order):
/// 1. Settings-based `read_only` patterns (from `PermissionRules`)
/// 2. `tool.is_read_only(input)` from the Tool trait
/// 3. Unknown tools default to non-read-only (blocked)
pub struct PlanModeMiddleware {
    enabled: AtomicBool,
    /// Settings-based read_only patterns for dynamic tools.
    read_only_patterns: Vec<String>,
}

impl PlanModeMiddleware {
    pub fn new(enabled: bool) -> Self {
        Self {
            enabled: AtomicBool::new(enabled),
            read_only_patterns: Vec::new(),
        }
    }

    /// Set read_only patterns from settings (e.g. `["mcp:context7:*"]`).
    pub fn with_read_only_patterns(mut self, patterns: Vec<String>) -> Self {
        self.read_only_patterns = patterns;
        self
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

/// Trait for runtime plan mode control, exposed via Bus.
pub trait PlanModeControl: Send + Sync {
    fn set_enabled(&self, enabled: bool);
}

impl PlanModeControl for PlanModeMiddleware {
    fn set_enabled(&self, enabled: bool) {
        self.enabled.store(enabled, Ordering::Relaxed);
    }
}

impl PlanModeMiddleware {

    /// Check if a tool name matches any read_only pattern from settings.
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

        // 1. Check settings-based read_only patterns first
        if self.matches_read_only_pattern(&tool_call.name) {
            return Ok(());
        }

        // 2. Check Tool trait is_read_only()
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
            async fn complete(&self, _: &[Message], _: &[&dyn Tool], _: &ModelConfig) -> Result<Message, AgentError> { unreachable!() }
            fn stream(&self, _: &[Message], _: &[&dyn Tool], _: &ModelConfig) -> std::pin::Pin<Box<dyn futures_core::Stream<Item = StreamEvent> + Send>> { Box::pin(tokio_stream::empty()) }
            fn model_id(&self) -> &str { "stub" }
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
        async fn execute(&self, _: serde_json::Value, _: &dyn alva_types::ToolExecutionContext) -> Result<ToolOutput, AgentError> { Ok(ToolOutput::text("ok")) }
    }

    struct WriteTool(&'static str);
    #[async_trait]
    impl Tool for WriteTool {
        fn name(&self) -> &str { self.0 }
        fn description(&self) -> &str { "write" }
        fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        async fn execute(&self, _: serde_json::Value, _: &dyn alva_types::ToolExecutionContext) -> Result<ToolOutput, AgentError> { Ok(ToolOutput::text("ok")) }
    }

    struct McpTool(&'static str);
    #[async_trait]
    impl Tool for McpTool {
        fn name(&self) -> &str { self.0 }
        fn description(&self) -> &str { "mcp tool" }
        fn parameters_schema(&self) -> serde_json::Value { serde_json::json!({}) }
        async fn execute(&self, _: serde_json::Value, _: &dyn alva_types::ToolExecutionContext) -> Result<ToolOutput, AgentError> { Ok(ToolOutput::text("ok")) }
    }

    fn test_tools() -> Vec<Arc<dyn Tool>> {
        vec![
            Arc::new(ReadOnlyTool("grep_search")),
            Arc::new(WriteTool("execute_shell")),
            Arc::new(WriteTool("create_file")),
            Arc::new(McpTool("mcp:context7:query-docs")),
            Arc::new(McpTool("mcp:evil-server:run-code")),
        ]
    }

    #[tokio::test]
    async fn blocks_write_tools() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "1".into(), name: "execute_shell".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_err());
    }

    #[tokio::test]
    async fn allows_read_only_tools() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "2".into(), name: "grep_search".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
    }

    #[tokio::test]
    async fn mcp_tool_blocked_by_default() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "3".into(), name: "mcp:context7:query-docs".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_err());
    }

    #[tokio::test]
    async fn mcp_tool_allowed_by_read_only_pattern() {
        let mw = PlanModeMiddleware::new(true)
            .with_read_only_patterns(vec!["mcp:context7:*".to_string()]);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "4".into(), name: "mcp:context7:query-docs".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
        let tc = ToolCall { id: "5".into(), name: "mcp:evil-server:run-code".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_err());
    }

    #[tokio::test]
    async fn disabled_allows_everything() {
        let mw = PlanModeMiddleware::new(false);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "7".into(), name: "execute_shell".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
    }

    #[tokio::test]
    async fn toggle() {
        let mw = PlanModeMiddleware::new(false);
        let mut state = make_state_with_tools(test_tools());
        let tc = ToolCall { id: "8".into(), name: "create_file".into(), arguments: serde_json::json!({}) };
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
        mw.set_enabled(true);
        assert!(mw.before_tool_call(&mut state, &tc).await.is_err());
        mw.set_enabled(false);
        assert!(mw.before_tool_call(&mut state, &tc).await.is_ok());
    }
}
