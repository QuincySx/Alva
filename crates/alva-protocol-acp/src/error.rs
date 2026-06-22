// INPUT:  thiserror
// OUTPUT: AcpError
// POS:    Unified error enum for all ACP protocol operations (spawn, permission, IO, serialization)
use thiserror::Error;

#[derive(Debug, Error)]
pub enum AcpError {
    #[error("Agent CLI not found: {kind} -- {hint}")]
    AgentNotFound { kind: String, hint: String },

    #[error("Failed to spawn agent process '{agent}': {reason}")]
    SpawnFailed { agent: String, reason: String },

    #[error("Process {pid} is no longer alive")]
    ProcessDead { pid: u32 },

    #[error("Process '{0}' not found in manager")]
    ProcessNotFound(String),

    #[error("Permission request '{0}' not found (already resolved or expired)")]
    PermissionRequestNotFound(String),

    #[error("Invalid ACP configuration: {0}")]
    InvalidConfig(String),

    #[error("Serialization error: {0}")]
    Serialization(String),

    #[error("I/O error: {0}")]
    Io(String),

    #[error("Storage error: {0}")]
    Storage(String),

    #[error("Protocol error: {0}")]
    Protocol(String),
}

#[cfg(test)]
mod tests {
    //! Tests for AcpError Display interpolation.
    //!
    //! 10 variants — 3 struct variants (AgentNotFound / SpawnFailed /
    //! ProcessDead) plus 7 single-payload. Pinning each prevents
    //! dropped-placeholder bugs: a missing `{kind}` or `{agent}` in
    //! a struct variant surfaces as "Agent CLI not found:  --" type
    //! garbage to users with no compile-time hint.
    use super::*;

    // -- Struct variants: multiple named fields ---------------------------

    #[test]
    fn agent_not_found_interpolates_kind_and_hint() {
        let e = AcpError::AgentNotFound {
            kind: "claude".into(),
            hint: "install with: npm i -g @anthropic-ai/claude-code".into(),
        };
        assert_eq!(
            e.to_string(),
            "Agent CLI not found: claude -- install with: npm i -g @anthropic-ai/claude-code"
        );
    }

    #[test]
    fn spawn_failed_interpolates_agent_and_reason() {
        let e = AcpError::SpawnFailed {
            agent: "claude".into(),
            reason: "permission denied (os error 13)".into(),
        };
        assert_eq!(
            e.to_string(),
            "Failed to spawn agent process 'claude': permission denied (os error 13)"
        );
    }

    #[test]
    fn process_dead_interpolates_pid() {
        let e = AcpError::ProcessDead { pid: 12345 };
        assert_eq!(e.to_string(), "Process 12345 is no longer alive");
    }

    // -- Single-payload variants (positional {0}) -------------------------

    #[test]
    fn process_not_found_quotes_id() {
        let e = AcpError::ProcessNotFound("proc-1".into());
        assert_eq!(e.to_string(), "Process 'proc-1' not found in manager");
    }

    #[test]
    fn permission_request_not_found_quotes_id_and_explains_state() {
        // Pin the full message — "already resolved or expired" is a
        // user diagnostic hint, dropping it would leave users guessing.
        let e = AcpError::PermissionRequestNotFound("req-42".into());
        assert_eq!(
            e.to_string(),
            "Permission request 'req-42' not found (already resolved or expired)"
        );
    }

    #[test]
    fn invalid_config_includes_payload() {
        let e = AcpError::InvalidConfig("missing required field 'name'".into());
        assert_eq!(
            e.to_string(),
            "Invalid ACP configuration: missing required field 'name'"
        );
    }

    #[test]
    fn serialization_includes_payload() {
        let e = AcpError::Serialization("expected ',' at line 2".into());
        assert_eq!(e.to_string(), "Serialization error: expected ',' at line 2");
    }

    #[test]
    fn io_includes_payload() {
        let e = AcpError::Io("broken pipe".into());
        assert_eq!(e.to_string(), "I/O error: broken pipe");
    }

    #[test]
    fn storage_includes_payload() {
        let e = AcpError::Storage("sqlite: disk i/o error".into());
        assert_eq!(e.to_string(), "Storage error: sqlite: disk i/o error");
    }

    #[test]
    fn protocol_includes_payload() {
        let e = AcpError::Protocol("unexpected close frame".into());
        assert_eq!(e.to_string(), "Protocol error: unexpected close frame");
    }

    // -- Debug derive smoke -----------------------------------------------

    #[test]
    fn all_variants_implement_debug() {
        let variants = vec![
            AcpError::AgentNotFound {
                kind: "k".into(),
                hint: "h".into(),
            },
            AcpError::SpawnFailed {
                agent: "a".into(),
                reason: "r".into(),
            },
            AcpError::ProcessDead { pid: 1 },
            AcpError::ProcessNotFound("p".into()),
            AcpError::PermissionRequestNotFound("p".into()),
            AcpError::InvalidConfig("c".into()),
            AcpError::Serialization("s".into()),
            AcpError::Io("i".into()),
            AcpError::Storage("s".into()),
            AcpError::Protocol("p".into()),
        ];
        for v in &variants {
            assert!(!format!("{v:?}").is_empty());
        }
    }
}
