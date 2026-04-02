// INPUT:  std::collections, std::sync, tokio::sync, crate::types, crate::error, crate::transport
// OUTPUT: McpTransportFactory (trait), McpClient
// POS:    MCP Server lifecycle manager — handles registration, connection, disconnection, tool enumeration, and tool invocation.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::error::McpError;
use crate::transport::McpTransport;
use crate::types::{McpServerConfig, McpServerState, McpToolInfo};

/// MCP Server runtime instance (in-memory).
struct McpServerInstance {
    config: McpServerConfig,
    state: McpServerState,
    /// Transport layer instance (held after connection established).
    /// Wrapped in Arc<tokio::sync::Mutex> so tool calls can execute without
    /// holding the outer RwLock on the servers map.
    transport: Option<Arc<tokio::sync::Mutex<Box<dyn McpTransport>>>>,
    /// Tool list for this Server (populated after connected)
    tools: Vec<McpToolInfo>,
}

/// Factory trait: creates transport layer based on McpServerConfig.
pub trait McpTransportFactory: Send + Sync {
    fn create(&self, config: &McpServerConfig) -> Box<dyn McpTransport>;
}

/// MCP client — manages connections to one or more MCP servers.
///
/// Handles server lifecycle: registration, connection, disconnection,
/// tool enumeration, and tool invocation.
pub struct McpClient {
    factory: Arc<dyn McpTransportFactory>,
    /// server_id -> instance
    servers: Arc<RwLock<HashMap<String, McpServerInstance>>>,
}

impl McpClient {
    pub fn new(factory: Arc<dyn McpTransportFactory>) -> Self {
        Self {
            factory,
            servers: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Register MCP Server config (does not connect immediately).
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

    /// Connect to specified Server (establish transport, handshake, enumerate tools).
    #[tracing::instrument(name = "mcp_connect", skip(self), fields(server_id = %server_id))]
    pub async fn connect(&self, server_id: &str) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        let instance = servers
            .get_mut(server_id)
            .ok_or_else(|| McpError::ServerNotFound(server_id.to_string()))?;

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
        .map_err(|_| McpError::ConnectTimeout(server_id.to_string()))
        .and_then(|r| r);

        match connect_result {
            Ok(()) => {
                let tools = transport.list_tools().await?;
                let tool_count = tools.len();
                instance.tools = tools;
                instance.state = McpServerState::Connected { tool_count };
                instance.transport = Some(Arc::new(tokio::sync::Mutex::new(transport)));
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

    /// Disconnect specified Server.
    pub async fn disconnect(&self, server_id: &str) -> Result<(), McpError> {
        let mut servers = self.servers.write().await;
        if let Some(instance) = servers.get_mut(server_id) {
            if let Some(transport) = instance.transport.take() {
                let mut t = transport.lock().await;
                let _ = t.disconnect().await;
            }
            instance.state = McpServerState::Disconnected;
            instance.tools.clear();
        }
        Ok(())
    }

    /// List all tools from all connected Servers.
    pub async fn list_all_tools(&self) -> Vec<McpToolInfo> {
        self.servers
            .read()
            .await
            .values()
            .flat_map(|inst| inst.tools.clone())
            .collect()
    }

    /// Call MCP tool.
    ///
    /// Clones the Arc<Mutex<transport>> under a brief read lock, then releases
    /// the servers map lock before executing the (potentially slow) tool call.
    #[tracing::instrument(name = "mcp_call_tool", skip(self, arguments), fields(server_id = %server_id, tool_name = %tool_name))]
    pub async fn call_tool(
        &self,
        server_id: &str,
        tool_name: &str,
        arguments: serde_json::Value,
    ) -> Result<serde_json::Value, McpError> {
        // Brief read lock: clone the Arc, then release the servers map.
        let transport = {
            let servers = self.servers.read().await;
            let instance = servers
                .get(server_id)
                .ok_or_else(|| McpError::ServerNotFound(server_id.to_string()))?;

            instance
                .transport
                .as_ref()
                .ok_or_else(|| McpError::NotConnected(server_id.to_string()))?
                .clone()
        };
        // servers read lock released here.

        let t = transport.lock().await;
        t.call_tool(tool_name, arguments).await
    }

    /// Get state snapshot of all Servers.
    pub async fn server_states(&self) -> HashMap<String, McpServerState> {
        self.servers
            .read()
            .await
            .iter()
            .map(|(id, inst)| (id.clone(), inst.state.clone()))
            .collect()
    }

    /// Connect all Servers with auto_connect = true.
    pub async fn connect_auto(&self) -> Vec<(String, McpError)> {
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
    use crate::transport::McpTransport;
    use async_trait::async_trait;
    use serde_json::Value;
    use std::sync::atomic::{AtomicBool, Ordering};

    // ── Mock transport ──────────────────────────────────────────────────

    struct MockTransport {
        connected: AtomicBool,
        tools: Vec<McpToolInfo>,
        fail_connect: bool,
    }

    impl MockTransport {
        fn new(tools: Vec<McpToolInfo>) -> Self {
            Self {
                connected: AtomicBool::new(false),
                tools,
                fail_connect: false,
            }
        }

        fn failing() -> Self {
            Self {
                connected: AtomicBool::new(false),
                tools: vec![],
                fail_connect: true,
            }
        }
    }

    #[async_trait]
    impl McpTransport for MockTransport {
        async fn connect(&mut self) -> Result<(), McpError> {
            if self.fail_connect {
                return Err(McpError::Transport("mock connection failure".into()));
            }
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
            Ok(self.tools.clone())
        }

        async fn call_tool(
            &self,
            tool_name: &str,
            arguments: Value,
        ) -> Result<Value, McpError> {
            Ok(serde_json::json!({
                "tool": tool_name,
                "args": arguments,
                "result": "ok"
            }))
        }
    }

    // ── Mock factory ────────────────────────────────────────────────────

    struct MockFactory {
        tools: Vec<McpToolInfo>,
        fail_connect: bool,
    }

    impl MockFactory {
        fn new(tools: Vec<McpToolInfo>) -> Self {
            Self {
                tools,
                fail_connect: false,
            }
        }

        fn failing() -> Self {
            Self {
                tools: vec![],
                fail_connect: true,
            }
        }
    }

    impl McpTransportFactory for MockFactory {
        fn create(&self, _config: &McpServerConfig) -> Box<dyn McpTransport> {
            if self.fail_connect {
                Box::new(MockTransport::failing())
            } else {
                Box::new(MockTransport::new(self.tools.clone()))
            }
        }
    }

    fn make_config(id: &str, auto_connect: bool) -> McpServerConfig {
        McpServerConfig {
            id: id.into(),
            display_name: format!("{id} display"),
            transport: crate::types::McpTransportConfig::Stdio {
                command: "echo".into(),
                args: vec![],
                env: HashMap::new(),
            },
            auto_connect,
            connect_timeout_secs: 5,
        }
    }

    fn sample_tool(server_id: &str, name: &str) -> McpToolInfo {
        McpToolInfo {
            server_id: server_id.into(),
            tool_name: name.into(),
            description: format!("{name} tool"),
            input_schema: serde_json::json!({}),
        }
    }

    // ── Tests ───────────────────────────────────────────────────────────

    #[tokio::test]
    async fn new_client_has_no_servers() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);
        let states = client.server_states().await;
        assert!(states.is_empty());
    }

    #[tokio::test]
    async fn register_adds_server_in_disconnected_state() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;

        let states = client.server_states().await;
        assert_eq!(states.len(), 1);
        assert_eq!(states["srv1"], McpServerState::Disconnected);
    }

    #[tokio::test]
    async fn connect_transitions_to_connected() {
        let tools = vec![sample_tool("srv1", "tool-a"), sample_tool("srv1", "tool-b")];
        let factory = Arc::new(MockFactory::new(tools));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.connect("srv1").await.unwrap();

        let states = client.server_states().await;
        assert_eq!(
            states["srv1"],
            McpServerState::Connected { tool_count: 2 }
        );
    }

    #[tokio::test]
    async fn connect_idempotent_when_already_connected() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.connect("srv1").await.unwrap();
        // Second connect should succeed silently
        client.connect("srv1").await.unwrap();
    }

    #[tokio::test]
    async fn connect_unknown_server_returns_error() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);

        let err = client.connect("nonexistent").await.unwrap_err();
        assert!(matches!(err, McpError::ServerNotFound(_)));
    }

    #[tokio::test]
    async fn connect_failure_transitions_to_failed() {
        let factory = Arc::new(MockFactory::failing());
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;

        let err = client.connect("srv1").await;
        assert!(err.is_err());

        let states = client.server_states().await;
        match &states["srv1"] {
            McpServerState::Failed { reason } => {
                assert!(reason.contains("mock connection failure"));
            }
            other => panic!("expected Failed, got {:?}", other),
        }
    }

    #[tokio::test]
    async fn disconnect_transitions_back_to_disconnected() {
        let factory = Arc::new(MockFactory::new(vec![sample_tool("srv1", "t")]));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.connect("srv1").await.unwrap();
        client.disconnect("srv1").await.unwrap();

        let states = client.server_states().await;
        assert_eq!(states["srv1"], McpServerState::Disconnected);
    }

    #[tokio::test]
    async fn disconnect_clears_tools() {
        let factory = Arc::new(MockFactory::new(vec![sample_tool("srv1", "t")]));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.connect("srv1").await.unwrap();

        assert_eq!(client.list_all_tools().await.len(), 1);
        client.disconnect("srv1").await.unwrap();
        assert!(client.list_all_tools().await.is_empty());
    }

    #[tokio::test]
    async fn list_all_tools_aggregates_across_servers() {
        let tools = vec![
            sample_tool("srv1", "tool-a"),
            sample_tool("srv2", "tool-b"),
        ];
        let factory = Arc::new(MockFactory::new(tools));
        let client = McpClient::new(factory);

        client.register(make_config("srv1", true)).await;
        client.register(make_config("srv2", true)).await;
        client.connect("srv1").await.unwrap();
        client.connect("srv2").await.unwrap();

        let all_tools = client.list_all_tools().await;
        // Each server gets the same 2 tools from factory, so total = 4
        // (the mock factory returns the same set for each server)
        assert_eq!(all_tools.len(), 4);
    }

    #[tokio::test]
    async fn call_tool_on_connected_server() {
        let tools = vec![sample_tool("srv1", "greet")];
        let factory = Arc::new(MockFactory::new(tools));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.connect("srv1").await.unwrap();

        let result = client
            .call_tool("srv1", "greet", serde_json::json!({"name": "Alice"}))
            .await
            .unwrap();

        assert_eq!(result["tool"], "greet");
        assert_eq!(result["result"], "ok");
    }

    #[tokio::test]
    async fn call_tool_on_unknown_server_returns_error() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);

        let err = client
            .call_tool("nope", "t", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::ServerNotFound(_)));
    }

    #[tokio::test]
    async fn call_tool_on_disconnected_server_returns_error() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);
        client.register(make_config("srv1", false)).await;

        let err = client
            .call_tool("srv1", "t", serde_json::json!({}))
            .await
            .unwrap_err();
        assert!(matches!(err, McpError::NotConnected(_)));
    }

    #[tokio::test]
    async fn connect_auto_connects_auto_servers_only() {
        let factory = Arc::new(MockFactory::new(vec![]));
        let client = McpClient::new(factory);
        client.register(make_config("auto-yes", true)).await;
        client.register(make_config("auto-no", false)).await;

        let errors = client.connect_auto().await;
        assert!(errors.is_empty());

        let states = client.server_states().await;
        assert!(matches!(
            states["auto-yes"],
            McpServerState::Connected { .. }
        ));
        assert_eq!(states["auto-no"], McpServerState::Disconnected);
    }

    #[tokio::test]
    async fn connect_auto_collects_errors() {
        let factory = Arc::new(MockFactory::failing());
        let client = McpClient::new(factory);
        client.register(make_config("srv1", true)).await;
        client.register(make_config("srv2", true)).await;

        let errors = client.connect_auto().await;
        assert_eq!(errors.len(), 2);
    }
}
