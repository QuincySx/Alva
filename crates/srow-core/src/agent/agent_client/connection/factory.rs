// INPUT:  std::collections, std::sync, tokio::sync, crate::agent::agent_client::{protocol, connection}, uuid
// OUTPUT: ProcessManagerConfig, AcpProcessManager
// POS:    Global ACP process manager — spawns external Agent processes, routes messages via broadcast, and manages lifecycle.
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::agent::agent_client::{
    protocol::{bootstrap::BootstrapPayload, message::AcpInboundMessage},
    connection::{
        discovery::{AgentDiscovery, ExternalAgentKind},
        processes::{AcpProcessHandle, ProcessState},
    },
    AcpError,
};

/// Process manager configuration
#[derive(Debug, Clone)]
pub struct ProcessManagerConfig {
    /// Max restart attempts after crash
    pub max_restart_attempts: u32,
    /// Restart interval (seconds)
    pub restart_delay_secs: u64,
    /// Heartbeat timeout (seconds) -- triggers force kill
    pub heartbeat_timeout_secs: u64,
}

impl Default for ProcessManagerConfig {
    fn default() -> Self {
        Self {
            max_restart_attempts: 3,
            restart_delay_secs: 2,
            heartbeat_timeout_secs: 30,
        }
    }
}

/// Global ACP process manager (singleton, held in AppState)
pub struct AcpProcessManager {
    #[allow(dead_code)]
    config: ProcessManagerConfig,
    /// Agent discovery instance
    discovery: AgentDiscovery,
    /// process_id -> handle
    processes: Arc<Mutex<HashMap<String, AcpProcessHandle>>>,
    /// Broadcast channel: all process messages unified broadcast
    /// (session filters by process_id)
    event_tx: broadcast::Sender<(String, AcpInboundMessage)>,
}

impl AcpProcessManager {
    pub async fn new(config: ProcessManagerConfig, app_name: &str) -> Self {
        // Clean up orphan processes on startup
        super::orphan::cleanup_orphan_processes().await;

        let (event_tx, _) = broadcast::channel(1024);
        Self {
            config,
            discovery: AgentDiscovery::new(app_name),
            processes: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    /// Start a new external Agent child process.
    /// Returns process_id (UUID), usable for subsequent send / shutdown.
    pub async fn spawn(
        &self,
        kind: ExternalAgentKind,
        bootstrap: BootstrapPayload,
    ) -> Result<String, AcpError> {
        let cmd = self.discovery.discover(&kind)?;
        let process_id = uuid::Uuid::new_v4().to_string();

        // Create message routing channel (process -> broadcast)
        let (inbound_tx, mut inbound_rx) = mpsc::channel::<AcpInboundMessage>(256);
        let event_tx = self.event_tx.clone();
        let pid_for_broadcast = process_id.clone();

        tokio::spawn(async move {
            while let Some(msg) = inbound_rx.recv().await {
                let _ = event_tx.send((pid_for_broadcast.clone(), msg));
            }
        });

        let handle = AcpProcessHandle::spawn(&cmd, bootstrap, inbound_tx).await?;

        tracing::info!(
            "acp process spawned: process_id={process_id} pid={} kind={:?}",
            handle.pid,
            kind
        );

        self.processes
            .lock()
            .await
            .insert(process_id.clone(), handle);
        Ok(process_id)
    }

    /// Send message to specified process
    pub async fn send(
        &self,
        process_id: &str,
        msg: crate::agent::agent_client::protocol::message::AcpOutboundMessage,
    ) -> Result<(), AcpError> {
        let processes = self.processes.lock().await;
        let handle = processes
            .get(process_id)
            .ok_or_else(|| AcpError::ProcessNotFound(process_id.to_string()))?;
        handle.send(msg).await
    }

    /// Subscribe to messages from a specific process.
    /// Caller filters by process_id from the broadcast stream.
    pub fn subscribe(&self) -> broadcast::Receiver<(String, AcpInboundMessage)> {
        self.event_tx.subscribe()
    }

    /// Shutdown and remove process
    pub async fn shutdown(&self, process_id: &str) {
        let mut processes = self.processes.lock().await;
        if let Some(handle) = processes.remove(process_id) {
            handle.shutdown().await;
            tracing::info!("acp process shutdown: process_id={process_id}");
        }
    }

    /// Get process state
    pub async fn process_state(&self, process_id: &str) -> Option<ProcessState> {
        let processes = self.processes.lock().await;
        match processes.get(process_id) {
            Some(h) => Some(h.state().await),
            None => None,
        }
    }

    /// List all active processes
    pub async fn list_processes(&self) -> Vec<String> {
        self.processes.lock().await.keys().cloned().collect()
    }
}
