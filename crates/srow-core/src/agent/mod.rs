// INPUT:  (none)
// OUTPUT: pub mod agent_client, persistence, session
// POS:    Module declaration for the Agent core subsystem.
//         orchestrator/ deleted (replaced by alva-graph).
//         runtime/ deleted (security → alva-security, tools → alva-tools, builder → alva-runtime).
//         memory/ deleted (extracted to alva-memory crate).
pub mod agent_client;
pub mod persistence;
pub mod session;
