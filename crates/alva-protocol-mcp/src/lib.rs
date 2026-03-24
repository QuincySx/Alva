// INPUT:  (none)
// OUTPUT: pub mod client, config, error, tool_adapter, transport, types
// POS:    Crate root for alva-protocol-mcp — MCP client library.
//! # alva-protocol-mcp
//!
//! Standalone MCP (Model Context Protocol) client crate.
//!
//! Provides:
//! - **types**: Core protocol types (server config, state, tool info)
//! - **transport**: Abstract transport trait (stdio, SSE)
//! - **client**: Multi-server lifecycle manager (connect, disconnect, list/call tools)
//! - **config**: JSON config file reader/writer for `mcpServerConfig.json`
//! - **tool_adapter**: Adapts MCP tools to `alva_types::Tool` trait
//! - **error**: MCP-specific error types

pub mod client;
pub mod config;
pub mod error;
pub mod tool_adapter;
pub mod transport;
pub mod types;
