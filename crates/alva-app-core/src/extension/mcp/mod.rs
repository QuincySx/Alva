// INPUT:  (none)
// OUTPUT: McpExtension (+ submodules: config, runtime, tool_adapter, tools)
// POS:    MCP protocol integration plugin + Extension impl.
pub mod config;
pub mod runtime;
pub mod tool_adapter;
pub mod tools;

mod extension;
pub use extension::McpExtension;
