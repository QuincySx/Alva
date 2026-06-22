//! Runtime phase contribution metadata and executable phase handlers.
//!
//! This is the common assembly product for plugin lifecycle hooks. The
//! kernel still executes middleware today; agent-core records phase
//! contributions now so plugins can target the stable timeline instead of
//! smuggling every hook through an ad-hoc middleware bridge.

use std::sync::Arc;

use alva_kernel_abi::{AgentMessage, Message, Phase, PhaseEffect, ToolCall, ToolOutput};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, ToolCallFn};
use alva_kernel_core::state::AgentState;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};

/// Coarse ordering tier for phase contributions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PhaseOrder {
    System,
    Security,
    Hooks,
    Policy,
    Normal,
    Telemetry,
    Finalizer,
}

impl PhaseOrder {
    /// Map the semantic tier to the existing middleware priority scale while
    /// kernel phase execution is still backed by `MiddlewareStack`.
    pub fn priority(self) -> i32 {
        match self {
            PhaseOrder::System => 0,
            PhaseOrder::Security => alva_kernel_core::middleware::MiddlewarePriority::SECURITY,
            PhaseOrder::Hooks => alva_kernel_core::middleware::MiddlewarePriority::HOOKS,
            PhaseOrder::Policy => alva_kernel_core::middleware::MiddlewarePriority::GUARDRAIL,
            PhaseOrder::Normal => alva_kernel_core::middleware::MiddlewarePriority::DEFAULT,
            PhaseOrder::Telemetry => alva_kernel_core::middleware::MiddlewarePriority::OBSERVATION,
            PhaseOrder::Finalizer => alva_kernel_core::middleware::MiddlewarePriority::RETRY,
        }
    }
}

/// Build-time descriptor for one plugin contribution to the runtime phase
/// timeline.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PhaseContribution {
    pub name: String,
    pub phase: Phase,
    pub effect: PhaseEffect,
    pub order: PhaseOrder,
}

impl PhaseContribution {
    pub fn new(
        name: impl Into<String>,
        phase: Phase,
        effect: PhaseEffect,
        order: PhaseOrder,
    ) -> Self {
        Self {
            name: name.into(),
            phase,
            effect,
            order,
        }
    }
}

/// Executable handler for one runtime phase contribution.
///
/// Semantic plugin helpers can implement this trait and register through
/// `Registrar::phase_handler`. While the kernel is still middleware-backed,
/// agent-core compiles handlers into a small middleware adapter. The public
/// registration target stays the stable phase timeline.
#[async_trait]
pub trait PhaseHandler: Send + Sync {
    fn contribution(&self) -> PhaseContribution;

    async fn run_start(&self, _state: &mut AgentState) -> Result<(), MiddlewareError> {
        Ok(())
    }

    async fn input_committed(
        &self,
        _state: &mut AgentState,
        _message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    async fn run_end(
        &self,
        _state: &mut AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    async fn before_llm_call(
        &self,
        _state: &mut AgentState,
        _messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    async fn after_llm_call(
        &self,
        _state: &mut AgentState,
        _response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    async fn before_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }

    fn handles_before_tool_call_as_wrap(&self) -> bool {
        false
    }

    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        next.call(state, tool_call)
            .await
            .map_err(MiddlewareError::from)
    }

    async fn after_tool_call(
        &self,
        _state: &mut AgentState,
        _tool_call: &ToolCall,
        _result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        Ok(())
    }
}

pub(crate) struct PhaseHandlerMiddleware {
    contribution: PhaseContribution,
    handler: Arc<dyn PhaseHandler>,
    name: String,
}

impl PhaseHandlerMiddleware {
    pub(crate) fn new(handler: Arc<dyn PhaseHandler>, contribution: PhaseContribution) -> Self {
        let name = format!("phase:{}", contribution.name);
        Self {
            contribution,
            handler,
            name,
        }
    }
}

#[async_trait]
impl Middleware for PhaseHandlerMiddleware {
    fn name(&self) -> &str {
        &self.name
    }

    fn priority(&self) -> i32 {
        self.contribution.order.priority()
    }

    async fn on_agent_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        match self.contribution.phase {
            Phase::RunStart => self.handler.run_start(state).await,
            _ => Ok(()),
        }
    }

    async fn input_committed(
        &self,
        state: &mut AgentState,
        message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::InputCommitted {
            self.handler.input_committed(state, message).await?;
        }
        Ok(())
    }

    async fn on_agent_end(
        &self,
        state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::RunEnd {
            self.handler.run_end(state, error).await?;
        }
        Ok(())
    }

    async fn before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::BeforeLlmCall {
            self.handler.before_llm_call(state, messages).await?;
        }
        Ok(())
    }

    async fn after_llm_call(
        &self,
        state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::AfterLlmCall {
            self.handler.after_llm_call(state, response).await?;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::BeforeToolCall
            && !self.handler.handles_before_tool_call_as_wrap()
        {
            self.handler.before_tool_call(state, tool_call).await?;
        }
        Ok(())
    }

    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        if self.contribution.phase == Phase::BeforeToolCall
            && self.handler.handles_before_tool_call_as_wrap()
        {
            return self.handler.wrap_tool_call(state, tool_call, next).await;
        }
        next.call(state, tool_call)
            .await
            .map_err(MiddlewareError::from)
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        if self.contribution.phase == Phase::AfterToolCall {
            self.handler
                .after_tool_call(state, tool_call, result)
                .await?;
        }
        Ok(())
    }
}
