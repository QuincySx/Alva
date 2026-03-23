// INPUT:  thiserror, std::io
// OUTPUT: EngineError, SkillError
// POS:    Defines the two root error enums for the engine subsystem and skill subsystem.
//         ChatError and StreamError removed (depended on deleted ui_message/ui_message_stream).
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("LLM provider error: {0}")]
    LLMProvider(String),

    #[error("LLM stream interrupted unexpectedly")]
    LLMStreamInterrupted,

    #[error("Max tokens reached")]
    MaxTokensReached,

    #[error("Max iterations ({0}) reached")]
    MaxIterationsReached(u32),

    #[error("Tool '{0}' not found in registry")]
    ToolNotFound(String),

    #[error("Tool execution error: {0}")]
    ToolExecution(String),

    #[error("Session '{0}' not found")]
    SessionNotFound(String),

    #[error("Session is already running")]
    SessionAlreadyRunning,

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Context compaction failed: {0}")]
    Compaction(String),

    #[error("Operation cancelled")]
    Cancelled,
}

impl From<agent_memory::MemoryError> for EngineError {
    fn from(e: agent_memory::MemoryError) -> Self {
        EngineError::Storage(e.to_string())
    }
}

#[derive(Debug, Error)]
pub enum SkillError {
    #[error("Skill '{0}' not found")]
    SkillNotFound(String),

    #[error("Invalid SKILL.md: {0}")]
    InvalidSkillMd(String),

    #[error("Invalid SKILL.md frontmatter: {0}")]
    InvalidFrontmatter(String),

    #[error("Cannot remove bundled skill '{0}'")]
    CannotRemoveBundledSkill(String),

    #[error("Path traversal attempt: '{0}'")]
    PathTraversal(String),

    #[error("MCP server '{0}' not found")]
    McpServerNotFound(String),

    #[error("MCP server '{0}' not connected")]
    McpNotConnected(String),

    #[error("MCP server '{0}' connect timed out")]
    McpConnectTimeout(String),

    #[error("MCP transport error: {0}")]
    McpTransport(String),

    #[error("MCP tool call error: {0}")]
    McpToolCall(String),

    #[error("Transport type mismatch for server config")]
    TransportMismatch,

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(String),
}
