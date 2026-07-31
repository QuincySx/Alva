// INPUT:  alva_engine_runtime::RuntimeError, thiserror::Error
// OUTPUT: HubError
// POS:    Product-session and registry errors for the Hub supervisor.

use alva_engine_runtime::RuntimeError;
use thiserror::Error;

/// Errors raised before an engine event stream is established or when routing
/// a product-level operation to its backing engine session.
#[derive(Debug, Error)]
pub enum HubError {
    #[error("engine '{0}' is not registered")]
    EngineNotFound(String),

    #[error("engine '{0}' is already registered")]
    EngineAlreadyRegistered(String),

    #[error("product session '{0}' was not found")]
    SessionNotFound(String),

    #[error("product session '{session_id}' belongs to engine '{actual}', not '{requested}'")]
    EngineMismatch {
        session_id: String,
        requested: String,
        actual: String,
    },

    #[error("product session '{0}' has no engine session yet")]
    SessionNotReady(String),

    #[error("product session '{0}' is already running")]
    SessionBusy(String),

    #[error("product session '{session_id}': {detail}")]
    SessionConflict { session_id: String, detail: String },

    #[error("engine '{0}' does not support session resume")]
    ResumeUnsupported(String),

    #[error("engine '{0}' does not support cancellation")]
    CancelUnsupported(String),

    #[error("engine '{0}' does not support permission callbacks")]
    PermissionUnsupported(String),

    #[error(transparent)]
    Runtime(#[from] RuntimeError),
}
