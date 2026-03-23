// INPUT:  (none)
// OUTPUT: pub mod agent_client, persistence, session
// POS:    Module declaration for the Agent core subsystem.
//         orchestrator/ deleted (replaced by agent-graph).
//         runtime/ deleted (security → agent-security, tools → agent-tools, builder → agent-runtime).
//         memory/ deleted (extracted to agent-memory crate).
pub mod agent_client;
pub mod persistence;
pub mod session;
