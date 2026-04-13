//! McpExtension — discovers and exposes tools from MCP servers.

use std::path::PathBuf;
use std::sync::Arc;

use alva_kernel_abi::tool::Tool;
use async_trait::async_trait;

use alva_protocol_mcp::transport::McpTransport;
use alva_protocol_mcp::error::McpError;
use alva_protocol_mcp::types::McpToolInfo;

use crate::extension::Extension;
use crate::extension::mcp::config::McpConfig;
use crate::extension::mcp::runtime::{McpManager, McpTransportFactory};
use crate::extension::mcp::tool_adapter::build_mcp_tools;
use crate::extension::mcp::tools::McpRuntimeTool;

/// Stub transport factory used when no real MCP transport implementation is
/// available.  Creates transports that immediately fail on connect, so the
/// extension degrades gracefully (tools from unreachable servers are simply
/// omitted).
struct StubTransportFactory;

impl McpTransportFactory for StubTransportFactory {
    fn create(
        &self,
        _config: &alva_protocol_mcp::types::McpServerConfig,
    ) -> Box<dyn McpTransport> {
        Box::new(StubTransport)
    }
}

/// A transport that always fails — used as a placeholder until real stdio/SSE
/// transports are wired in.
struct StubTransport;

#[async_trait]
impl McpTransport for StubTransport {
    async fn connect(&mut self) -> Result<(), McpError> {
        Err(McpError::Transport(
            "no real MCP transport implementation available yet".into(),
        ))
    }
    async fn disconnect(&mut self) -> Result<(), McpError> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        false
    }
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        Ok(vec![])
    }
    async fn call_tool(
        &self,
        _tool_name: &str,
        _arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        Err(McpError::NotConnected("stub transport".into()))
    }
}

/// MCP server integration — discovers and exposes tools from MCP servers.
///
/// During `tools()`, the extension:
/// 1. Loads MCP config from the given paths (global + project `mcp.json`).
/// 2. Creates an [`McpManager`], registers servers, and auto-connects.
/// 3. Wraps discovered MCP tools as standard `Tool` trait objects via
///    [`McpToolAdapter`](crate::extension::mcp::tool_adapter::McpToolAdapter).
/// 4. Provides an [`McpRuntimeTool`] for runtime server management.
///
/// All errors are caught and logged — MCP failures never prevent agent startup.
pub struct McpExtension {
    config_paths: Vec<PathBuf>,
}

impl McpExtension {
    /// Create a new MCP extension that will load config from the given paths.
    ///
    /// Typically called with `[paths.global_mcp_config(), paths.project_mcp_config()]`.
    pub fn new(config_paths: Vec<PathBuf>) -> Self {
        Self { config_paths }
    }
}

#[async_trait]
impl Extension for McpExtension {
    fn name(&self) -> &str {
        "mcp"
    }

    fn description(&self) -> &str {
        "MCP server integration"
    }

    async fn tools(&self) -> Vec<Box<dyn Tool>> {
        match self.load_and_connect().await {
            Ok(tools) => tools,
            Err(e) => {
                tracing::warn!("MCP extension failed to initialise: {e}");
                vec![]
            }
        }
    }
}

impl McpExtension {
    /// Internal helper: load config, create manager, connect, discover tools.
    async fn load_and_connect(&self) -> Result<Vec<Box<dyn Tool>>, Box<dyn std::error::Error + Send + Sync>> {
        // 1. Load and merge configs from all paths (later paths override earlier).
        let mut merged = McpConfig::default();
        for path in &self.config_paths {
            let cfg = McpConfig::load(path).await?;
            for (id, entry) in cfg.servers {
                merged.servers.insert(id, entry);
            }
        }

        if merged.servers.is_empty() {
            tracing::debug!("MCP: no servers configured — skipping");
            return Ok(vec![]);
        }

        tracing::info!("MCP: {} server(s) configured", merged.servers.len());

        // 2. Create manager with stub factory (will be replaced with real
        //    transport implementations later).
        let factory: Arc<dyn McpTransportFactory> = Arc::new(StubTransportFactory);
        let manager = Arc::new(McpManager::new(factory));

        // 3. Register all servers.
        let server_configs = merged.to_server_configs();
        for cfg in &server_configs {
            manager.register(cfg.clone()).await;
        }

        // 4. Auto-connect servers that have auto_connect = true.
        let errors = manager.connect_auto().await;
        for (id, err) in &errors {
            tracing::warn!("MCP: server '{id}' auto-connect failed: {err}");
        }

        // 5. Discover tools from connected servers.
        let tool_infos = manager.list_all_tools().await;
        tracing::info!("MCP: discovered {} tool(s) from connected servers", tool_infos.len());

        // 6. Wrap MCP tools as standard Tool trait objects.
        let mut tools = build_mcp_tools(manager.clone(), tool_infos);

        // 7. Add the MCP runtime meta-tool for server management.
        tools.push(Box::new(McpRuntimeTool {
            manager: manager.clone(),
        }));

        Ok(tools)
    }
}
