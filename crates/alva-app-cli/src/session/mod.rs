// INPUT:  json_file_session, manager
// OUTPUT: JsonFileAgentSession, JsonFileSessionManager, SessionSummary
// POS:    CLI-private AgentSession backend — stores each session as a JSON file
//         under .alva/sessions/{session_id}.json, with an index.json for fast
//         listing. Replaces the legacy session_store module.

pub mod json_file_session;
pub mod manager;

pub use json_file_session::JsonFileAgentSession;
pub use manager::{JsonFileSessionManager, SessionSummary};
