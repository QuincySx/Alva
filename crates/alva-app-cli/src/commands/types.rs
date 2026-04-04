use std::collections::HashMap;

/// Command execution result
#[derive(Debug)]
pub enum CommandResult {
    /// Text output to display
    Text(String),
    /// Prompt to send to LLM
    Prompt {
        content: String,
        progress_message: Option<String>,
        allowed_tools: Option<Vec<String>>,
    },
    /// Session was compacted
    Compact { summary: String },
    /// No output needed
    Skip,
    /// Error occurred
    Error(String),
}

/// Accumulated token usage for the session.
#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}

impl TokenUsage {
    pub fn total(&self) -> u64 {
        self.input_tokens + self.output_tokens
    }

    /// Rough cost estimate in USD (Claude-family heuristic: $3/M input, $15/M output).
    pub fn estimated_cost_usd(&self) -> f64 {
        (self.input_tokens as f64 * 3.0 + self.output_tokens as f64 * 15.0) / 1_000_000.0
    }
}

/// Context available to commands during execution
pub struct CommandContext<'a> {
    pub workspace: &'a std::path::Path,
    pub home_dir: std::path::PathBuf,
    pub model: &'a str,
    pub session_id: &'a str,
    /// Number of messages in the current session.
    pub message_count: usize,
    /// Session-level accumulated token usage.
    pub token_usage: TokenUsage,
    /// Names of registered tools.
    pub tool_names: Vec<String>,
    /// Whether plan mode is active.
    pub plan_mode: bool,
    /// Arbitrary extra key-value data.
    pub extra: HashMap<String, String>,
}

/// A slash command
pub trait Command: Send + Sync {
    /// Command name (without /)
    fn name(&self) -> &str;

    /// Alternate names
    fn aliases(&self) -> Vec<&str> {
        vec![]
    }

    /// Human-readable description
    fn description(&self) -> &str;

    /// Whether this command is currently enabled
    fn is_enabled(&self) -> bool {
        true
    }

    /// Execute the command
    fn execute(&self, args: &str, ctx: &CommandContext) -> CommandResult;
}
