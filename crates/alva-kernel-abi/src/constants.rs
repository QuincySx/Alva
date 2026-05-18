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

/// Terminal spinner frame rate in milliseconds.
pub const SPINNER_FRAME_RATE_MS: u64 = 80;

/// Agent tool availability matrix.
pub mod agent_tools {
    /// Tools disallowed for all agent types.
    pub const ALL_AGENT_DISALLOWED: &[&str] = &["EnterPlanMode", "ExitPlanMode"];

    /// Tools allowed only for coordinator mode.
    pub const COORDINATOR_ONLY: &[&str] = &["TeamCreate", "TeamDelete", "SendMessage"];

    /// Tools allowed for async/background agents.
    pub const ASYNC_AGENT_ALLOWED: &[&str] = &[
        "Read",
        "Glob",
        "Grep",
        "Bash",
        "Edit",
        "Write",
        "WebFetch",
        "WebSearch",
        "TaskCreate",
        "TaskUpdate",
        "SendMessage",
    ];

    /// Tools allowed for in-process teammate agents.
    pub const IN_PROCESS_TEAMMATE_ALLOWED: &[&str] = &[
        "Read",
        "Glob",
        "Grep",
        "Bash",
        "Edit",
        "Write",
        "WebFetch",
        "WebSearch",
        "TaskCreate",
        "TaskUpdate",
        "TaskGet",
        "TaskList",
        "SendMessage",
    ];
}

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
        assert!(AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT > 0.0);
        assert!(AUTO_COMPACT_TOKEN_THRESHOLD_PERCENT <= 1.0);
    }

    #[test]
    fn max_tool_timeout_is_at_least_default() {
        // Pin: MAX must allow DEFAULT — otherwise users get a
        // compile-clean panic when an explicit timeout exceeds MAX.
        assert!(MAX_TOOL_TIMEOUT_MS >= DEFAULT_TOOL_TIMEOUT_MS);
    }

    #[test]
    fn positive_counts_and_sizes() {
        // Pin: zero anywhere here breaks the corresponding feature
        // silently (history disabled, no concurrent tools, etc.).
        assert!(MAX_TOOL_RESULT_INLINE_CHARS > 0);
        assert!(MAX_GLOB_RESULTS > 0);
        assert!(MAX_CONCURRENT_TOOL_EXECUTIONS > 0);
        assert!(MAX_AGENT_DEPTH > 0);
        assert!(MAX_HISTORY_ENTRIES > 0);
        assert!(DEFAULT_MAX_OUTPUT_TOKENS > 0);
        assert!(AUTO_COMPACT_MESSAGE_THRESHOLD > 0);
        assert!(MEMORY_EXTRACTION_THRESHOLD > 0);
        assert!(SPINNER_FRAME_RATE_MS > 0);
    }

    // -- agent_tools list invariants --------------------------------------

    #[test]
    fn agent_tool_lists_are_all_non_empty() {
        // Empty ASYNC_AGENT_ALLOWED would silently strip ALL tool
        // permissions from background agents.
        assert!(!agent_tools::ALL_AGENT_DISALLOWED.is_empty());
        assert!(!agent_tools::COORDINATOR_ONLY.is_empty());
        assert!(!agent_tools::ASYNC_AGENT_ALLOWED.is_empty());
        assert!(!agent_tools::IN_PROCESS_TEAMMATE_ALLOWED.is_empty());
    }

    #[test]
    fn all_agent_disallowed_does_not_appear_in_async_allowed() {
        // A tool in BOTH lists would be ambiguous — pinned so a
        // future addition doesn't accidentally allow what's
        // universally disallowed (EnterPlanMode / ExitPlanMode).
        for disallowed in agent_tools::ALL_AGENT_DISALLOWED {
            assert!(
                !agent_tools::ASYNC_AGENT_ALLOWED.contains(disallowed),
                "tool {disallowed:?} is both disallowed and allowed for async agents"
            );
        }
    }

    #[test]
    fn all_agent_disallowed_does_not_appear_in_in_process_allowed() {
        for disallowed in agent_tools::ALL_AGENT_DISALLOWED {
            assert!(
                !agent_tools::IN_PROCESS_TEAMMATE_ALLOWED.contains(disallowed),
                "tool {disallowed:?} is both disallowed and allowed for in-process teammates"
            );
        }
    }

    #[test]
    fn coordinator_only_tools_are_not_in_async_allowed() {
        // Pin: "coordinator only" means just that — if a tool ends
        // up in BOTH lists, the name lies (it's coordinator + async).
        // TeamCreate / TeamDelete must NOT be allowed for background
        // agents.
        for tool in agent_tools::COORDINATOR_ONLY {
            // SendMessage is intentionally shared (it's coordinator +
            // both teammate types) — exclude it from the assertion
            // so this test pins the OTHER coordinator-only tools.
            if *tool == "SendMessage" {
                continue;
            }
            assert!(
                !agent_tools::ASYNC_AGENT_ALLOWED.contains(tool),
                "coordinator-only tool {tool:?} leaked into ASYNC_AGENT_ALLOWED"
            );
            assert!(
                !agent_tools::IN_PROCESS_TEAMMATE_ALLOWED.contains(tool),
                "coordinator-only tool {tool:?} leaked into IN_PROCESS_TEAMMATE_ALLOWED"
            );
        }
    }

    #[test]
    fn in_process_teammate_is_a_superset_of_async_agent_in_practice() {
        // Pin the empirical relationship: in-process teammates get
        // EVERYTHING async agents get, plus TaskGet / TaskList for
        // synchronous read-back. If a future tool is added to
        // ASYNC_AGENT_ALLOWED without also being added to the
        // in-process list, the in-process variant becomes silently
        // less capable.
        for tool in agent_tools::ASYNC_AGENT_ALLOWED {
            assert!(
                agent_tools::IN_PROCESS_TEAMMATE_ALLOWED.contains(tool),
                "async-allowed tool {tool:?} missing from in-process teammate list"
            );
        }
    }

    #[test]
    fn agent_tools_have_no_internal_duplicates() {
        // Defensive pin: a duplicate entry would be a silent typo.
        for list in [
            agent_tools::ALL_AGENT_DISALLOWED,
            agent_tools::COORDINATOR_ONLY,
            agent_tools::ASYNC_AGENT_ALLOWED,
            agent_tools::IN_PROCESS_TEAMMATE_ALLOWED,
        ] {
            let mut seen = std::collections::HashSet::new();
            for t in list {
                assert!(seen.insert(*t), "duplicate tool {t:?} in list");
            }
        }
    }
}
