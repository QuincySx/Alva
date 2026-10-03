// INPUT:  (none)
// OUTPUT: McpPlugin, optional I/O observer/error contracts and narrow child MCP rebinding
// POS:    MCP protocol integration plugin + Extension impl.
pub mod config;
pub mod runtime;
pub mod tool_adapter;
pub mod tools;
pub mod progressive_tools;
mod worker;
mod spawn;
pub use spawn::{McpGatewayBinding, McpSpawnPolicy};
pub use alva_protocol_mcp::{transport::McpIoObserver, error::McpError as McpInteractionError};

mod extension;
pub use extension::McpPlugin;
