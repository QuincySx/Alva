#![allow(dead_code)]
// INPUT:  rusqlite, alva_kernel_abi::agent_session
// OUTPUT: SqliteEvalSession, SqliteEvalSessionManager, SessionSummary, StoredModel, StoredWorkspace
// POS:    Session persistence layer — SQLite storage for sessions, models, workspaces.

pub mod manager;
pub mod registry;
pub mod schema;
#[allow(clippy::module_inception)] // Preserve the established sqlite_session::sqlite_session path.
pub mod sqlite_session;

#[allow(unused_imports)]
pub use manager::{SessionSummary, SqliteEvalSessionManager, StoredWorkspace};
#[allow(unused_imports)]
pub use registry::SqliteSessionRegistry;
#[allow(unused_imports)]
pub use sqlite_session::SqliteEvalSession;
