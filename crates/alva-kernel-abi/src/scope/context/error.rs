// POS: Context error types.

/// Error type for context plugin operations.
#[derive(Debug, thiserror::Error)]
pub enum ContextError {
    #[error("context error: {0}")]
    Other(String),
}
