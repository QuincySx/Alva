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

#[cfg(test)]
mod tests {
    //! Tests for AgentError Display interpolation. A dropped `{0}`
    //! placeholder would show users empty / garbled error messages
    //! with no compile-time signal.
    use super::*;

    #[test]
    fn display_llm_error_includes_payload() {
        let e = AgentError::LlmError("rate limit".into());
        assert_eq!(format!("{e}"), "LLM error: rate limit");
    }

    #[test]
    fn display_tool_error_interpolates_both_struct_fields() {
        // Struct variant: BOTH tool_name AND message must appear,
        // separated by ": ". A refactor that loses one field would
        // surface as "Tool error: : the message" or "Tool error: ".
        let e = AgentError::ToolError {
            tool_name: "read_file".into(),
            message: "no such file".into(),
        };
        assert_eq!(format!("{e}"), "Tool error: read_file: no such file");
    }

    #[test]
    fn display_cancelled_is_constant_string() {
        assert_eq!(format!("{}", AgentError::Cancelled), "Cancelled");
    }

    #[test]
    fn display_max_iterations_includes_count() {
        let e = AgentError::MaxIterations(8);
        assert_eq!(format!("{e}"), "Max iterations reached: 8");
    }

    #[test]
    fn display_config_error_includes_payload() {
        let e = AgentError::ConfigError("missing api_key".into());
        assert_eq!(format!("{e}"), "Configuration error: missing api_key");
    }

    #[test]
    fn display_other_has_no_prefix_just_payload() {
        // Pin: `Other` is the "I don't have a specific category"
        // catch-all — Display must show the raw message WITHOUT
        // any "Error: " or "Other: " prefix.
        let e = AgentError::Other("anything goes".into());
        assert_eq!(format!("{e}"), "anything goes");
    }

    #[test]
    fn debug_derive_renders_variant_name() {
        // Smoke pin: Debug works (used in logging + panic messages
        // throughout the codebase).
        let e = AgentError::Cancelled;
        assert_eq!(format!("{e:?}"), "Cancelled");
    }
}
