// INPUT:  alva_types::{AgentMessage, Message, MessageRole, ContentBlock}, crate::store::estimate_tokens
// OUTPUT: CompactionResult, CompactionConfig, should_compact(), compact_messages()
// POS:    Message compaction service matching Claude Code's compact/ — triggers on token/count thresholds,
//         replaces older messages with a summary while preserving recent context.
//! Context compaction service — summarize older messages to reclaim token budget.
//!
//! Matches Claude Code's compact/ service behavior:
//! - Trigger when token usage exceeds threshold percentage or message count is too high
//! - Preserve recent messages (never compact the tail)
//! - Replace older messages with a single summary message
//! - Optionally preserve thinking/reasoning blocks

use alva_types::{AgentMessage, ContentBlock, Message, MessageRole};

use crate::store::estimate_tokens;

/// Result of a compaction operation.
#[derive(Debug, Clone)]
pub struct CompactionResult {
    /// Compacted messages replacing the original set.
    pub messages: Vec<AgentMessage>,
    /// Number of messages removed by compaction.
    pub messages_removed: usize,
    /// Estimated tokens saved.
    pub tokens_saved: usize,
    /// Summary text of what was compacted.
    pub summary: String,
}

/// Compaction configuration.
#[derive(Debug, Clone)]
pub struct CompactionConfig {
    /// Maximum token count before triggering compaction.
    pub max_tokens: usize,
    /// Percentage of max tokens that triggers compaction (0.0-1.0).
    pub trigger_threshold: f64,
    /// Number of recent messages to preserve (never compact).
    pub preserve_recent: usize,
    /// Whether to preserve thinking/reasoning blocks during compaction.
    pub preserve_thinking: bool,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            max_tokens: 200_000,
            trigger_threshold: alva_types::constants::AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT,
            preserve_recent: 10,
            preserve_thinking: true,
        }
    }
}

/// Check if compaction should be triggered based on current state.
///
/// Returns `true` if either:
/// - Current token count exceeds `max_tokens * trigger_threshold`
/// - Message count exceeds `AUTO_COMPACT_MESSAGE_THRESHOLD` (200)
pub fn should_compact(
    messages: &[AgentMessage],
    config: &CompactionConfig,
    current_tokens: usize,
) -> bool {
    let threshold = (config.max_tokens as f64 * config.trigger_threshold) as usize;
    current_tokens > threshold
        || messages.len() > alva_types::constants::AUTO_COMPACT_MESSAGE_THRESHOLD
}

/// Compact messages by replacing older messages with a summary.
///
/// The `summary_text` is expected to be pre-generated (e.g., by an LLM summarizer
/// or by `ContextHandle::summarize()`). This function handles the structural work
/// of splitting old/recent messages and inserting the summary.
///
/// Returns a `CompactionResult` with the new message set and statistics.
pub fn compact_messages(
    messages: &[AgentMessage],
    config: &CompactionConfig,
    summary_text: &str,
) -> CompactionResult {
    if messages.len() <= config.preserve_recent {
        return CompactionResult {
            messages: messages.to_vec(),
            messages_removed: 0,
            tokens_saved: 0,
            summary: String::new(),
        };
    }

    let split_point = messages.len() - config.preserve_recent;
    let old_messages = &messages[..split_point];
    let recent_messages = &messages[split_point..];

    // Estimate tokens in old messages for savings calculation
    let old_tokens: usize = old_messages
        .iter()
        .map(|m| estimate_agent_message_tokens(m))
        .sum();

    // Optionally extract and preserve thinking blocks from old messages
    let mut preserved_thinking = Vec::new();
    if config.preserve_thinking {
        for msg in old_messages {
            if let AgentMessage::Standard(m) = msg {
                for block in &m.content {
                    if let ContentBlock::Reasoning { text } = block {
                        preserved_thinking.push(text.clone());
                    }
                }
            }
        }
    }

    // Build the summary content
    let mut summary_content = format!(
        "<context-compacted>\nThe following is a summary of the conversation so far:\n{}",
        summary_text
    );

    if !preserved_thinking.is_empty() {
        summary_content.push_str("\n\nPreserved reasoning:\n");
        for thought in &preserved_thinking {
            let truncated = if thought.len() > 500 {
                format!("{}...", &thought[..500])
            } else {
                thought.clone()
            };
            summary_content.push_str(&format!("- {}\n", truncated));
        }
    }

    summary_content.push_str("\n</context-compacted>");

    // Create the summary message
    let summary_tokens = estimate_tokens(&summary_content);
    let summary_message = AgentMessage::Standard(Message {
        id: uuid::Uuid::new_v4().to_string(),
        role: MessageRole::System,
        content: vec![ContentBlock::Text {
            text: summary_content,
        }],
        tool_call_id: None,
        usage: None,
        timestamp: chrono::Utc::now().timestamp_millis(),
    });

    let mut compacted = vec![summary_message];
    compacted.extend_from_slice(recent_messages);

    let tokens_saved = old_tokens.saturating_sub(summary_tokens);

    CompactionResult {
        messages: compacted,
        messages_removed: old_messages.len(),
        tokens_saved,
        summary: summary_text.to_string(),
    }
}

/// Estimate token count for an AgentMessage.
fn estimate_agent_message_tokens(msg: &AgentMessage) -> usize {
    match msg {
        AgentMessage::Standard(m) => estimate_tokens(&m.text_content()),
        AgentMessage::Extension { data, .. } => estimate_tokens(&data.to_string()),
        _ => 0,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn user_msg(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 1000,
        })
    }

    fn assistant_msg_with_reasoning(text: &str, reasoning: &str) -> AgentMessage {
        AgentMessage::Standard(Message {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Reasoning {
                    text: reasoning.to_string(),
                },
                ContentBlock::Text {
                    text: text.to_string(),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 2000,
        })
    }

    #[test]
    fn should_compact_by_tokens() {
        let config = CompactionConfig {
            max_tokens: 1000,
            trigger_threshold: 0.8,
            ..Default::default()
        };
        let msgs = vec![user_msg("hi")];

        // 900 > 800 (1000 * 0.8)
        assert!(should_compact(&msgs, &config, 900));
        // 700 < 800
        assert!(!should_compact(&msgs, &config, 700));
    }

    #[test]
    fn should_compact_by_message_count() {
        let config = CompactionConfig::default();
        let msgs: Vec<AgentMessage> = (0..201).map(|i| user_msg(&format!("msg {}", i))).collect();
        assert!(should_compact(&msgs, &config, 0));
    }

    #[test]
    fn compact_preserves_recent() {
        let config = CompactionConfig {
            preserve_recent: 3,
            preserve_thinking: false,
            ..Default::default()
        };
        let msgs: Vec<AgentMessage> = (0..10)
            .map(|i| user_msg(&format!("message {}", i)))
            .collect();

        let result = compact_messages(&msgs, &config, "Summary of old messages");

        // 1 summary + 3 preserved = 4
        assert_eq!(result.messages.len(), 4);
        assert_eq!(result.messages_removed, 7);

        // First message should be the summary
        if let AgentMessage::Standard(m) = &result.messages[0] {
            assert_eq!(m.role, MessageRole::System);
            assert!(m.text_content().contains("context-compacted"));
            assert!(m.text_content().contains("Summary of old messages"));
        } else {
            panic!("Expected Standard message for summary");
        }

        // Last 3 should be the original recent messages
        for (i, msg) in result.messages[1..].iter().enumerate() {
            if let AgentMessage::Standard(m) = msg {
                assert_eq!(m.text_content(), format!("message {}", i + 7));
            }
        }
    }

    #[test]
    fn compact_noop_when_few_messages() {
        let config = CompactionConfig {
            preserve_recent: 5,
            ..Default::default()
        };
        let msgs = vec![user_msg("a"), user_msg("b"), user_msg("c")];

        let result = compact_messages(&msgs, &config, "unused summary");

        assert_eq!(result.messages.len(), 3);
        assert_eq!(result.messages_removed, 0);
        assert!(result.summary.is_empty());
    }

    #[test]
    fn compact_preserves_thinking_blocks() {
        let config = CompactionConfig {
            preserve_recent: 1,
            preserve_thinking: true,
            ..Default::default()
        };
        let msgs = vec![
            assistant_msg_with_reasoning("answer 1", "I need to think about this"),
            assistant_msg_with_reasoning("answer 2", "Let me reason carefully"),
            user_msg("final question"),
        ];

        let result = compact_messages(&msgs, &config, "Summary text");

        // Summary message should contain preserved reasoning
        if let AgentMessage::Standard(m) = &result.messages[0] {
            let content = m.text_content();
            assert!(content.contains("Preserved reasoning"));
            assert!(content.contains("I need to think about this"));
            assert!(content.contains("Let me reason carefully"));
        }
    }

    #[test]
    fn compact_tokens_saved_is_positive() {
        let config = CompactionConfig {
            preserve_recent: 2,
            preserve_thinking: false,
            ..Default::default()
        };
        // Create messages with substantial text
        let long_text = "a".repeat(4000); // ~1000 tokens
        let msgs = vec![
            user_msg(&long_text),
            user_msg(&long_text),
            user_msg(&long_text),
            user_msg("recent 1"),
            user_msg("recent 2"),
        ];

        let result = compact_messages(&msgs, &config, "brief summary");
        assert!(result.tokens_saved > 0);
    }
}
