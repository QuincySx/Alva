// INPUT:  (none)
// OUTPUT: pub mod agent, message, session, tool
// POS:    Module declaration for domain entity layer.
//         message.rs and tool.rs kept (stripped of deleted dependencies) because storage/persistence/tools depend on them.
pub mod message;
pub mod session;
pub mod tool;
