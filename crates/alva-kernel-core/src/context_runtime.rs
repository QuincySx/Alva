// INPUT:  alva_kernel_abi context hooks/types, AgentConfig, context_apply
// OUTPUT: ContextRuntime
// POS:    Coordinates ingest, pending injection, assembly, and budget compression before provider calls.
use alva_kernel_abi::scope::context::{
    ContextEntry, ContextLayer, ContextMetadata, IngestAction, Injection,
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
            let id = match message {
                AgentMessage::Standard(msg) => msg.id.clone(),
                _ => uuid::Uuid::new_v4().to_string(),
            };
            let entry = ContextEntry {
                id,
                message: message.clone(),
                metadata: ContextMetadata::new(ContextLayer::RuntimeInject).with_tokens(
                    estimate_message_tokens(std::slice::from_ref(message), config.bus.as_ref()),
                ),
            };
            let message = match cs.hooks().ingest(cs.handle(), &self.agent_id, &entry).await {
                IngestAction::Keep => message,
                IngestAction::Skip => return,
                IngestAction::Modify { message, .. } => {
                    self.pending_injections.extend(
                        cs.hooks()
                            .on_message(cs.handle(), &self.agent_id, &message)
                            .await,
                    );
                    return;
                }
            };
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
            match cs_apply(config) {
                Some(apply) => apply.apply_injections(
                    std::mem::take(&mut self.pending_injections),
                    system_prompt_buf,
                    &mut working_messages,
                ),
                None => {
                    // 上下文系统未挂执行器：内核不执行注入（ticket 10）。
                    // 有自定义执行语义的第三方系统应把执行做在自己的钩子里。
                    self.pending_injections.clear();
                    tracing::debug!(
                        "context system has no apply executor; dropping pending injections"
                    );
                }
            }
        }

        if let Some(cs) = config.context_system.as_ref() {
            let assemble_budget = config
                .context_token_budget
                .or_else(|| {
                    let budget = cs.handle().budget(&self.agent_id).budget_tokens;
                    (budget > 0).then_some(budget)
                })
                .or_else(|| {
                    config
                        .bus
                        .as_ref()
                        .and_then(|bus| bus.get::<dyn alva_kernel_abi::TokenCounter>())
                        .map(|counter| counter.context_window())
                })
                .unwrap_or(usize::MAX);
            let entries: Vec<ContextEntry> = working_messages
                .into_iter()
                .map(|m| {
                    let id = match &m {
                        AgentMessage::Standard(msg) => msg.id.clone(),
                        _ => uuid::Uuid::new_v4().to_string(),
                    };
                    let tokens =
                        estimate_message_tokens(std::slice::from_ref(&m), config.bus.as_ref());
                    ContextEntry {
                        id,
                        message: m,
                        metadata: ContextMetadata::new(ContextLayer::RuntimeInject)
                            .with_tokens(tokens),
                    }
                })
                .collect();
            let assembled = cs
                .hooks()
                .assemble(cs.handle(), &self.agent_id, entries, assemble_budget)
                .await;
            working_messages = assembled.into_iter().map(|e| e.message).collect();
        }

        if let (Some(cs), Some(budget)) =
            (config.context_system.as_ref(), config.context_token_budget)
        {
            let total_tokens = estimate_message_tokens(&working_messages, config.bus.as_ref())
                + estimate_prompt_tokens(system_prompt_buf, config.bus.as_ref());
            if total_tokens > budget {
                let mut snapshot = cs.handle().snapshot(&self.agent_id);
                snapshot.total_tokens = total_tokens;
                snapshot.budget_tokens = budget;
                snapshot.model_window = config
                    .bus
                    .as_ref()
                    .and_then(|bus| bus.get::<dyn alva_kernel_abi::TokenCounter>())
                    .map(|counter| counter.context_window())
                    .unwrap_or(snapshot.model_window.max(budget));
                snapshot.usage_ratio = if budget == 0 {
                    1.0
                } else {
                    total_tokens as f32 / budget as f32
                };
                let actions = cs
                    .hooks()
                    .on_budget_exceeded(cs.handle(), &self.agent_id, &snapshot)
                    .await;
                if let Some(apply) = cs_apply(config) {
                    apply
                        .apply_compressions(
                            actions,
                            &mut working_messages,
                            cs.handle(),
                            &self.agent_id,
                        )
                        .await;
                }
            }
        }

        working_messages
    }
}

fn cs_apply(config: &AgentConfig) -> Option<&dyn alva_kernel_abi::scope::context::ContextApply> {
    config.context_system.as_ref().and_then(|cs| cs.apply())
}

fn estimate_prompt_tokens(prompt: &[String], bus: Option<&BusHandle>) -> usize {
    let counter = bus.and_then(|b| b.get::<dyn alva_kernel_abi::TokenCounter>());
    prompt
        .iter()
        .map(|segment| match &counter {
            Some(counter) => counter.count_tokens(segment),
            None => segment.chars().count().div_ceil(4),
        })
        .sum()
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
                None => text.chars().count().div_ceil(4),
            };
            tokens + 4
        })
        .sum()
}
