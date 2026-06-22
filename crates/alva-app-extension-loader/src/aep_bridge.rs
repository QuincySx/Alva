// INPUT:  proxy::{RemotePluginProxy, AepEvent, AepDispatchResult},
//         alva_agent_core::extension::PhaseHandler
// OUTPUT: AepPhaseHandler
// POS:    Executable agent-core phase handler for one remote AEP subscription.

//! AEP phase handlers.
//!
//! The loader maps remote `eventSubscriptions` to agent-core
//! `PhaseContribution`s. Executable subscriptions are represented by this
//! `PhaseHandler` implementation; agent-core compiles it into the current
//! middleware stack while kernel-native phase execution is still pending.

use std::sync::Arc;

use alva_agent_core::extension::{PhaseContribution, PhaseHandler};
use alva_kernel_abi::{AgentMessage, Message, MessageRole, ToolCall, ToolOutput};
use alva_kernel_core::middleware::{MiddlewareError, ToolCallFn};
use alva_kernel_core::state::AgentState;
use async_trait::async_trait;

use crate::proxy::{AepDispatchResult, AepEvent, RemotePluginProxy};

/// Executable phase handler for exactly one remote plugin event subscription.
pub struct AepPhaseHandler {
    plugin: Arc<RemotePluginProxy>,
    subscription: String,
    contribution: PhaseContribution,
}

impl AepPhaseHandler {
    pub fn new(
        plugin: Arc<RemotePluginProxy>,
        subscription: impl Into<String>,
        contribution: PhaseContribution,
    ) -> Self {
        Self {
            plugin,
            subscription: subscription.into(),
            contribution,
        }
    }

    fn dispatch(&self, event: &AepEvent<'_>) -> Result<AepDispatchResult, MiddlewareError> {
        let result = self.plugin.dispatch_event_sync(event);
        self.plugin.clear_state_snapshot();
        if let AepDispatchResult::Block { reason } = result {
            return Err(MiddlewareError::Blocked { reason });
        }
        Ok(result)
    }

    async fn refresh_state_snapshot(&self, state: &AgentState) {
        let messages = state
            .session
            .messages()
            .await
            .into_iter()
            .filter_map(agent_message_to_message)
            .collect();
        self.plugin.update_state_snapshot(
            messages,
            serde_json::json!({
                "session_id": state.session.session_id(),
            }),
        );
    }
}

#[async_trait]
impl PhaseHandler for AepPhaseHandler {
    fn contribution(&self) -> PhaseContribution {
        self.contribution.clone()
    }

    async fn run_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        if self.subscription == "on_agent_start" {
            self.refresh_state_snapshot(state).await;
            self.dispatch(&AepEvent::AgentStart)?;
        }
        Ok(())
    }

    async fn input_committed(
        &self,
        state: &mut AgentState,
        message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "on_user_message" {
            if let Some(text) = user_message_text(message) {
                self.refresh_state_snapshot(state).await;
                self.dispatch(&AepEvent::UserMessage { text: &text })?;
            }
        }
        Ok(())
    }

    async fn run_end(
        &self,
        state: &mut AgentState,
        error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "on_agent_end" {
            self.refresh_state_snapshot(state).await;
            self.dispatch(&AepEvent::AgentEnd { error })?;
        }
        Ok(())
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "before_tool_call" {
            self.refresh_state_snapshot(state).await;
            self.dispatch(&AepEvent::BeforeToolCall {
                tool_name: &tool_call.name,
                tool_call_id: &tool_call.id,
                arguments: &tool_call.arguments,
            })?;
        }
        Ok(())
    }

    fn handles_before_tool_call_as_wrap(&self) -> bool {
        self.subscription == "before_tool_call"
    }

    async fn wrap_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        if self.subscription != "before_tool_call" {
            return next
                .call(state, tool_call)
                .await
                .map_err(MiddlewareError::from);
        }

        self.refresh_state_snapshot(state).await;
        let decision = self.plugin.dispatch_event_sync(&AepEvent::BeforeToolCall {
            tool_name: &tool_call.name,
            tool_call_id: &tool_call.id,
            arguments: &tool_call.arguments,
        });
        self.plugin.clear_state_snapshot();
        match decision {
            AepDispatchResult::Continue => next
                .call(state, tool_call)
                .await
                .map_err(MiddlewareError::from),
            AepDispatchResult::ModifyToolArguments { arguments } => {
                let mut modified = tool_call.clone();
                modified.arguments = arguments;
                next.call(state, &modified)
                    .await
                    .map_err(MiddlewareError::from)
            }
            AepDispatchResult::ReplaceResult { result } => Ok(result),
            AepDispatchResult::Block { reason } => {
                Ok(ToolOutput::error(format!("Tool call blocked: {reason}")))
            }
            _ => next
                .call(state, tool_call)
                .await
                .map_err(MiddlewareError::from),
        }
    }

    async fn before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "on_llm_call_start" {
            self.refresh_state_snapshot(state).await;
            if let AepDispatchResult::ModifyMessages { messages: next } =
                self.dispatch(&AepEvent::LlmCallStart { messages })?
            {
                *messages = next;
            }
        }
        Ok(())
    }

    async fn after_llm_call(
        &self,
        state: &mut AgentState,
        response: &mut Message,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "on_llm_call_end" {
            self.refresh_state_snapshot(state).await;
            if let AepDispatchResult::ModifyResponse { response: next } =
                self.dispatch(&AepEvent::LlmCallEnd { response })?
            {
                *response = next;
            }
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        if self.subscription == "after_tool_call" {
            self.refresh_state_snapshot(state).await;
            if let AepDispatchResult::ModifyResult { result: next } =
                self.dispatch(&AepEvent::AfterToolCall {
                    tool_name: &tool_call.name,
                    tool_call_id: &tool_call.id,
                    result,
                })?
            {
                *result = next;
            }
        }
        Ok(())
    }
}

fn user_message_text(message: &AgentMessage) -> Option<String> {
    match message {
        AgentMessage::Standard(message)
        | AgentMessage::Steering(message)
        | AgentMessage::FollowUp(message)
            if message.role == MessageRole::User =>
        {
            Some(message.text_content())
        }
        _ => None,
    }
}

fn agent_message_to_message(message: AgentMessage) -> Option<Message> {
    match message {
        AgentMessage::Standard(message)
        | AgentMessage::Steering(message)
        | AgentMessage::FollowUp(message) => Some(message),
        AgentMessage::Marker(_) | AgentMessage::Extension { .. } => None,
    }
}
