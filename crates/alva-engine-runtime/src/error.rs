use thiserror::Error;

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("Engine not ready: {0}")]
    NotReady(String),

    #[error("Session not found: {0}")]
    SessionNotFound(String),

    #[error("Permission request not found: {0}")]
    PermissionNotFound(String),

    #[error("Process error: {0}")]
    ProcessError(String),

    #[error("Protocol error: {0}")]
    ProtocolError(String),

    #[error("Cancelled")]
    Cancelled,

    #[error("{0}")]
    Other(String),
}

impl From<std::io::Error> for RuntimeError {
    fn from(e: std::io::Error) -> Self {
        RuntimeError::ProcessError(e.to_string())
    }
}

impl From<serde_json::Error> for RuntimeError {
    fn from(e: serde_json::Error) -> Self {
        RuntimeError::ProtocolError(e.to_string())
    }
}
