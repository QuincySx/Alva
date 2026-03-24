// INPUT:  alva_types (ContentBlock, Message), async_trait, tracing, super::{Middleware, MiddlewareContext, MiddlewareError}
// OUTPUT: CompressionConfig, CompressionMiddleware
// POS:    Context compression middleware — truncates old messages and inserts a summary marker when estimated token count exceeds threshold.
//! Context compression middleware — automatically compresses conversation
//! history when token count exceeds threshold.

use async_trait::async_trait;
use alva_types::{ContentBlock, Message};

use super::{Middleware, MiddlewareContext, MiddlewareError};

/// Configuration for context compression.
pub struct CompressionConfig {
    /// Estimated max tokens before compression triggers.
    pub token_threshold: u32,
    /// Number of recent messages to always keep uncompressed.
    pub keep_recent: usize,
    /// Approximate tokens per character (for estimation).
    pub tokens_per_char: f32,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            token_threshold: 100_000,
            keep_recent: 20,
            tokens_per_char: 0.25,
        }
    }
}

/// Middleware that compresses old messages when token count exceeds threshold.
///
/// Uses a simple truncation + summary marker approach. For LLM-powered
/// summarization, extend this middleware.
pub struct CompressionMiddleware {
    config: CompressionConfig,
}

impl CompressionMiddleware {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    pub fn with_defaults() -> Self {
        Self::new(CompressionConfig::default())
    }

    fn estimate_tokens(&self, messages: &[Message]) -> u32 {
        let total_chars: usize = messages
            .iter()
            .flat_map(|m| &m.content)
            .map(|block| match block {
                ContentBlock::Text { text } => text.len(),
                ContentBlock::Reasoning { text } => text.len(),
                ContentBlock::ToolResult { content, .. } => content.len(),
                ContentBlock::ToolUse { input, .. } => input.to_string().len(),
                ContentBlock::Image { data, .. } => data.len(),
            })
            .sum();
        (total_chars as f32 * self.config.tokens_per_char) as u32
    }
}

#[async_trait]
impl Middleware for CompressionMiddleware {
    async fn before_llm_call(
        &self,
        _ctx: &mut MiddlewareContext,
        messages: &mut Vec<Message>,
    ) -> Result<(), MiddlewareError> {
        let estimated = self.estimate_tokens(messages);
        if estimated <= self.config.token_threshold {
            return Ok(());
        }

        let total = messages.len();
        if total <= self.config.keep_recent + 1 {
            return Ok(());
        }

        let compress_end = total - self.config.keep_recent;
        let compressed_count = compress_end - 1; // exclude system prompt

        let summary = Message::system(&format!(
            "[Context compressed: {} earlier messages were summarized to save tokens. \
             The conversation continues below with the most recent {} messages.]",
            compressed_count, self.config.keep_recent
        ));

        let mut new_messages = Vec::with_capacity(self.config.keep_recent + 2);
        new_messages.push(messages[0].clone()); // system prompt
        new_messages.push(summary);
        new_messages.extend_from_slice(&messages[compress_end..]);

        *messages = new_messages;
        tracing::info!(
            compressed = compressed_count,
            remaining = messages.len(),
            "context compressed"
        );

        Ok(())
    }

    fn name(&self) -> &str {
        "compression"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::Message;
    use crate::middleware::Extensions;

    #[tokio::test]
    async fn compression_triggers_above_threshold() {
        let config = CompressionConfig {
            token_threshold: 10,
            keep_recent: 2,
            tokens_per_char: 1.0,
        };
        let mw = CompressionMiddleware::new(config);

        let mut messages = vec![
            Message::system("sys"),
            Message::user("msg1 with some content here"),
            Message::user("msg2 with some content here"),
            Message::user("msg3 with some content here"),
            Message::user("msg4 recent"),
            Message::user("msg5 recent"),
        ];

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: "sys".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();
        // system + summary + 2 recent = 4
        assert_eq!(messages.len(), 4);
    }

    #[tokio::test]
    async fn compression_skips_below_threshold() {
        let mw = CompressionMiddleware::new(CompressionConfig {
            token_threshold: 1_000_000,
            ..Default::default()
        });

        let mut messages = vec![
            Message::system("sys"),
            Message::user("hello"),
        ];
        let original_len = messages.len();

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: "sys".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();
        assert_eq!(messages.len(), original_len);
    }

    #[tokio::test]
    async fn compression_preserves_system_and_recent() {
        let config = CompressionConfig {
            token_threshold: 5,
            keep_recent: 1,
            tokens_per_char: 1.0,
        };
        let mw = CompressionMiddleware::new(config);

        let mut messages = vec![
            Message::system("system prompt"),
            Message::user("old message 1"),
            Message::user("old message 2"),
            Message::user("most recent message"),
        ];

        let mut ctx = MiddlewareContext {
            session_id: "test".into(),
            system_prompt: "system prompt".into(),
            messages: vec![],
            extensions: Extensions::new(),
        };

        mw.before_llm_call(&mut ctx, &mut messages).await.unwrap();
        // system + summary + 1 recent = 3
        assert_eq!(messages.len(), 3);
        // First message should be original system prompt
        assert!(matches!(&messages[0].content[0], ContentBlock::Text { text } if text == "system prompt"));
        // Last message should be the most recent
        assert!(matches!(&messages[2].content[0], ContentBlock::Text { text } if text == "most recent message"));
    }
}
