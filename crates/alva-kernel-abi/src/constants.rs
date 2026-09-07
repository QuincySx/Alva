// INPUT:  (none)
// OUTPUT: Various application-wide constants
// POS:    Shared constants matching Claude Code's constants/ — tool limits, agent config, UI parameters.

//! Application-wide constants matching Claude Code's constants.

/// Maximum characters for tool results before disk persistence.
pub const MAX_TOOL_RESULT_INLINE_CHARS: usize = 100_000;

/// Maximum number of files returned by glob/find tools.
pub const MAX_GLOB_RESULTS: usize = 100;

/// Default timeout for tool execution in milliseconds.
pub const DEFAULT_TOOL_TIMEOUT_MS: u64 = 120_000;

/// Maximum timeout for tool execution in milliseconds (10 minutes).
pub const MAX_TOOL_TIMEOUT_MS: u64 = 600_000;

/// Maximum messages in a conversation before auto-compact triggers.
pub const AUTO_COMPACT_MESSAGE_THRESHOLD: usize = 200;

/// Token budget percentage that triggers auto-compact.
pub const AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT: f64 = 0.8;

/// Maximum number of concurrent tool executions.
pub const MAX_CONCURRENT_TOOL_EXECUTIONS: usize = 5;

/// Maximum depth for sub-agent spawning.
pub const MAX_AGENT_DEPTH: usize = 5;

/// Default max output tokens per LLM call.
pub const DEFAULT_MAX_OUTPUT_TOKENS: usize = 16384;

/// History file maximum entries.
pub const MAX_HISTORY_ENTRIES: usize = 1000;

/// Session memory extraction threshold (messages since last extraction).
pub const MEMORY_EXTRACTION_THRESHOLD: usize = 20;

/// Background agent summary interval in seconds.
pub const AGENT_SUMMARY_INTERVAL_SECS: u64 = 30;

/// Rate limit tracking window in hours.
pub const RATE_LIMIT_WINDOW_HOURS: u64 = 5;

/// Policy limits polling interval in seconds.
pub const POLICY_LIMITS_POLL_INTERVAL_SECS: u64 = 3600;

/// Web fetch cache TTL in seconds (15 minutes).
pub const WEB_FETCH_CACHE_TTL_SECS: u64 = 900;

/// Maximum file size for inline paste content.
pub const MAX_INLINE_PASTE_BYTES: usize = 1024;

#[cfg(test)]
mod tests {
    //! Tests for constants: relationships and ranges, NOT specific
    //! values. Mirroring const values into tests just doubles the
    //! source — the worthwhile pin is invariants between constants
    //! (max ≥ default, percent in [0,1], permission lists non-empty
    //! + non-overlapping where they shouldn't overlap).
    use super::*;

    // -- Range invariants --------------------------------------------------

    #[test]
    fn auto_compact_token_threshold_is_a_percentage() {
        // Used as a multiplier on the context window: must be > 0
        // (otherwise auto-compact fires at 0 tokens, every turn) and
        // ≤ 1.0 (otherwise it never fires).
        const {
            assert!(AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT > 0.0);
            assert!(AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT <= 1.0);
        }
    }

    #[test]
    fn max_tool_timeout_is_at_least_default() {
        // Pin: MAX must allow DEFAULT — otherwise users get a
        // compile-clean panic when an explicit timeout exceeds MAX.
        const { assert!(MAX_TOOL_TIMEOUT_MS >= DEFAULT_TOOL_TIMEOUT_MS) }
    }

    #[test]
    fn positive_counts_and_sizes() {
        // Pin: zero anywhere here breaks the corresponding feature
        // silently (history disabled, no concurrent tools, etc.).
        const {
            assert!(MAX_TOOL_RESULT_INLINE_CHARS > 0);
            assert!(MAX_GLOB_RESULTS > 0);
            assert!(MAX_CONCURRENT_TOOL_EXECUTIONS > 0);
            assert!(MAX_AGENT_DEPTH > 0);
            assert!(MAX_HISTORY_ENTRIES > 0);
            assert!(DEFAULT_MAX_OUTPUT_TOKENS > 0);
            assert!(AUTO_COMPACT_MESSAGE_THRESHOLD > 0);
            assert!(MEMORY_EXTRACTION_THRESHOLD > 0);
        }
    }
}
