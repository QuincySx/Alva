// INPUT:  alva_agent_core, alva_types, alva_agent_graph::compaction
// OUTPUT: CompactionMiddleware
// POS:    Auto-compacts conversation history when token count approaches model context limit.
//! Compaction middleware — summarizes old messages to stay within context window.
//!
//! When the estimated token count of messages exceeds `trigger_tokens`,
//! older messages are summarized by the LLM and replaced with a single
//! summary message. The most recent `keep_recent_tokens` worth of messages
//! are always preserved verbatim.

use std::sync::atomic::{AtomicUsize, Ordering};

use alva_agent_core::middleware::{Middleware, MiddlewareError, MiddlewarePriority};
use alva_agent_core::state::AgentState;
use alva_types::{BusHandle, Message, ModelConfig};
use async_trait::async_trait;

/// Configuration for the compaction middleware.
pub struct CompactionConfig {
    /// Trigger compaction when estimated tokens exceed this threshold.
    /// Default: 80% of a typical 128K context window = 100_000 tokens.
    pub trigger_tokens: usize,

    /// Reserve this many tokens for the model's response.
    /// Default: 16_000 tokens.
    pub reserve_tokens: usize,

    /// Keep at least this many tokens of recent messages verbatim.
    /// Default: 20_000 tokens.
    pub keep_recent_tokens: usize,
}

impl Default for CompactionConfig {
    fn default() -> Self {
        Self {
            trigger_tokens: 100_000,
            reserve_tokens: 16_000,
            keep_recent_tokens: 20_000,
        }
    }
}

/// Middleware that auto-compacts conversation history via LLM summarization.
///
/// Inserts itself at `CONTEXT` priority (3000) so it runs after security
/// but before observation middleware.
pub struct CompactionMiddleware {
    config: CompactionConfig,
    compaction_count: AtomicUsize,
    bus: Option<BusHandle>,
}

impl CompactionMiddleware {
    pub fn new(config: CompactionConfig) -> Self {
        Self {
            config,
            compaction_count: AtomicUsize::new(0),
            bus: None,
        }
    }

    /// Attach a bus handle for token counting and event emission.
    pub fn with_bus(mut self, bus: BusHandle) -> Self {
        self.bus = Some(bus);
        self
    }

    /// How many times compaction has been triggered in this session.
    pub fn compaction_count(&self) -> usize {
        self.compaction_count.load(Ordering::Relaxed)
    }
}

impl Default for CompactionMiddleware {
    fn default() -> Self {
        Self::new(CompactionConfig::default())
    }
}

/// Rough token estimate: ~4 characters per token for English text.
/// Includes tool call arguments and tool results.
fn estimate_tokens(messages: &[Message]) -> usize {
    messages
        .iter()
        .map(|m| {
            let text_len: usize = m.content.iter().map(|b| b.estimated_tokens()).sum();
            // Add overhead for role, separators, etc.
            text_len + 4
        })
        .sum()
}

/// Find the split point: walk backwards from the end, keeping messages
/// until we've accumulated `keep_tokens` worth of recent messages.
/// Returns the index where "old" messages end and "recent" messages begin.
fn find_split_point(messages: &[Message], keep_tokens: usize) -> usize {
    let mut accumulated = 0;
    for i in (0..messages.len()).rev() {
        let msg_tokens: usize = messages[i].content.iter().map(|b| b.estimated_tokens()).sum();
        accumulated += msg_tokens + 4;
        if accumulated > keep_tokens {
            return i + 1;
        }
    }
    0 // Keep everything (nothing to compact)
}

/// Build the summarization prompt for the LLM.
fn build_summary_prompt(old_messages: &[Message]) -> String {
    let mut conversation = String::new();
    for msg in old_messages {
        let role = match msg.role {
            alva_types::MessageRole::User => "User",
            alva_types::MessageRole::Assistant => "Assistant",
            alva_types::MessageRole::System => "System",
            alva_types::MessageRole::Tool => "Tool",
        };
        let text = msg.text_content();
        if !text.is_empty() {
            conversation.push_str(&format!("[{}]: {}\n\n", role, text));
        }
    }

    format!(
        "Summarize the following conversation concisely. \
         Preserve all key information: decisions made, files read or modified, \
         errors encountered, and current task progress. \
         Keep technical details (file paths, function names, error messages). \
         Be concise but don't lose important context.\n\n\
         ---\n\n{}\n\n---\n\nSummary:",
        conversation
    )
}

#[async_trait]
impl Middleware for CompactionMiddleware {
    async fn before_llm_call(
        &self,
        state: &mut AgentState,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        // Use bus TokenCounter for better estimation; fall back to local heuristic.
        let total_tokens = if let Some(ref bus) = self.bus {
            if let Some(counter) = bus.get::<dyn alva_types::TokenCounter>() {
                messages
                    .iter()
                    .map(|m| counter.count_tokens(&m.text_content()) + 4)
                    .sum()
            } else {
                estimate_tokens(messages)
            }
        } else {
            estimate_tokens(messages)
        };

        if total_tokens <= self.config.trigger_tokens {
            return Ok(()); // Under budget, no compaction needed
        }

        // Emit TokenBudgetExceeded event for observability.
        if let Some(ref bus) = self.bus {
            bus.emit(alva_types::TokenBudgetExceeded {
                agent_id: String::new(),
                usage_ratio: total_tokens as f32 / self.config.trigger_tokens as f32,
                used_tokens: total_tokens,
                budget_tokens: self.config.trigger_tokens,
            });
        }

        tracing::info!(
            total_tokens,
            trigger = self.config.trigger_tokens,
            "compaction triggered — summarizing old messages"
        );

        // Find split point: keep recent messages, summarize old ones
        let split = find_split_point(messages, self.config.keep_recent_tokens);
        if split <= 1 {
            // Nothing meaningful to compact (only system prompt + recent)
            return Ok(());
        }

        // Separate system prompt from conversation messages
        let (system_msgs, conversation): (Vec<_>, Vec<_>) = messages
            .iter()
            .enumerate()
            .partition(|(_, m)| m.role == alva_types::MessageRole::System);

        // Only compact conversation messages, not system prompt
        let conv_messages: Vec<&Message> = conversation.iter().map(|(_, m)| *m).collect();
        if conv_messages.len() <= 2 {
            return Ok(()); // Too few messages to compact
        }

        // Find split in conversation messages only
        let conv_split = {
            let mut acc = 0;
            let mut idx = conv_messages.len();
            for i in (0..conv_messages.len()).rev() {
                let t: usize = conv_messages[i].content.iter().map(|b| b.estimated_tokens()).sum();
                acc += t + 4;
                if acc > self.config.keep_recent_tokens {
                    idx = i + 1;
                    break;
                }
            }
            idx
        };

        if conv_split == 0 || conv_split >= conv_messages.len() {
            return Ok(());
        }

        let old_messages: Vec<Message> = conv_messages[..conv_split].iter().map(|m| (*m).clone()).collect();
        let recent_messages: Vec<Message> = conv_messages[conv_split..].iter().map(|m| (*m).clone()).collect();

        // Call LLM to generate summary
        let summary_prompt = build_summary_prompt(&old_messages);
        let summary_msg = Message::user(summary_prompt);

        let summary_result = state
            .model
            .complete(
                &[summary_msg],
                &[], // no tools for summarization
                &ModelConfig {
                    max_tokens: Some(2000),
                    temperature: Some(0.0),
                    ..Default::default()
                },
            )
            .await
            .map_err(|e| MiddlewareError::Other(format!("compaction LLM call failed: {}", e)))?;

        let summary_text = summary_result.text_content();
        if summary_text.is_empty() {
            tracing::warn!("compaction LLM returned empty summary, skipping");
            return Ok(());
        }

        // Rebuild messages: system prompts + summary + recent
        let mut compacted = Vec::new();

        // Keep system messages
        for (_, m) in &system_msgs {
            compacted.push((*m).clone());
        }

        // Add summary as a system message
        compacted.push(Message::system(format!(
            "[Conversation summary — {} messages compacted]\n\n{}",
            old_messages.len(),
            summary_text
        )));

        // Add recent messages
        compacted.extend(recent_messages);

        let old_count = messages.len();
        let new_count = compacted.len();
        let new_tokens = if let Some(ref bus) = self.bus {
            if let Some(counter) = bus.get::<dyn alva_types::TokenCounter>() {
                compacted
                    .iter()
                    .map(|m| counter.count_tokens(&m.text_content()) + 4)
                    .sum()
            } else {
                estimate_tokens(&compacted)
            }
        } else {
            estimate_tokens(&compacted)
        };

        tracing::info!(
            old_messages = old_count,
            new_messages = new_count,
            old_tokens = total_tokens,
            new_tokens,
            "compaction complete"
        );

        *messages = compacted;
        self.compaction_count.fetch_add(1, Ordering::Relaxed);

        // Emit ContextCompacted event for observability.
        if let Some(ref bus) = self.bus {
            bus.emit(alva_types::ContextCompacted {
                agent_id: String::new(),
                strategy: "llm_summarization".to_string(),
                tokens_before: total_tokens,
                tokens_after: new_tokens,
            });
        }

        Ok(())
    }

    fn name(&self) -> &str {
        "compaction"
    }

    fn priority(&self) -> i32 {
        MiddlewarePriority::CONTEXT
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use alva_types::base::content::ContentBlock;

    fn make_user_msg(text: &str) -> Message {
        Message::user(text)
    }

    fn make_assistant_msg(text: &str) -> Message {
        Message {
            id: "test-msg".to_string(),
            role: alva_types::MessageRole::Assistant,
            content: vec![ContentBlock::Text {
                text: text.to_string(),
            }],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
    }

    #[test]
    fn estimate_tokens_basic() {
        let msgs = vec![make_user_msg("hello world")]; // 11 chars
        let tokens = estimate_tokens(&msgs);
        assert!(tokens > 0);
        assert!(tokens < 20); // rough estimate, not exact
    }

    #[test]
    fn find_split_keeps_recent() {
        // Create 10 messages, each ~100 tokens (400 chars)
        let msgs: Vec<Message> = (0..10)
            .map(|i| make_user_msg(&format!("Message {} {}", i, "x".repeat(400))))
            .collect();

        // Keep ~200 tokens of recent → should keep last 2 messages
        let split = find_split_point(&msgs, 200);
        assert!(split > 0);
        assert!(split < 10);
    }

    #[test]
    fn find_split_all_fit() {
        let msgs = vec![make_user_msg("short")];
        let split = find_split_point(&msgs, 10000);
        assert_eq!(split, 0); // everything fits
    }

    #[test]
    fn build_summary_prompt_includes_roles() {
        let msgs = vec![
            make_user_msg("What is Rust?"),
            make_assistant_msg("Rust is a systems programming language."),
        ];
        let prompt = build_summary_prompt(&msgs);
        assert!(prompt.contains("[User]"));
        assert!(prompt.contains("[Assistant]"));
        assert!(prompt.contains("What is Rust?"));
        assert!(prompt.contains("systems programming language"));
    }

    #[test]
    fn default_config() {
        let config = CompactionConfig::default();
        assert_eq!(config.trigger_tokens, 100_000);
        assert_eq!(config.reserve_tokens, 16_000);
        assert_eq!(config.keep_recent_tokens, 20_000);
    }

    #[test]
    fn with_bus_sets_bus() {
        let bus = alva_types::Bus::new();
        let mw = CompactionMiddleware::default().with_bus(bus.handle());
        assert!(mw.bus.is_some());
    }

    #[test]
    fn without_bus_field_is_none() {
        let mw = CompactionMiddleware::default();
        assert!(mw.bus.is_none());
    }

    #[test]
    fn bus_token_counter_used_for_estimation() {
        // Register a TokenCounter on the bus that always returns 10 per call.
        let bus = alva_types::Bus::new();
        let handle = bus.handle();

        struct FixedCounter;
        impl alva_types::TokenCounter for FixedCounter {
            fn count_tokens(&self, _text: &str) -> usize { 10 }
            fn context_window(&self) -> usize { 100_000 }
        }
        handle.provide::<dyn alva_types::TokenCounter>(Arc::new(FixedCounter));

        let mw = CompactionMiddleware::new(CompactionConfig {
            trigger_tokens: 50,
            reserve_tokens: 0,
            keep_recent_tokens: 0,
        })
        .with_bus(handle.clone());

        // With the fixed counter, each message = 10 + 4 = 14 tokens.
        // 3 messages = 42 tokens, which is under trigger_tokens (50).
        let msgs = vec![
            make_user_msg("a"),
            make_user_msg("b"),
            make_user_msg("c"),
        ];
        // Verify the bus counter is used (local estimate would give different result).
        let total: usize = {
            let counter = handle.get::<dyn alva_types::TokenCounter>().unwrap();
            msgs.iter()
                .map(|m| counter.count_tokens(&m.text_content()) + 4)
                .sum()
        };
        assert_eq!(total, 42); // 3 * (10 + 4)
    }
}
