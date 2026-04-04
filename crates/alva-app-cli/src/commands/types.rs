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
        estimate_cost_usd(self.input_tokens, self.output_tokens)
    }

    /// Human-readable compact number (e.g., "1.5K", "2.3M").
    pub fn format_total(&self) -> String {
        format_token_count(self.total())
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

/// Shared cost estimation (Claude-family heuristic: $3/M input, $15/M output).
pub fn estimate_cost_usd(input_tokens: u64, output_tokens: u64) -> f64 {
    (input_tokens as f64 * 3.0 + output_tokens as f64 * 15.0) / 1_000_000.0
}

/// Shared compact number formatter (e.g., 1500 → "1.5K", 2500000 → "2.5M").
pub fn format_token_count(n: u64) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}
