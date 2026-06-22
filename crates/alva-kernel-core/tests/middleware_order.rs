use std::sync::Arc;

use alva_kernel_abi::agent_session::{AgentSession, InMemoryAgentSession};
use alva_kernel_abi::{AgentError, ToolCall, ToolOutput};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, MiddlewareStack, ToolCallFn};
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::AgentState;
use alva_test::mock_provider::MockLanguageModel;
use async_trait::async_trait;

struct AppendArgMiddleware {
    label: &'static str,
}

#[async_trait]
impl Middleware for AppendArgMiddleware {
    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        let mut modified = tool_call.clone();
        let current = modified
            .arguments
            .get("value")
            .and_then(|value| value.as_str())
            .unwrap_or("");
        modified.arguments = serde_json::json!({
            "value": format!("{current}{}", self.label),
        });
        next.call(state, &modified)
            .await
            .map_err(MiddlewareError::from)
    }
}

struct AppendResultMiddleware {
    label: &'static str,
}

#[async_trait]
impl Middleware for AppendResultMiddleware {
    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        *result = ToolOutput::text(format!("{}{}", result.model_text(), self.label));
        Ok(())
    }
}

struct EchoToolCall;

#[async_trait]
impl ToolCallFn for EchoToolCall {
    async fn call(
        &self,
        _state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput::text(
            tool_call
                .arguments
                .get("value")
                .and_then(|value| value.as_str())
                .unwrap_or("")
                .to_string(),
        ))
    }
}

fn state() -> AgentState {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    AgentState {
        model: Arc::new(MockLanguageModel::new()),
        tools: Vec::new(),
        session,
        extensions: Extensions::new(),
    }
}

fn tool_call(value: &str) -> ToolCall {
    ToolCall {
        id: "call-1".to_string(),
        name: "echo".to_string(),
        arguments: serde_json::json!({ "value": value }),
    }
}

#[tokio::test]
async fn wrap_tool_call_runs_top_to_bottom_for_argument_mutation() {
    let mut stack = MiddlewareStack::new();
    stack.push(Arc::new(AppendArgMiddleware { label: "A" }));
    stack.push(Arc::new(AppendArgMiddleware { label: "B" }));
    let mut state = state();

    let result = stack
        .run_wrap_tool_call(&mut state, &tool_call(""), &EchoToolCall)
        .await
        .expect("wrap chain should succeed");

    assert_eq!(result.model_text(), "AB");
}

#[tokio::test]
async fn after_tool_call_runs_bottom_to_top_for_result_mutation() {
    let mut stack = MiddlewareStack::new();
    stack.push(Arc::new(AppendResultMiddleware { label: "A" }));
    stack.push(Arc::new(AppendResultMiddleware { label: "B" }));
    let mut state = state();
    let mut result = ToolOutput::text("");

    stack
        .run_after_tool_call(&mut state, &tool_call(""), &mut result)
        .await
        .expect("after chain should succeed");

    assert_eq!(result.model_text(), "BA");
}
