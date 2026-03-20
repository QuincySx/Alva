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
