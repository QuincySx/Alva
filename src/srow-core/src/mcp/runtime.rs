use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::{
    skills::skill_domain::mcp::{McpServerConfig, McpServerState, McpToolInfo},
    error::SkillError,
    skills::skill_ports::mcp_transport::McpTransport,
};

/// MCP Server runtime instance (in-memory)
struct McpServerInstance {
    config: McpServerConfig,
    state: McpServerState,
    /// Transport layer instance (held after connection established)
    transport: Option<Box<dyn McpTransport>>,
    /// Tool list for this Server (populated after connected)
    tools: Vec<McpToolInfo>,
}

/// Factory trait: creates transport layer based on McpTransportConfig
pub trait McpTransportFactory: Send + Sync {
    fn create(&self, config: &McpServerConfig) -> Box<dyn McpTransport>;
}

/// MCP Server lifecycle manager
/// Manages connection state and tool lists for all MCP Servers
pub struct McpManager {
    factory: Arc<dyn McpTransportFactory>,
    /// server_id -> instance
    servers: Arc<RwLock<HashMap<String, McpServerInstance>>>,
}

impl McpManager {
    pub fn new(factory: Arc<dyn McpTransportFactory>) -> Self {
        Self {
            factory,
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register MCP Server config (does not connect immediately)
    pub async fn register(&self, config: McpServerConfig) {
        let mut servers = self.servers.write().await;
        servers.insert(
            config.id.clone(),
            McpServerInstance {
                config,
                state: McpServerState::Disconnected,
                transport: None,
                tools: vec![],
            },
        );
    }

    /// Connect to specified Server (establish transport, handshake, enumerate tools)
    pub async fn connect(&self, server_id: &str) -> Result<(), SkillError> {
        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_id)
            .ok_or_else(|| SkillError::McpServerNotFound(server_id.to_string()))?;

        if matches!(instance.state, McpServerState::Connected { .. }) {
            return Ok(()); // Idempotent
        }

        instance.state = McpServerState::Connecting;

        let mut transport = self.factory.create(&instance.config);
        let connect_result = tokio::time::timeout(
            std::time::Duration::from_secs(instance.config.connect_timeout_secs as u64),
            transport.connect(),
        )
        .await
        .map_err(|_| SkillError::McpConnectTimeout(server_id.to_string()))
        .and_then(|r| r);

        match connect_result {
            Ok(()) => {
                let tools = transport.list_tools().await?;
                let tool_count = tools.len();
                instance.tools = tools;
                instance.state = McpServerState::Connected { tool_count };
                instance.transport = Some(transport);
                Ok(())
            }
            Err(e) => {
                instance.state = McpServerState::Failed {
                    reason: e.to_string(),
                };
                Err(e)
            }
        }
    }

    /// Disconnect specified Server
    pub async fn disconnect(&self, server_id: &str) -> Result<(), SkillError> {
        let mut servers = self.servers.write().await;
        if let Some(instance) = servers.get_mut(server_id) {
            if let Some(transport) = instance.transport.as_mut() {
                let _ = transport.disconnect().await;
            }
            instance.transport = None;
            instance.state = McpServerState::Disconnected;
            instance.tools.clear();
        }
        Ok(())
    }

    /// List all tools from all connected Servers
    pub async fn list_all_tools(&self) -> Vec<McpToolInfo> {
        self.servers
            .read()
            .await
            .values()
            .flat_map(|inst| inst.tools.clone())
            .collect()
    }

    /// Call MCP tool
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, SkillError> {
        let servers = self.servers.read().await;
        let instance = servers
            .get(server_id)
            .ok_or_else(|| SkillError::McpServerNotFound(server_id.to_string()))?;

        let transport = instance
            .transport
            .as_ref()
            .ok_or_else(|| SkillError::McpNotConnected(server_id.to_string()))?;

        transport.call_tool(tool_name, arguments).await
    }

    /// Get state snapshot of all Servers
    pub async fn server_states(&self) -> HashMap<String, McpServerState> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(id, inst)| (id.clone(), inst.state.clone()))
            .collect()
    }

    /// Connect all Servers with auto_connect = true
    pub async fn connect_auto(&self) -> Vec<(String, SkillError)> {
        let server_ids: Vec<String> = {
            self.servers
                .read()
                .await
                .values()
                .filter(|inst| inst.config.auto_connect)
                .map(|inst| inst.config.id.clone())
                .collect()
        };

        let mut errors = vec![];
        for id in server_ids {
            if let Err(e) = self.connect(&id).await {
                tracing::warn!("MCP Server '{}' auto-connect failed: {}", id, e);
                errors.push((id, e));
            }
        }
        errors
    }
}
