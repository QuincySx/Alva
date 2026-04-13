// INPUT:  (none)
// OUTPUT: pub mod client, config, elicitation, error, prompts, resources, tool_adapter, transport, transport_sse, types
// POS:    Crate root for alva-protocol-mcp — MCP client library.
//! # alva-protocol-mcp
//!
//! Standalone MCP (Model Context Protocol) client crate.
//!
//! Provides:
//! - **types**: Core protocol types (server config, state, tool info)
//! - **transport**: Abstract transport trait (stdio, SSE)
//! - **transport_sse**: SSE and WebSocket transport configurations
//! - **client**: Multi-server lifecycle manager (connect, disconnect, list/call tools)
//! - **config**: JSON config file reader/writer for `mcpServerConfig.json`
//! - **tool_adapter**: Adapts MCP tools to `alva_kernel_abi::Tool` trait
//! - **resources**: MCP resource types (URIs, content, templates)
//! - **prompts**: MCP prompt templates and rendering
//! - **elicitation**: Server-to-client information requests
//! - **error**: MCP-specific error types

pub mod client;
pub mod config;
pub mod elicitation;
pub mod error;
pub mod prompts;
pub mod resources;
pub mod tool_adapter;
pub mod transport;
pub mod transport_sse;
pub mod types;
