// INPUT:  std::collections, std::sync, tokio::sync, alva_protocol_mcp, crate::error
// OUTPUT: McpTransportFactory (trait), McpManager
// POS:    MCP Server lifecycle manager — handles registration, connection, disconnection, tool enumeration, and tool invocation.
use serde_json::Value;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::SkillError;
use alva_protocol_mcp::error::McpError;
use alva_protocol_mcp::transport::McpTransport;
use alva_protocol_mcp::types::{McpServerConfig, McpServerState, McpToolInfo};

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

/// A factory whose transports never connect — used to build a manager that
/// can hold/register server configs but establishes no live sessions (e.g.
/// for template instantiation where no shared MCP runtime is available).
struct DisconnectedTransportFactory;

impl McpTransportFactory for DisconnectedTransportFactory {
    fn create(&self, _config: &McpServerConfig) -> Box<dyn McpTransport> {
        Box::new(DisconnectedTransport)
    }
}

struct DisconnectedTransport;

#[async_trait::async_trait]
impl McpTransport for DisconnectedTransport {
    async fn connect(&mut self) -> Result<(), McpError> {
        Err(McpError::Transport("no MCP runtime available".into()))
    }
    async fn disconnect(&mut self) -> Result<(), McpError> {
        Ok(())
    }
    fn is_connected(&self) -> bool {
        false
    }
    async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
        Ok(Vec::new())
    }
    async fn call_tool(&self, _tool_name: &str, _arguments: Value) -> Result<Value, McpError> {
        Err(McpError::Transport("no MCP runtime available".into()))
    }
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

    /// A manager backed by a transport that never connects. Lets callers
    /// build services that require an `McpManager` (e.g.
    /// `AgentTemplateService`) without a live MCP runtime — registered
    /// servers simply expose no tools.
    pub fn disconnected() -> Self {
        Self::new(Arc::new(DisconnectedTransportFactory))
    }

    /// Register MCP Server config (does not connect immediately)
    pub async fn register(&self, config: McpServerConfig) {
        let mut servers = self.servers.write().await;
        if servers
            .get(&config.id)
            .is_some_and(|instance| instance.config == config)
        {
            return;
        }
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
    #[tracing::instrument(name = "mcp_connect", skip(self), fields(server_id = %server_id))]
    pub async fn connect(&self, server_id: &str) -> Result<(), SkillError> {
        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_id)
            .ok_or_else(|| SkillError::McpServerNotFound(server_id.to_string()))?;

        if matches!(instance.state, McpServerState::Connected { .. })
            && instance
                .transport
                .as_ref()
                .is_some_and(|t| t.is_connected())
        {
            return Ok(()); // Idempotent
        }

        instance.transport = None;
        instance.tools.clear();
        // The map write lock hides the in-progress attempt. Cancellation must
        // leave no stale Connecting state after dropping the new transport.
        instance.state = McpServerState::Disconnected;

        let mut transport = self.factory.create(&instance.config);
        let connect_result = tokio::time::timeout(
            std::time::Duration::from_secs(instance.config.connect_timeout_secs as u64),
            async {
                transport.connect().await?;
                transport.list_tools().await
            },
        )
        .await
        .map_err(|_| SkillError::McpConnectTimeout(server_id.to_string()))
        .and_then(|r| r.map_err(SkillError::from));

        match connect_result {
            Ok(tools) => {
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
            .filter(|inst| inst.transport.as_ref().is_some_and(|t| t.is_connected()))
            .flat_map(|inst| inst.tools.clone())
            .collect()
    }

    /// Call MCP tool
    #[tracing::instrument(name = "mcp_call_tool", skip(self, arguments), fields(server_id = %server_id, tool_name = %tool_name))]
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
        if !transport.is_connected() {
            return Err(SkillError::McpNotConnected(server_id.to_string()));
        }

        transport
            .call_tool(tool_name, arguments)
            .await
            .map_err(SkillError::from)
    }

    /// Get state snapshot of all Servers
    pub async fn server_states(&self) -> HashMap<String, McpServerState> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(id, inst)| {
                let state = if matches!(inst.state, McpServerState::Connected { .. })
                    && !inst.transport.as_ref().is_some_and(|t| t.is_connected())
                {
                    McpServerState::Failed {
                        reason: "MCP transport closed; start a new run to reconnect".into(),
                    }
                } else {
                    inst.state.clone()
                };
                (id.clone(), state)
            })
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

    struct FixtureFactory {
        connected: Arc<AtomicBool>,
        fail_list: Arc<AtomicBool>,
        stall_list: Arc<AtomicBool>,
        starts: Arc<AtomicUsize>,
    }
    struct FixtureTransport {
        connected: Arc<AtomicBool>,
        fail_list: Arc<AtomicBool>,
        stall_list: Arc<AtomicBool>,
    }
    impl McpTransportFactory for FixtureFactory {
        fn create(&self, _: &McpServerConfig) -> Box<dyn McpTransport> {
            self.starts.fetch_add(1, Ordering::SeqCst);
            Box::new(FixtureTransport {
                connected: self.connected.clone(),
                fail_list: self.fail_list.clone(),
                stall_list: self.stall_list.clone(),
            })
        }
    }
    #[async_trait::async_trait]
    impl McpTransport for FixtureTransport {
        async fn connect(&mut self) -> Result<(), McpError> {
            self.connected.store(true, Ordering::SeqCst);
            Ok(())
        }
        async fn disconnect(&mut self) -> Result<(), McpError> {
            self.connected.store(false, Ordering::SeqCst);
            Ok(())
        }
        fn is_connected(&self) -> bool {
            self.connected.load(Ordering::SeqCst)
        }
        async fn list_tools(&self) -> Result<Vec<McpToolInfo>, McpError> {
            if self.stall_list.load(Ordering::SeqCst) {
                return std::future::pending().await;
            }
            if self.fail_list.load(Ordering::SeqCst) {
                return Err(McpError::Transport("Fixture catalog failure".into()));
            }
            Ok(vec![McpToolInfo {
                server_id: "fixture".into(),
                tool_name: "echo".into(),
                description: "Fixture".into(),
                input_schema: serde_json::json!({"type":"object"}),
            }])
        }
        async fn call_tool(&self, _: &str, arguments: Value) -> Result<Value, McpError> {
            Ok(arguments)
        }
    }

    #[tokio::test]
    async fn closed_transport_reconnects_and_failed_discovery_clears_old_tools() {
        let connected = Arc::new(AtomicBool::new(false));
        let fail_list = Arc::new(AtomicBool::new(false));
        let starts = Arc::new(AtomicUsize::new(0));
        let manager = McpManager::new(Arc::new(FixtureFactory {
            connected: connected.clone(),
            fail_list: fail_list.clone(),
            stall_list: Arc::new(AtomicBool::new(false)),
            starts: starts.clone(),
        }));
        manager
            .register(McpServerConfig {
                id: "fixture".into(),
                display_name: "Fixture".into(),
                transport: alva_protocol_mcp::types::McpTransportConfig::Stdio {
                    command: "/not-executed".into(),
                    args: vec![],
                    env: Default::default(),
                },
                auto_connect: true,
                connect_timeout_secs: 1,
            })
            .await;
        manager.connect("fixture").await.unwrap();
        manager.connect("fixture").await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(manager.list_all_tools().await.len(), 1);
        connected.store(false, Ordering::SeqCst);
        assert!(manager.list_all_tools().await.is_empty());
        assert!(matches!(
            manager.server_states().await["fixture"],
            McpServerState::Failed { .. }
        ));
        assert!(matches!(
            manager
                .call_tool("fixture", "echo", serde_json::json!({}))
                .await,
            Err(SkillError::McpNotConnected(_))
        ));
        fail_list.store(true, Ordering::SeqCst);
        assert!(manager.connect("fixture").await.is_err());
        assert_eq!(starts.load(Ordering::SeqCst), 2);
        assert!(manager.list_all_tools().await.is_empty());
        assert!(matches!(
            manager.server_states().await["fixture"],
            McpServerState::Failed { .. }
        ));
        fail_list.store(false, Ordering::SeqCst);
        manager.connect("fixture").await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 3);
        assert_eq!(manager.list_all_tools().await.len(), 1);
        manager.disconnect("fixture").await.unwrap();
        assert!(manager.list_all_tools().await.is_empty());
    }

    #[tokio::test]
    async fn regression_same_config_registration_preserves_live_connection() {
        let starts = Arc::new(AtomicUsize::new(0));
        let manager = McpManager::new(Arc::new(FixtureFactory {
            connected: Arc::new(AtomicBool::new(false)),
            fail_list: Arc::new(AtomicBool::new(false)),
            stall_list: Arc::new(AtomicBool::new(false)),
            starts: starts.clone(),
        }));
        let config = McpServerConfig {
            id: "fixture".into(),
            display_name: "Fixture".into(),
            transport: alva_protocol_mcp::types::McpTransportConfig::Stdio {
                command: "/not-executed".into(),
                args: vec![],
                env: Default::default(),
            },
            auto_connect: true,
            connect_timeout_secs: 1,
        };
        manager.register(config.clone()).await;
        manager.connect("fixture").await.unwrap();
        manager.register(config.clone()).await;
        assert_eq!(
            manager.server_states().await["fixture"],
            McpServerState::Connected { tool_count: 1 }
        );
        manager.connect("fixture").await.unwrap();
        assert_eq!(starts.load(Ordering::SeqCst), 1);
        assert_eq!(manager.list_all_tools().await.len(), 1);
        let mut changed = config;
        changed.connect_timeout_secs += 1;
        manager.register(changed).await;
        assert_eq!(
            manager.server_states().await["fixture"],
            McpServerState::Disconnected
        );
        assert!(manager.list_all_tools().await.is_empty());
    }

    #[tokio::test]
    async fn regression_cancelled_discovery_does_not_leave_connecting() {
        let manager = McpManager::new(Arc::new(FixtureFactory {
            connected: Arc::new(AtomicBool::new(false)),
            fail_list: Arc::new(AtomicBool::new(false)),
            stall_list: Arc::new(AtomicBool::new(true)),
            starts: Arc::new(AtomicUsize::new(0)),
        }));
        manager
            .register(McpServerConfig {
                id: "fixture".into(),
                display_name: "Fixture".into(),
                transport: alva_protocol_mcp::types::McpTransportConfig::Stdio {
                    command: "/not-executed".into(),
                    args: vec![],
                    env: Default::default(),
                },
                auto_connect: true,
                connect_timeout_secs: 1,
            })
            .await;
        assert!(tokio::time::timeout(
            std::time::Duration::from_millis(50),
            manager.connect("fixture")
        )
        .await
        .is_err());
        assert_eq!(
            manager.server_states().await["fixture"],
            McpServerState::Disconnected
        );
        assert!(manager.list_all_tools().await.is_empty());
    }

    #[tokio::test]
    async fn disconnected_manager_constructs_and_exposes_no_tools() {
        let mgr = McpManager::disconnected();
        // No servers registered → no tools, and the no-op transport keeps it
        // from ever connecting to anything real.
        assert!(mgr.list_all_tools().await.is_empty());
    }
}
