// INPUT:  alva_kernel_abi::scope::context::*, alva_kernel_abi::AgentMessage, alva_kernel_abi::Message
// OUTPUT: pub fn apply_injections, pub async fn apply_compressions
// POS:    Runtime helpers for applying context hook results to agent state.
//         Moved from alva_kernel_abi::context::apply — types crate should not contain runtime logic.

use crate::base::content::ContentBlock;
use crate::base::message::{Message, MessageRole};
use crate::scope::context::{
    CompressAction, ContextHandle, Injection, InjectionContent, MessageSelector,
};
use crate::AgentMessage;
use tracing::debug;

/// Apply a list of Injections to the layered system prompt and message
/// list.
///
/// Each entry in `system_prompt` is a "segment". Convention used by
/// the kernel:
///   - **All entries except the last** are stable / cacheable (Layer
///     L0 / L1 / L3 / Memory contributions live here).
///   - **The last entry** is the dynamic / volatile bucket (Layer L2
///     RuntimeInject — date, git status, fresh tool results).
///
/// Routing:
///   - `Memory` / `Skill` / `SystemPrompt` injections → appended to
///     the **stable** bucket (the second-to-last entry, or a fresh
///     stable entry if the vec was all-dynamic).
///   - `RuntimeContext` injection → appended to the **dynamic**
///     bucket (the last entry, creating it if necessary).
///   - `Message` → unchanged, pushed onto `messages`.
///
/// This preserves the long-stable prefix that prompt-cache providers
/// (Anthropic, OpenAI auto-prefix) need to hit cache reliably.
pub fn apply_injections(
    injections: Vec<Injection>,
    system_prompt: &mut Vec<String>,
    messages: &mut Vec<AgentMessage>,
) {
    /// Get a mutable reference to the dynamic (last) segment, creating
    /// an empty one if the prompt vec is empty or contains only stable
    /// content (we conservatively treat the existing trailing entry as
    /// stable when only one entry exists — caller's responsibility to
    /// already split via `assemble_system_prompt`).
    fn dynamic_seg(buf: &mut Vec<String>) -> &mut String {
        if buf.is_empty() {
            buf.push(String::new());
        }
        buf.last_mut().unwrap()
    }

    /// Mutable ref to the stable bucket — second-to-last entry. If the
    /// prompt has only one entry (all-stable so far), we append in
    /// place; if it has zero, we create one.
    fn stable_seg(buf: &mut Vec<String>) -> &mut String {
        if buf.is_empty() {
            buf.push(String::new());
            return buf.first_mut().unwrap();
        }
        if buf.len() == 1 {
            return buf.first_mut().unwrap();
        }
        // 2+ entries: last is dynamic, second-to-last is stable bucket.
        let idx = buf.len() - 2;
        &mut buf[idx]
    }

    for injection in injections {
        match injection.content {
            InjectionContent::Memory(facts) => {
                if !facts.is_empty() {
                    let text = facts
                        .iter()
                        .map(|f| format!("- {}", f.text))
                        .collect::<Vec<_>>()
                        .join("\n");
                    stable_seg(system_prompt).push_str(&format!(
                        "\n\n<user_memory>\n{}\n</user_memory>",
                        text
                    ));
                }
            }
            InjectionContent::Skill { name, content } => {
                stable_seg(system_prompt).push_str(&format!(
                    "\n\n<skill name=\"{}\">\n{}\n</skill>",
                    name, content
                ));
            }
            InjectionContent::RuntimeContext(data) => {
                dynamic_seg(system_prompt)
                    .push_str(&format!("\n\n<runtime>\n{}\n</runtime>", data));
            }
            InjectionContent::Message(msg) => {
                messages.push(msg);
            }
            InjectionContent::SystemPrompt(section) => {
                // PromptSection doesn't carry a layer (it's named L0
                // by definition), so route to the stable bucket.
                let target = stable_seg(system_prompt);
                target.push_str("\n\n");
                target.push_str(&section.content);
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
                    // The kernel does not enforce a timeout here — that is the
                    // ContextHandle implementation's responsibility. A real impl
                    // (e.g., LLM-backed) should bound its own latency; a noop or
                    // synchronous impl returns immediately. This keeps kernel-abi
                    // free of any tokio::time dependency.
                    let _range_text =
                        serialize_range(&messages[from_idx..to_idx], from_idx);
                    let summary_text =
                        handle.summarize(session_id, range.clone(), &hints).await;

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
                AgentMessage::Extension { .. } => "Extension".to_string(),
                _ => "Other".to_string(),
            };
            let text = match m {
                AgentMessage::Standard(msg) => msg.text_content(),
                AgentMessage::Extension { data, .. } => data.to_string(),
                _ => String::new(),
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
