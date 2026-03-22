use std::sync::Arc;

use agent_base::{AgentError, LanguageModel};
use agent_core::AgentMessage;

/// Configuration for context-window compaction.
///
/// When the estimated token count of the message history exceeds
/// `max_tokens`, the session can compact older messages to stay within
/// the model's context window.
pub struct CompactionConfig {
    /// Estimated token budget. Compaction is triggered when the message
    /// history exceeds this threshold.
    pub max_tokens: usize,

    /// Number of most-recent messages to always preserve (never summarised).
    pub keep_recent: usize,

    /// The model to use for generating summaries (future use).
    pub model: Arc<dyn LanguageModel>,
}

/// Rough token-count estimate: ~4 characters per token.
pub fn estimate_tokens(messages: &[AgentMessage]) -> usize {
    messages
        .iter()
        .map(|m| match m {
            AgentMessage::Standard(msg) => msg.text_content().len() / 4,
            AgentMessage::Custom { data, .. } => data.to_string().len() / 4,
        })
        .sum()
}

/// Returns `true` if the estimated token count exceeds the configured maximum.
pub fn should_compact(messages: &[AgentMessage], config: &CompactionConfig) -> bool {
    estimate_tokens(messages) > config.max_tokens
}

/// Compact the message history to fit within the token budget.
///
/// Currently uses simple truncation: keeps only the `keep_recent` most recent
/// messages. A future version will use LLM-generated summaries for the
/// discarded prefix.
pub async fn compact_messages(
    messages: &[AgentMessage],
    config: &CompactionConfig,
) -> Result<Vec<AgentMessage>, AgentError> {
    // Future: LLM-generated summary of old messages using config.model
    // For now, simple truncation: keep only keep_recent most recent
    let keep = config.keep_recent.min(messages.len());
    Ok(messages[messages.len() - keep..].to_vec())
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use agent_base::Message;

    fn make_message(text: &str) -> AgentMessage {
        AgentMessage::Standard(Message::user(text))
    }

    #[test]
    fn estimate_tokens_basic() {
        // "hello world" = 11 chars => 11/4 = 2 tokens
        let messages = vec![make_message("hello world")];
        assert_eq!(estimate_tokens(&messages), 2);
    }

    #[test]
    fn estimate_tokens_multiple_messages() {
        // 8 chars => 2, 12 chars => 3, total => 5
        let messages = vec![make_message("12345678"), make_message("123456789012")];
        assert_eq!(estimate_tokens(&messages), 5);
    }

    #[test]
    fn estimate_tokens_custom_message() {
        let messages = vec![AgentMessage::Custom {
            type_name: "test".into(),
            data: serde_json::json!({"key": "value"}),
        }];
        let tokens = estimate_tokens(&messages);
        assert!(tokens > 0);
    }

    #[tokio::test]
    async fn compact_truncates_to_keep_recent() {
        use agent_base::*;
        use async_trait::async_trait;
        use futures_core::Stream;
        use std::pin::Pin;

        // Dummy model — not used by current truncation impl
        struct DummyModel;

        #[async_trait]
        impl LanguageModel for DummyModel {
            fn model_id(&self) -> &str {
                "dummy"
            }
            async fn complete(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Result<Message, AgentError> {
                unimplemented!()
            }
            fn stream(
                &self,
                _messages: &[Message],
                _tools: &[&dyn Tool],
                _config: &ModelConfig,
            ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
                unimplemented!()
            }
        }

        let config = CompactionConfig {
            max_tokens: 100,
            keep_recent: 2,
            model: Arc::new(DummyModel),
        };

        let messages = vec![
            make_message("oldest message"),
            make_message("middle message"),
            make_message("newest message"),
        ];

        let compacted = compact_messages(&messages, &config).await.unwrap();
        assert_eq!(compacted.len(), 2);
    }
}
