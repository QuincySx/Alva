use std::time::Duration;

use agent_types::AgentError;

/// Configuration for retry behaviour with exponential backoff.
///
/// Used by [`AgentSession`](crate::AgentSession) to automatically retry
/// transient failures (e.g. LLM rate-limits or network errors).
pub struct RetryConfig {
    /// Maximum number of retry attempts before giving up.
    pub max_retries: u32,

    /// Base delay for the first retry. Subsequent delays double.
    pub initial_delay: Duration,

    /// Upper bound on the computed delay.
    pub max_delay: Duration,

    /// Predicate that determines whether a given error is retryable.
    pub retryable: Box<dyn Fn(&AgentError) -> bool + Send + Sync>,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            retryable: Box::new(|e| matches!(e, AgentError::LlmError(_))),
        }
    }
}

impl RetryConfig {
    /// Compute the delay for a given zero-based attempt number using
    /// exponential backoff: `initial_delay * 2^attempt`, capped at `max_delay`.
    pub fn delay_for_attempt(&self, attempt: u32) -> Duration {
        let delay_ms = self.initial_delay.as_millis() as u64 * 2u64.pow(attempt);
        Duration::from_millis(delay_ms.min(self.max_delay.as_millis() as u64))
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delay_exponential_growth() {
        let config = RetryConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(30),
            ..RetryConfig::default()
        };

        assert_eq!(config.delay_for_attempt(0), Duration::from_secs(1));
        assert_eq!(config.delay_for_attempt(1), Duration::from_secs(2));
        assert_eq!(config.delay_for_attempt(2), Duration::from_secs(4));
        assert_eq!(config.delay_for_attempt(3), Duration::from_secs(8));
        assert_eq!(config.delay_for_attempt(4), Duration::from_secs(16));
    }

    #[test]
    fn delay_capped_at_max() {
        let config = RetryConfig {
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            ..RetryConfig::default()
        };

        // 2^4 = 16 > 10, should be capped
        assert_eq!(config.delay_for_attempt(4), Duration::from_secs(10));
        // 2^10 = 1024 >> 10, should be capped
        assert_eq!(config.delay_for_attempt(10), Duration::from_secs(10));
    }

    #[test]
    fn default_retryable_matches_llm_error() {
        let config = RetryConfig::default();
        assert!((config.retryable)(&AgentError::LlmError("timeout".into())));
        assert!(!(config.retryable)(&AgentError::Cancelled));
        assert!(!(config.retryable)(&AgentError::Other("misc".into())));
    }
}
