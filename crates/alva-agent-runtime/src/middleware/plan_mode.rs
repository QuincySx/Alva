// INPUT:  alva_agent_core::middleware, alva_agent_core::shared, alva_types, async_trait
// OUTPUT: PlanModeMiddleware
// POS:    Blocks write/execute tools when plan mode is active — read-only analysis only.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};

use alva_agent_core::middleware::{Middleware, MiddlewareError};
use alva_agent_core::shared::MiddlewarePriority;
use alva_agent_core::state::AgentState;
use alva_types::ToolCall;
use async_trait::async_trait;

use super::WRITE_TOOL_NAMES;

/// Middleware that blocks write/execute tools when plan mode is active.
/// When enabled, the agent can only read and analyze — no modifications.
pub struct PlanModeMiddleware {
    enabled: AtomicBool,
    write_tools: HashSet<&'static str>,
}

impl PlanModeMiddleware {
    /// Additional plan-mode-only blocked tools (beyond the shared WRITE_TOOL_NAMES).
    const EXTRA_BLOCKED: &[&str] = &["browser_action", "browser_navigate", "browser_start"];

    pub fn new(enabled: bool) -> Self {
        let write_tools: HashSet<&'static str> = WRITE_TOOL_NAMES
            .iter()
            .chain(Self::EXTRA_BLOCKED.iter())
            .copied()
            .collect();
        Self {
            enabled: AtomicBool::new(enabled),
            write_tools,
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
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.enabled.load(Ordering::Relaxed)
            && self.write_tools.contains(tool_call.name.as_str())
        {
            Err(MiddlewareError::Blocked {
                reason: format!(
                    "tool '{}' is blocked in Plan mode (read-only). Use /plan to switch to Ask mode.",
                    tool_call.name
                ),
            })
        } else {
            Ok(())
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
    use std::sync::Arc;

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

    #[tokio::test]
    async fn plan_mode_blocks_write_tools() {
        let mw = PlanModeMiddleware::new(true);
        let mut state = make_state();

        for tool_name in &["execute_shell", "create_file", "file_edit"] {
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
        let mut state = make_state();

        for tool_name in &["grep_search", "list_files", "view_image", "read_url"] {
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
        let mut state = make_state();

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
        let mut state = make_state();
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
}
