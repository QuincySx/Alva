use std::collections::HashMap;

// Re-export shared utilities from app-core so builtins.rs can use them.
pub use alva_app_core::{estimate_cost_usd, format_token_count};

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
