// INPUT:  rusqlite, alva_kernel_abi::agent_session
// OUTPUT: SqliteEvalSession, SqliteEvalSessionManager, SessionSummary, StoredModel, StoredWorkspace
// POS:    Session persistence layer — SQLite storage for sessions, models, workspaces.

pub mod manager;
pub mod registry;
pub mod schema;
pub mod sqlite_session;

pub use manager::{SessionSummary, SqliteEvalSessionManager, StoredWorkspace};
pub use registry::SqliteSessionRegistry;
#[allow(unused_imports)]
pub use sqlite_session::SqliteEvalSession;
