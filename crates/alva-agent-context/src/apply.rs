// INPUT:  alva_types::scope::context::*, alva_types::AgentMessage, alva_types::Message
// OUTPUT: pub fn apply_injections, pub async fn apply_compressions
// POS:    Runtime helpers for applying context hook results to agent state.
//         Moved from alva_types::context::apply — types crate should not contain runtime logic.

use alva_types::base::content::ContentBlock;
use alva_types::base::message::{Message, MessageRole};
use alva_types::scope::context::{
    CompressAction, ContextHandle, Injection, InjectionContent, MessageSelector,
};
use alva_types::AgentMessage;
use tracing::debug;

/// Apply a list of Injections to system_prompt and messages.
pub fn apply_injections(
    injections: Vec<Injection>,
    system_prompt: &mut String,
    messages: &mut Vec<AgentMessage>,
) {
    for injection in injections {
        match injection.content {
            InjectionContent::Memory(facts) => {
                if !facts.is_empty() {
                    let text = facts
                        .iter()
                        .map(|f| format!("- {}", f.text))
                        .collect::<Vec<_>>()
                        .join("\n");
                    system_prompt
                        .push_str(&format!("\n\n<user_memory>\n{}\n</user_memory>", text));
                }
            }
            InjectionContent::Skill { name, content } => {
                system_prompt.push_str(&format!(
                    "\n\n<skill name=\"{}\">\n{}\n</skill>",
                    name, content
                ));
            }
            InjectionContent::RuntimeContext(data) => {
                system_prompt.push_str(&format!("\n\n<runtime>\n{}\n</runtime>", data));
            }
            InjectionContent::Message(msg) => {
                messages.push(msg);
            }
            InjectionContent::SystemPrompt(section) => {
                system_prompt.push_str("\n\n");
                system_prompt.push_str(&section.content);
            }
        }
    }
}

/// Apply a list of CompressActions to messages.
///
/// Handles SlidingWindow, ReplaceToolResult, Summarize, RemoveByPriority, Externalize.
/// Summarize calls `handle.summarize()` with a 5-second timeout.
pub async fn apply_compressions(
    actions: Vec<CompressAction>,
    messages: &mut Vec<AgentMessage>,
    handle: &dyn ContextHandle,
    session_id: &str,
) {
    for action in actions {
        match action {
            CompressAction::SlidingWindow { keep_recent } => {
                if messages.len() > keep_recent {
                    let drop_count = messages.len() - keep_recent;
                    messages.drain(..drop_count);
                    debug!(drop_count, keep_recent, "budget: applied sliding window");
                }
            }
            CompressAction::RemoveByPriority { .. } => {
                // Will activate once ContextStore holds real entries.
            }
            CompressAction::ReplaceToolResult {
                message_id,
                summary,
            } => {
                for msg in messages.iter_mut() {
                    if let AgentMessage::Standard(m) = msg {
                        if m.id == message_id {
                            m.content = vec![ContentBlock::Text {
                                text: summary.clone(),
                            }];
                            break;
                        }
                    }
                }
            }
            CompressAction::Summarize { range, hints } => {
                let msg_len = messages.len();
                let from_idx = resolve_selector(&range.from, messages, 0);
                let to_idx = resolve_selector(&range.to, messages, msg_len);

                if from_idx < to_idx && to_idx <= msg_len {
                    let range_text = serialize_range(&messages[from_idx..to_idx], from_idx);

                    let summary_result = tokio::time::timeout(
                        std::time::Duration::from_secs(5),
                        handle.summarize(session_id, range.clone(), &hints),
                    )
                    .await;

                    let summary_text = match summary_result {
                        Ok(s) => s,
                        Err(_) => {
                            tracing::warn!(
                                "budget: summarize timed out, falling back to truncation"
                            );
                            let truncated: String = range_text.chars().take(2000).collect();
                            format!(
                                "{}\n\n[... summarization timed out, truncated]",
                                truncated
                            )
                        }
                    };

                    let summary_msg = AgentMessage::Standard(Message {
                        id: uuid::Uuid::new_v4().to_string(),
                        role: MessageRole::User,
                        content: vec![ContentBlock::Text {
                            text: format!(
                                "<conversation_summary>\n{}\n</conversation_summary>",
                                summary_text
                            ),
                        }],
                        tool_call_id: None,
                        usage: None,
                        timestamp: chrono::Utc::now().timestamp_millis(),
                    });

                    messages.drain(from_idx..to_idx);
                    messages.insert(from_idx, summary_msg);
                    debug!(
                        from = from_idx,
                        to = to_idx,
                        "budget: summarized {} messages",
                        to_idx - from_idx
                    );
                }
            }
            CompressAction::Externalize { .. } => {
                debug!("budget: externalize action not yet implemented");
            }
        }
    }
}

fn resolve_selector(
    selector: &MessageSelector,
    messages: &[AgentMessage],
    default: usize,
) -> usize {
    let len = messages.len();
    match selector {
        MessageSelector::FromStart => 0,
        MessageSelector::ToEnd => len,
        MessageSelector::ByIndex(i) => (*i).min(len),
        MessageSelector::ById(id) => messages
            .iter()
            .position(|m| match m {
                AgentMessage::Standard(msg) => msg.id == *id,
                _ => false,
            })
            .unwrap_or(default),
    }
}

fn serialize_range(messages: &[AgentMessage], base_idx: usize) -> String {
    messages
        .iter()
        .enumerate()
        .map(|(i, m)| {
            let role = match m {
                AgentMessage::Standard(msg) => format!("{:?}", msg.role),
                AgentMessage::Custom { .. } => "Custom".to_string(),
            };
            let text = match m {
                AgentMessage::Standard(msg) => msg.text_content(),
                AgentMessage::Custom { data, .. } => data.to_string(),
            };
            let truncated = if text.len() > 2000 {
                format!("{}...[truncated]", &text[..2000])
            } else {
                text
            };
            format!("[{}] {}: {}", base_idx + i, role, truncated)
        })
        .collect::<Vec<_>>()
        .join("\n")
}
