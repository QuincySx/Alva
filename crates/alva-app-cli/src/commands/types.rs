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

/// Context available to commands during execution
pub struct CommandContext<'a> {
    pub workspace: &'a std::path::Path,
    pub home_dir: &'a std::path::Path,
    pub model: &'a str,
    pub session_id: &'a str,
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
