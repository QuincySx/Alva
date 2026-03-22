// INPUT:  (none)
// OUTPUT: pub mod provider, storage, tool
// POS:    Module declaration for the port (interface) layer.
//         tool.rs kept because runtime/tools/ depends on Tool trait/ToolRegistry/ToolContext.
pub mod provider;
pub mod storage;
pub mod tool;
