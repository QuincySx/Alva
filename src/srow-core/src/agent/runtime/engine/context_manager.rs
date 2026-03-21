// INPUT:  crate::domain::message, crate::error
// OUTPUT: ContextManager
// POS:    Manages context window size by triggering message history truncation when token threshold is exceeded.
use crate::domain::message::{LLMMessage, Role};
use crate::error::EngineError;

/// Manages context window size, triggering compaction when token count exceeds threshold.
pub struct ContextManager {
    /// Token threshold to trigger compaction (0 = disabled)
    threshold: u32,
    /// Number of recent messages to keep after compaction
    keep_recent: usize,
}

impl ContextManager {
    pub fn new(threshold: u32) -> Self {
        Self {
            threshold,
            keep_recent: 20,
        }
    }

    /// Check whether the current history exceeds the compaction threshold.
    pub fn needs_compaction(&self, history: &[LLMMessage], system: &str) -> bool {
        if self.threshold == 0 {
            return false;
        }
        let estimated: u32 = history
            .iter()
            .filter_map(|m| m.token_count)
            .sum::<u32>()
            + (system.len() / 4) as u32;
        estimated >= self.threshold
    }

    /// Strategy A (simple): truncate to keep_recent messages, ensuring first message is User role.
    /// Future: Strategy B would call LLM to summarize the dropped history.
    pub async fn compact(
        &self,
        history: Vec<LLMMessage>,
        _system: &str,
    ) -> Result<Vec<LLMMessage>, EngineError> {
        if history.len() <= self.keep_recent {
            return Ok(history);
        }

        let start = history.len().saturating_sub(self.keep_recent);
        let mut truncated = history[start..].to_vec();

        // Ensure the first message has User role (LLM API requirement)
        while !truncated.is_empty() && truncated[0].role != Role::User {
            truncated.remove(0);
        }

        tracing::info!(
            "Context compacted: {} -> {} messages",
            history.len(),
            truncated.len()
        );

        Ok(truncated)
    }
}
