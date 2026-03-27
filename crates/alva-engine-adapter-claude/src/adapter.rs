// INPUT:  async_trait, futures_core::Stream, tokio::sync, alva_engine_runtime::*, crate::{bridge, config, mapping, process, protocol}
// OUTPUT: pub struct ClaudeAdapter
// POS:    EngineRuntime implementation that spawns a Node.js bridge process to communicate with the Claude Agent SDK.

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::error;

use alva_engine_runtime::{
    EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
    RuntimeRequest,
};

use crate::bridge::ensure_bridge_script;
use crate::config::{BridgeConfig, ClaudeAdapterConfig};
use crate::mapping::EventMapper;
use crate::process::{BridgeProcess, BridgeSpawnConfig};
use crate::protocol::{BridgeOutbound, BridgePermissionDecision};

/// Claude Agent SDK bridge adapter.
///
/// Implements `EngineRuntime` by spawning a Node.js bridge process that
/// communicates with the Claude Agent SDK via stdin/stdout JSON-line protocol.
pub struct ClaudeAdapter {
    config: ClaudeAdapterConfig,
    /// Active sessions: session_id -> sender for control messages.
    sessions: Arc<Mutex<HashMap<String, mpsc::UnboundedSender<BridgeOutbound>>>>,
}

impl ClaudeAdapter {
    pub fn new(config: ClaudeAdapterConfig) -> Self {
        Self {
            config,
            sessions: Arc::new(Mutex::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl EngineRuntime for ClaudeAdapter {
    fn execute(
        &self,
        request: RuntimeRequest,
    ) -> Result<Pin<Box<dyn Stream<Item = RuntimeEvent> + Send>>, RuntimeError> {
        // Write bridge script (sync I/O — acceptable here since it's a one-time idempotent write)
        let script_path = ensure_bridge_script()?;

        // Build env with cloud provider flags
        let mut env = self.config.env.clone();
        if self.config.use_bedrock {
            env.insert("CLAUDE_CODE_USE_BEDROCK".into(), "1".into());
        }
        if self.config.use_vertex {
            env.insert("CLAUDE_CODE_USE_VERTEX".into(), "1".into());
        }
        if self.config.use_azure {
            env.insert("CLAUDE_CODE_USE_FOUNDRY".into(), "1".into());
        }

        let bridge_config = BridgeConfig {
            prompt: request.prompt,
            cwd: request
                .working_directory
                .map(|p| p.to_string_lossy().into_owned()),
            system_prompt: request.system_prompt,
            streaming: request.options.streaming,
            api_key: self.config.api_key.clone(),
            api_base_url: self.config.api_base_url.clone(),
            model: self.config.model.clone(),
            effort: self.config.effort.clone(),
            max_budget_usd: self.config.max_budget_usd,
            permission_mode: self.config.permission_mode.as_sdk_str().to_string(),
            allowed_tools: self.config.allowed_tools.clone(),
            disallowed_tools: self.config.disallowed_tools.clone(),
            sandbox: self.config.sandbox.clone(),
            mcp_servers: self.config.mcp_servers.clone(),
            agents: self.config.agents.clone(),
            setting_sources: self.config.setting_sources.clone(),
            persist_session: self.config.persist_session,
            sdk_executable_path: self.config.sdk_package_path.clone(),
            env,
        };

        let config_json = serde_json::to_string(&bridge_config)?;
        let node_path = self
            .config
            .node_path
            .clone()
            .unwrap_or_else(|| "node".into());

        let (event_tx, event_rx) = mpsc::unbounded_channel::<RuntimeEvent>();
        let (control_tx, mut control_rx) = mpsc::unbounded_channel::<BridgeOutbound>();
        let sessions = self.sessions.clone();

        tokio::spawn(async move {
            // Spawn bridge process
            let spawn_config = BridgeSpawnConfig {
                node_path,
                script_path: script_path.to_string_lossy().into_owned(),
                config_json,
                env: vec![],
            };

            let mut process = match BridgeProcess::spawn(spawn_config).await {
                Ok(p) => p,
                Err(e) => {
                    let _ = event_tx.send(RuntimeEvent::Error {
                        message: e.to_string(),
                        recoverable: false,
                    });
                    let _ = event_tx.send(RuntimeEvent::Completed {
                        session_id: String::new(),
                        result: None,
                        usage: None,
                    });
                    return;
                }
            };

            let mut mapper = EventMapper::new();

            // Main event loop: read stdout, map events, send to consumer
            loop {
                // Check for pending control messages first
                while let Ok(ctrl) = control_rx.try_recv() {
                    if let Err(e) = process.send(&ctrl).await {
                        error!(target: "claude_adapter", "Failed to send control message: {e}");
                    }
                }

                match process.recv().await {
                    Ok(Some(msg)) => {
                        let is_done =
                            matches!(&msg, crate::protocol::BridgeMessage::Done);
                        let events = mapper.map(msg);

                        // Register session once we know the ID
                        for event in &events {
                            if let RuntimeEvent::SessionStarted { session_id, .. } = event {
                                sessions
                                    .lock()
                                    .await
                                    .insert(session_id.clone(), control_tx.clone());
                            }
                        }

                        for event in events {
                            let is_completed = matches!(&event, RuntimeEvent::Completed { .. });
                            if event_tx.send(event).is_err() {
                                break;
                            }
                            if is_completed {
                                let _ = process.shutdown().await;
                                return;
                            }
                        }

                        if is_done {
                            // Bridge script ended without a Result message — force Completed
                            let _ = event_tx.send(RuntimeEvent::Completed {
                                session_id: mapper.session_id().to_string(),
                                result: None,
                                usage: None,
                            });
                            let _ = process.shutdown().await;
                            return;
                        }
                    }
                    Ok(None) => {
                        // stdout closed — process exited
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: "Bridge process exited unexpectedly".into(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        return;
                    }
                    Err(e) => {
                        let _ = event_tx.send(RuntimeEvent::Error {
                            message: e.to_string(),
                            recoverable: false,
                        });
                        let _ = event_tx.send(RuntimeEvent::Completed {
                            session_id: mapper.session_id().to_string(),
                            result: None,
                            usage: None,
                        });
                        let _ = process.kill().await;
                        return;
                    }
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(event_rx)))
    }

    async fn cancel(&self, session_id: &str) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let tx = sessions
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;
        tx.send(BridgeOutbound::Cancel).map_err(|_| {
            RuntimeError::ProcessError("Session channel closed".into())
        })
    }

    async fn respond_permission(
        &self,
        session_id: &str,
        request_id: &str,
        decision: PermissionDecision,
    ) -> Result<(), RuntimeError> {
        let sessions = self.sessions.lock().await;
        let tx = sessions
            .get(session_id)
            .ok_or_else(|| RuntimeError::SessionNotFound(session_id.into()))?;

        let bridge_decision = match decision {
            PermissionDecision::Allow { updated_input } => {
                BridgePermissionDecision::Allow { updated_input }
            }
            PermissionDecision::Deny { message } => {
                BridgePermissionDecision::Deny { message }
            }
        };

        tx.send(BridgeOutbound::PermissionResponse {
            request_id: request_id.into(),
            decision: bridge_decision,
        })
        .map_err(|_| RuntimeError::ProcessError("Session channel closed".into()))
    }

    fn capabilities(&self) -> RuntimeCapabilities {
        RuntimeCapabilities {
            streaming: true,
            tool_control: false,
            permission_callback: true,
            resume: false, // v1: not implemented
            cancel: true,
        }
    }
}
