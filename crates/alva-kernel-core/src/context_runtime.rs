use alva_kernel_abi::scope::context::{
    apply_compressions, apply_injections, ContextEntry, ContextLayer, ContextMetadata,
    ContextSnapshot, Injection,
};
use alva_kernel_abi::{AgentMessage, BusHandle};

use crate::state::AgentConfig;

pub struct ContextRuntime {
    agent_id: String,
    pending_injections: Vec<Injection>,
}

impl ContextRuntime {
    pub fn new(agent_id: impl Into<String>) -> Self {
        Self {
            agent_id: agent_id.into(),
            pending_injections: Vec::new(),
        }
    }

    pub async fn bootstrap(&self, config: &AgentConfig) {
        if let Some(cs) = config.context_system.as_ref() {
            if let Err(e) = cs.hooks().bootstrap(cs.handle(), &self.agent_id).await {
                tracing::warn!(error = ?e, "context bootstrap failed");
            }
        }
    }

    pub async fn on_message(&mut self, config: &AgentConfig, message: &AgentMessage) {
        if let Some(cs) = config.context_system.as_ref() {
            self.pending_injections.extend(
                cs.hooks()
                    .on_message(cs.handle(), &self.agent_id, message)
                    .await,
            );
        }
    }

    pub async fn after_turn(&self, config: &AgentConfig) {
        if let Some(cs) = config.context_system.as_ref() {
            cs.hooks().after_turn(cs.handle(), &self.agent_id).await;
        }
    }

    pub async fn dispose(&self, config: &AgentConfig) {
        if let Some(cs) = config.context_system.as_ref() {
            if let Err(e) = cs.hooks().dispose().await {
                tracing::warn!(error = ?e, "context dispose failed");
            }
        }
    }

    pub async fn prepare_llm_context(
        &mut self,
        config: &AgentConfig,
        system_prompt_buf: &mut Vec<String>,
        session_messages: Vec<AgentMessage>,
    ) -> Vec<AgentMessage> {
        let mut working_messages = session_messages;

        if !self.pending_injections.is_empty() {
            apply_injections(
                std::mem::take(&mut self.pending_injections),
                system_prompt_buf,
                &mut working_messages,
            );
        }

        if let Some(cs) = config.context_system.as_ref() {
            let entries: Vec<ContextEntry> = working_messages
                .into_iter()
                .map(|m| {
                    let id = match &m {
                        AgentMessage::Standard(msg) => msg.id.clone(),
                        _ => uuid::Uuid::new_v4().to_string(),
                    };
                    ContextEntry {
                        id,
                        message: m,
                        metadata: ContextMetadata::new(ContextLayer::RuntimeInject),
                    }
                })
                .collect();
            let assembled = cs
                .hooks()
                .assemble(cs.handle(), &self.agent_id, entries, 0)
                .await;
            working_messages = assembled.into_iter().map(|e| e.message).collect();
        }

        if let (Some(cs), Some(budget)) =
            (config.context_system.as_ref(), config.context_token_budget)
        {
            let total_tokens = estimate_message_tokens(&working_messages, config.bus.as_ref());
            if total_tokens > budget {
                let snapshot = build_budget_snapshot(total_tokens, budget);
                let actions = cs
                    .hooks()
                    .on_budget_exceeded(cs.handle(), &self.agent_id, &snapshot)
                    .await;
                apply_compressions(actions, &mut working_messages, cs.handle(), &self.agent_id)
                    .await;
            }
        }

        working_messages
    }
}

/// Estimate total tokens for a working message list. Uses a bus-registered
/// `TokenCounter` if available, otherwise a 4-chars-per-token heuristic.
/// 4 tokens of overhead per message accounts for role / separator framing.
fn estimate_message_tokens(messages: &[AgentMessage], bus: Option<&BusHandle>) -> usize {
    let counter = bus.and_then(|b| b.get::<dyn alva_kernel_abi::TokenCounter>());
    messages
        .iter()
        .map(|m| {
            let text = match m {
                AgentMessage::Standard(msg) => msg.text_content(),
                AgentMessage::Steering(msg) => msg.text_content(),
                AgentMessage::FollowUp(msg) => msg.text_content(),
                AgentMessage::Marker(_) => String::new(),
                AgentMessage::Extension { data, .. } => data.to_string(),
            };
            let tokens = match &counter {
                Some(c) => c.count_tokens(&text),
                None => text.len() / 4,
            };
            tokens + 4
        })
        .sum()
}

fn build_budget_snapshot(total_tokens: usize, budget: usize) -> ContextSnapshot {
    ContextSnapshot {
        total_tokens,
        budget_tokens: budget,
        model_window: budget,
        usage_ratio: if budget == 0 {
            1.0
        } else {
            total_tokens as f32 / budget as f32
        },
        layer_breakdown: std::collections::HashMap::new(),
        entries: Vec::new(),
        recent_tool_patterns: Vec::new(),
    }
}
