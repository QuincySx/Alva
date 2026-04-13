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
