// INPUT:  (none — standalone state tracker)
// OUTPUT: AutoCompactState
// POS:    Tracks state for automatic compaction triggering — token counts, message counts, compaction history.
//! Auto-compact state tracking — monitors when automatic compaction should trigger.
//!
//! Used by the agent loop or context middleware to determine when to invoke
//! the compaction service based on accumulated token usage and message counts.

/// Tracks state for automatic compaction triggering.
///
/// This is a lightweight bookkeeping struct — it does not perform compaction
/// itself. The agent loop checks `should_trigger()` and then calls into the
/// compact module when needed.
#[derive(Debug, Clone)]
pub struct AutoCompactState {
    /// Current estimated token count across all messages.
    pub current_tokens: usize,
    /// Number of messages added since last compaction.
    pub messages_since_compact: usize,
    /// Whether a reactive compact has been attempted for the current budget overage.
    pub has_attempted_reactive: bool,
    /// Total number of compactions performed in this session.
    pub compaction_count: usize,
}

impl AutoCompactState {
    /// Create a new tracking state with zero counters.
    pub fn new() -> Self {
        Self {
            current_tokens: 0,
            messages_since_compact: 0,
            has_attempted_reactive: false,
            compaction_count: 0,
        }
    }

    /// Record that a new message was added to the conversation.
    ///
    /// Call this after each user or assistant message is appended.
    pub fn on_message_added(&mut self, estimated_tokens: usize) {
        self.current_tokens += estimated_tokens;
        self.messages_since_compact += 1;
    }

    /// Record that a compaction was completed successfully.
    ///
    /// Resets the message counter and adjusts the token count.
    pub fn on_compaction_complete(&mut self, tokens_saved: usize) {
        self.current_tokens = self.current_tokens.saturating_sub(tokens_saved);
        self.messages_since_compact = 0;
        self.compaction_count += 1;
        self.has_attempted_reactive = false;
    }

    /// Check whether compaction should be triggered based on current state
    /// and the given compaction config.
    pub fn should_trigger(&self, config: &super::compact::CompactionConfig) -> bool {
        super::compact::should_compact(
            // We only need message count check here, use a dummy slice
            // The actual message slice is checked by the caller
            &[],
            config,
            self.current_tokens,
        ) || self.messages_since_compact > alva_kernel_abi::constants::AUTO_COMPACT_MESSAGE_THRESHOLD
    }

    /// Mark that a reactive compaction attempt was made.
    ///
    /// Prevents repeated reactive compaction attempts within the same
    /// budget overage window.
    pub fn mark_reactive_attempted(&mut self) {
        self.has_attempted_reactive = true;
    }

    /// Reset all counters (e.g., on conversation clear).
    pub fn reset(&mut self) {
        self.current_tokens = 0;
        self.messages_since_compact = 0;
        self.has_attempted_reactive = false;
        // Note: compaction_count is NOT reset — it's session-lifetime.
    }
}

impl Default for AutoCompactState {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::CompactionConfig;

    #[test]
    fn new_state_is_zeroed() {
        let state = AutoCompactState::new();
        assert_eq!(state.current_tokens, 0);
        assert_eq!(state.messages_since_compact, 0);
        assert!(!state.has_attempted_reactive);
        assert_eq!(state.compaction_count, 0);
    }

    #[test]
    fn on_message_added_accumulates() {
        let mut state = AutoCompactState::new();
        state.on_message_added(100);
        state.on_message_added(200);
        assert_eq!(state.current_tokens, 300);
        assert_eq!(state.messages_since_compact, 2);
    }

    #[test]
    fn on_compaction_complete_adjusts() {
        let mut state = AutoCompactState::new();
        state.on_message_added(1000);
        state.on_message_added(500);
        state.has_attempted_reactive = true;

        state.on_compaction_complete(800);

        assert_eq!(state.current_tokens, 700);
        assert_eq!(state.messages_since_compact, 0);
        assert_eq!(state.compaction_count, 1);
        assert!(!state.has_attempted_reactive);
    }

    #[test]
    fn on_compaction_complete_saturates() {
        let mut state = AutoCompactState::new();
        state.on_message_added(100);
        state.on_compaction_complete(500); // more than current
        assert_eq!(state.current_tokens, 0);
    }

    #[test]
    fn should_trigger_by_tokens() {
        let mut state = AutoCompactState::new();
        let config = CompactionConfig {
            max_tokens: 1000,
            trigger_threshold: 0.8,
            ..Default::default()
        };

        state.on_message_added(700);
        assert!(!state.should_trigger(&config));

        state.on_message_added(200);
        // 900 > 800 threshold
        assert!(state.should_trigger(&config));
    }

    #[test]
    fn should_trigger_by_message_count() {
        let mut state = AutoCompactState::new();
        let config = CompactionConfig {
            max_tokens: 1_000_000, // very high, won't trigger by tokens
            ..Default::default()
        };

        for _ in 0..201 {
            state.on_message_added(1);
        }
        assert!(state.should_trigger(&config));
    }

    #[test]
    fn reset_preserves_compaction_count() {
        let mut state = AutoCompactState::new();
        state.on_message_added(500);
        state.on_compaction_complete(300);
        assert_eq!(state.compaction_count, 1);

        state.on_message_added(200);
        state.reset();

        assert_eq!(state.current_tokens, 0);
        assert_eq!(state.messages_since_compact, 0);
        assert_eq!(state.compaction_count, 1); // preserved
    }
}
