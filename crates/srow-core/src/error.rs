// INPUT:  thiserror, std::io, alva_agent_memory::MemoryError
// OUTPUT: EngineError, SkillError
// POS:    Defines the two root error enums for the engine and skill subsystems, with From<MemoryError> conversion.
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
    Storage(#[source] Box<dyn std::error::Error + Send + Sync>),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Context compaction failed: {0}")]
    Compaction(String),

    #[error("Operation cancelled")]
    Cancelled,
}

impl EngineError {
    /// Create a Storage error from any displayable message.
    pub fn storage(msg: impl std::fmt::Display) -> Self {
        Self::Storage(msg.to_string().into())
    }
}

impl From<alva_agent_memory::MemoryError> for EngineError {
    fn from(e: alva_agent_memory::MemoryError) -> Self {
        EngineError::Storage(Box::new(e))
    }
}

impl From<alva_protocol_mcp::error::McpError> for SkillError {
    fn from(e: alva_protocol_mcp::error::McpError) -> Self {
        match e {
            alva_protocol_mcp::error::McpError::ServerNotFound(s) => Self::McpServerNotFound(s),
            alva_protocol_mcp::error::McpError::NotConnected(s) => Self::McpNotConnected(s),
            alva_protocol_mcp::error::McpError::ConnectTimeout(s) => Self::McpConnectTimeout(s),
            alva_protocol_mcp::error::McpError::Transport(s) => Self::McpTransport(s),
            alva_protocol_mcp::error::McpError::ToolExecution(s) => Self::McpToolCall(s),
            alva_protocol_mcp::error::McpError::Serialization(s) => Self::Serialization(s),
            alva_protocol_mcp::error::McpError::Io(s) => Self::Io(s),
        }
    }
}

impl From<alva_protocol_skill::error::SkillError> for SkillError {
    fn from(e: alva_protocol_skill::error::SkillError) -> Self {
        match e {
            alva_protocol_skill::error::SkillError::SkillNotFound(s) => Self::SkillNotFound(s),
            alva_protocol_skill::error::SkillError::InvalidSkillMd(s) => Self::InvalidSkillMd(s),
            alva_protocol_skill::error::SkillError::InvalidFrontmatter(s) => {
                Self::InvalidFrontmatter(s)
            }
            alva_protocol_skill::error::SkillError::CannotRemoveBundledSkill(s) => {
                Self::CannotRemoveBundledSkill(s)
            }
            alva_protocol_skill::error::SkillError::PathTraversal(s) => Self::PathTraversal(s),
            alva_protocol_skill::error::SkillError::Serialization(s) => Self::Serialization(s),
            alva_protocol_skill::error::SkillError::Io(s) => Self::Io(s),
        }
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
