// INPUT:  crate::Message
// OUTPUT: TokenEstimator (trait), SimpleTokenEstimator
// POS:    Token counting abstractions — trait for pluggable estimators and a simple heuristic-based default.

/// Trait for estimating token counts.
///
/// Implementations can range from a simple character-ratio heuristic
/// to a full tokenizer (e.g., tiktoken, sentencepiece) wrapper.
pub trait TokenEstimator: Send + Sync {
    /// Estimate the number of tokens in a text string.
    fn estimate_tokens(&self, text: &str) -> usize;

    /// Estimate the total tokens for a slice of messages.
    fn estimate_message_tokens(&self, messages: &[crate::Message]) -> usize;
}

/// Simple estimator using a ~4 characters per token heuristic.
///
/// For ASCII text the Claude tokenizer averages roughly 4 characters per
/// token. CJK and other non-ASCII characters are counted as approximately
/// 1 token each. This provides a fast, dependency-free estimate.
#[derive(Debug, Clone, Default)]
pub struct SimpleTokenEstimator;

impl TokenEstimator for SimpleTokenEstimator {
    fn estimate_tokens(&self, text: &str) -> usize {
        if text.is_empty() {
            return 0;
        }
        let mut count = 0usize;
        let mut ascii_run = 0usize;
        for ch in text.chars() {
            if ch.is_ascii() {
                ascii_run += 1;
            } else {
                // Flush ASCII run
                count += (ascii_run + 3) / 4;
                ascii_run = 0;
                // CJK and other non-ASCII: ~1 token per char
                count += 1;
            }
        }
        // Flush remaining ASCII
        count += (ascii_run + 3) / 4;
        count.max(1) // At least 1 token for non-empty text
    }

    fn estimate_message_tokens(&self, messages: &[crate::Message]) -> usize {
        messages
            .iter()
            .map(|m| {
                let content_tokens = self.estimate_tokens(&m.text_content());
                // Add overhead per message (~4 tokens for role + formatting)
                content_tokens + 4
            })
            .sum()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_string_zero_tokens() {
        let est = SimpleTokenEstimator;
        assert_eq!(est.estimate_tokens(""), 0);
    }

    #[test]
    fn short_ascii_at_least_one() {
        let est = SimpleTokenEstimator;
        assert_eq!(est.estimate_tokens("hi"), 1);
    }

    #[test]
    fn four_chars_one_token() {
        let est = SimpleTokenEstimator;
        assert_eq!(est.estimate_tokens("abcd"), 1);
    }

    #[test]
    fn eight_chars_two_tokens() {
        let est = SimpleTokenEstimator;
        assert_eq!(est.estimate_tokens("abcdefgh"), 2);
    }

    #[test]
    fn cjk_chars_one_per_token() {
        let est = SimpleTokenEstimator;
        // 3 CJK characters = 3 tokens
        assert_eq!(est.estimate_tokens("你好世"), 3);
    }

    #[test]
    fn mixed_content() {
        let est = SimpleTokenEstimator;
        // "hello" = 5 ASCII chars => ceil(5/4) = 2 tokens
        // "世界" = 2 CJK chars => 2 tokens
        // Total = 4
        let tokens = est.estimate_tokens("hello世界");
        assert_eq!(tokens, 4);
    }

    #[test]
    fn message_token_estimation() {
        let est = SimpleTokenEstimator;
        let messages = vec![
            crate::Message::user("Hello, world!"), // 13 ASCII chars => ceil(13/4)=4 + 4 overhead = 8
            crate::Message::system("Be helpful."),  // 11 ASCII chars => ceil(11/4)=3 + 4 overhead = 7
        ];
        let total = est.estimate_message_tokens(&messages);
        // 8 + 7 = 15
        assert_eq!(total, 15);
    }
}
