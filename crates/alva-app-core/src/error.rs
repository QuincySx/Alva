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

#[cfg(test)]
mod tests {
    //! Tests for the engine + skill error types: Display interpolation
    //! and the From<McpError> / From<protocol_skill::SkillError>
    //! mappings.
    //!
    //! From-mappings are silent miscategorization risks — a wrong arm
    //! shows users the wrong error name with no compile-time signal.
    //! Display strings drive the actual user-facing message; missing
    //! the {0} placeholder yields garbage like "Tool '' not found".
    use super::*;

    // -- EngineError Display strings --------------------------------------

    #[test]
    fn display_llm_provider_interpolates_payload() {
        let e = EngineError::LLMProvider("rate limit".into());
        assert_eq!(format!("{e}"), "LLM provider error: rate limit");
    }

    #[test]
    fn display_max_iterations_includes_count() {
        let e = EngineError::MaxIterationsReached(42);
        assert_eq!(format!("{e}"), "Max iterations (42) reached");
    }

    #[test]
    fn display_tool_not_found_quotes_name() {
        // Pin: quotes around the name make the boundary visible
        // when names contain whitespace.
        let e = EngineError::ToolNotFound("read file".into());
        assert_eq!(format!("{e}"), "Tool 'read file' not found in registry");
    }

    #[test]
    fn display_session_not_found_quotes_id() {
        let e = EngineError::SessionNotFound("sid-1".into());
        assert_eq!(format!("{e}"), "Session 'sid-1' not found");
    }

    #[test]
    fn display_constant_variants_have_expected_text() {
        assert_eq!(format!("{}", EngineError::LLMStreamInterrupted), "LLM stream interrupted unexpectedly");
        assert_eq!(format!("{}", EngineError::MaxTokensReached), "Max tokens reached");
        assert_eq!(format!("{}", EngineError::SessionAlreadyRunning), "Session is already running");
        assert_eq!(format!("{}", EngineError::Cancelled), "Operation cancelled");
    }

    // -- EngineError::storage helper ---------------------------------------

    #[test]
    fn storage_helper_produces_storage_variant_with_message() {
        let e = EngineError::storage("disk full");
        // Variant tag
        assert!(matches!(e, EngineError::Storage(_)));
        // Message survives in the Display
        assert!(format!("{e}").contains("disk full"));
        assert!(format!("{e}").starts_with("Storage error:"));
    }

    // -- From<McpError> -> SkillError -------------------------------------
    //
    // Each arm must map to the *same-named* destination variant. A
    // mis-mapping (e.g. Transport→ToolCall) would label transport
    // problems as tool problems, sending users diagnosing the wrong
    // layer.

    #[test]
    fn from_mcp_server_not_found_maps_to_skill_mcp_server_not_found() {
        let src = alva_protocol_mcp::error::McpError::ServerNotFound("srv".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::McpServerNotFound(s) if s == "srv"));
    }

    #[test]
    fn from_mcp_not_connected_maps_to_skill_mcp_not_connected() {
        let src = alva_protocol_mcp::error::McpError::NotConnected("srv".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::McpNotConnected(s) if s == "srv"));
    }

    #[test]
    fn from_mcp_connect_timeout_maps_to_skill_mcp_connect_timeout() {
        let src = alva_protocol_mcp::error::McpError::ConnectTimeout("srv".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::McpConnectTimeout(s) if s == "srv"));
    }

    #[test]
    fn from_mcp_transport_maps_to_skill_mcp_transport() {
        let src = alva_protocol_mcp::error::McpError::Transport("io".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::McpTransport(s) if s == "io"));
    }

    #[test]
    fn from_mcp_tool_execution_maps_to_skill_mcp_tool_call() {
        // Naming-asymmetry pin: the source variant is "ToolExecution"
        // but the destination variant is "McpToolCall". A naive
        // rename refactor that aligned names would also need to
        // update this arm — pinning catches that drift.
        let src = alva_protocol_mcp::error::McpError::ToolExecution("bad".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::McpToolCall(s) if s == "bad"));
    }

    #[test]
    fn from_mcp_serialization_maps_to_skill_serialization() {
        let src = alva_protocol_mcp::error::McpError::Serialization("json".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::Serialization(s) if s == "json"));
    }

    #[test]
    fn from_mcp_io_maps_to_skill_io() {
        let src = alva_protocol_mcp::error::McpError::Io("file gone".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::Io(s) if s == "file gone"));
    }

    // -- From<protocol_skill::SkillError> -> SkillError -------------------

    #[test]
    fn from_protocol_skill_not_found_maps_through() {
        let src = alva_protocol_skill::error::SkillError::SkillNotFound("alpha".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::SkillNotFound(s) if s == "alpha"));
    }

    #[test]
    fn from_protocol_skill_invalid_md_maps_through() {
        let src = alva_protocol_skill::error::SkillError::InvalidSkillMd("bad".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::InvalidSkillMd(s) if s == "bad"));
    }

    #[test]
    fn from_protocol_skill_path_traversal_maps_through() {
        // Security-relevant: path traversal must NOT silently
        // re-categorize as "InvalidSkillMd" or similar.
        let src = alva_protocol_skill::error::SkillError::PathTraversal("../etc/passwd".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::PathTraversal(s) if s == "../etc/passwd"));
    }

    #[test]
    fn from_protocol_skill_cannot_remove_bundled_maps_through() {
        let src = alva_protocol_skill::error::SkillError::CannotRemoveBundledSkill("autonomous".into());
        let dst: SkillError = src.into();
        assert!(matches!(dst, SkillError::CannotRemoveBundledSkill(s) if s == "autonomous"));
    }
}
