// INPUT:  std::io, thiserror
// OUTPUT: pub enum SrowError
// POS:    Defines the unified application error type with NotFound, InvalidInput, Engine, Io, and Internal variants.
#[derive(Debug, thiserror::Error)]
pub enum SrowError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("engine error: {0}")]
    Engine(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("internal error: {0}")]
    Internal(String),
}
