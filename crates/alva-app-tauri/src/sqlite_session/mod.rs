// INPUT:  rusqlite, alva_kernel_abi::agent_session
// OUTPUT: SqliteEvalSession, SqliteEvalSessionManager, SessionSummary, StoredModel, StoredWorkspace
// POS:    Session persistence layer — SQLite storage for sessions, models, workspaces.

pub mod manager;
pub mod schema;
pub mod sqlite_session;

pub use manager::{SessionSummary, SqliteEvalSessionManager, StoredModel, StoredWorkspace};
#[allow(unused_imports)]
pub use sqlite_session::SqliteEvalSession;
