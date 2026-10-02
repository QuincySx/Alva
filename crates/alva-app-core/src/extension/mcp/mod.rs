// INPUT:  (none)
// OUTPUT: McpPlugin (+ config, runtime, tool_adapter, tools, progressive_tools)
// POS:    MCP protocol integration plugin + Extension impl.
pub mod config;
pub mod runtime;
pub mod tool_adapter;
pub mod tools;
pub mod progressive_tools;

mod extension;
pub use extension::McpPlugin;
