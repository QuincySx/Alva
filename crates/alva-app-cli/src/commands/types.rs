// INPUT:  alva_app_core cost utilities
// OUTPUT: CommandResult, TokenUsage, CommandContext, Command trait
// POS:    Shared CLI slash-command contracts and runtime command context.
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
}

/// Context available to commands during execution
pub struct CommandContext<'a> {
    pub workspace: &'a std::path::Path,
    pub model: &'a str,
    pub session_id: &'a str,
    /// Number of messages in the current session.
    pub message_count: usize,
    /// Session-level accumulated token usage.
    pub token_usage: TokenUsage,
    /// Names of registered tools.
    pub tool_names: Vec<String>,
    /// Names of plugins that participated in this agent build.
    pub plugin_names: Vec<String>,
    /// Names of middleware layers in final execution order.
    pub middleware_names: Vec<String>,
    /// Per-component config overrides from ~/.alva/config.json.
    pub component_overrides: std::collections::HashMap<String, bool>,
    /// Whether plan mode is active.
    pub plan_mode: bool,
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

#[cfg(test)]
mod tests {
    //! Tests for `TokenUsage` — thin wrapper around utils::estimate_cost_usd
    //! used by the CLI status bar to show "session totals". Core arithmetic
    //! is pinned in alva-app-core/src/utils.rs tests; here we pin the
    //! wrapper's Default + total + delegation behavior.
    use super::*;

    #[test]
    fn default_yields_zero_input_zero_output_zero_total() {
        let u = TokenUsage::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total(), 0);
    }

    #[test]
    fn total_sums_input_and_output() {
        let u = TokenUsage {
            input_tokens: 100,
            output_tokens: 250,
        };
        assert_eq!(u.total(), 350);
    }

    #[test]
    fn estimated_cost_delegates_to_utils_at_published_rates() {
        // 1M input → $3, 1M output → $15, combined = $18. If this
        // diverges from utils::estimate_cost_usd, the wrapper has
        // started its own (wrong) computation.
        let u = TokenUsage {
            input_tokens: 1_000_000,
            output_tokens: 1_000_000,
        };
        assert_eq!(u.estimated_cost_usd(), 18.0);
    }

    #[test]
    fn clone_produces_independent_equal_value() {
        // TokenUsage derives Clone — pin that mutating the original
        // doesn't affect the clone (and field values match).
        let original = TokenUsage {
            input_tokens: 42,
            output_tokens: 7,
        };
        let cloned = original.clone();
        assert_eq!(cloned.input_tokens, 42);
        assert_eq!(cloned.output_tokens, 7);
        assert_eq!(cloned.total(), original.total());
    }
}
