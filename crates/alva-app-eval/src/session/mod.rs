// INPUT:  rusqlite, alva_kernel_abi::agent_session
// OUTPUT: SqliteEvalSession, SqliteEvalSessionManager, StoredRunSummary, schema
// POS:    Eval-private session layer — SQLite persistence for eval runs.

pub mod manager;
pub mod schema;
pub mod sqlite_session;

pub use manager::{SqliteEvalSessionManager, StoredRunSummary};
#[allow(unused_imports)]
pub use sqlite_session::SqliteEvalSession;
