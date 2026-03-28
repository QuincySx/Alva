// INPUT:  thiserror
// OUTPUT: pub enum AgentError
// POS:    Unified error enum for agent-level failures including LLM, tool, cancellation, and configuration errors.
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AgentError {
    #[error("LLM error: {0}")]
    LlmError(String),
    #[error("Tool error: {tool_name}: {message}")]
    ToolError { tool_name: String, message: String },
    #[error("Cancelled")]
    Cancelled,
    #[error("Max iterations reached: {0}")]
    MaxIterations(u32),
    #[error("Configuration error: {0}")]
    ConfigError(String),
    #[error("{0}")]
    Other(String),
}
