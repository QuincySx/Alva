// ACP connection management: agent discovery, process spawning, orphan cleanup, and process handles.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, BufWriter};
use tokio::process::Child;
use tokio::sync::{broadcast, mpsc, Mutex};

use crate::error::AcpError;
use crate::protocol::{
    bootstrap::BootstrapPayload,
    message::{AcpInboundMessage, AcpOutboundMessage},
};

// ---------------------------------------------------------------------------
// Agent discovery
// ---------------------------------------------------------------------------

/// Supported external Agent types
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalAgentKind {
    /// Well-known agent with discovery hints
    Named {
        id: String,
        executables: Vec<String>,
        fallback_npx: Option<String>,
    },
    /// User-specified arbitrary command
    Generic { command: String },
}

/// Discovery result: complete executable command and arguments
#[derive(Debug, Clone)]
pub struct AgentCliCommand {
    pub kind: ExternalAgentKind,
    /// Executable file path (absolute path)
    pub executable: PathBuf,
    /// Additional arguments (e.g. npx package name)
    pub args: Vec<String>,
}

pub struct AgentDiscovery {
    packages_dir: PathBuf,
}

impl AgentDiscovery {
    pub fn new(app_name: &str) -> Self {
        Self {
            packages_dir: builtin_packages_dir(app_name),
        }
    }

    pub fn with_packages_dir(packages_dir: PathBuf) -> Self {
        Self { packages_dir }
    }

    /// Discover the CLI command for the specified Agent
    pub fn discover(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        match kind {
            ExternalAgentKind::Generic { command } => Self::discover_generic(command),
            ExternalAgentKind::Named { .. } => self.discover_named(kind),
        }
    }

    /// Discover a well-known named agent by trying executables in PATH,
    /// then the built-in packages directory, then an npx fallback.
    fn discover_named(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        let ExternalAgentKind::Named {
            id,
            executables,
            fallback_npx,
        } = kind
        else {
            unreachable!()
        };

        // 1. Try each executable in PATH
        for exe_name in executables {
            if let Some(exe) = which(exe_name) {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: exe,
                    args: vec![],
                });
            }
        }

        // 2. Try builtin packages dir
        for exe_name in executables {
            let builtin = self
                .packages_dir
                .join(id)
                .join("node_modules")
                .join(".bin")
                .join(exe_name);
            if builtin.exists() {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: builtin,
                    args: vec![],
                });
            }
        }

        // 3. Try npx fallback
        if let Some(npx_pkg) = fallback_npx {
            if let Some(npx) = which("npx") {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: npx,
                    args: vec![npx_pkg.clone()],
                });
            }
        }

        Err(AcpError::AgentNotFound {
            kind: id.clone(),
            hint: format!("Ensure one of {:?} is in $PATH", executables),
        })
    }

    /// Generic ACP: directly use user-specified command string
    fn discover_generic(command: &str) -> Result<AgentCliCommand, AcpError> {
        let mut parts = command.split_whitespace();
        let exe_str = parts
            .next()
            .ok_or_else(|| AcpError::InvalidConfig("empty command".to_string()))?;
        let extra_args: Vec<String> = parts.map(str::to_string).collect();
        let exe = which(exe_str).ok_or_else(|| AcpError::AgentNotFound {
            kind: exe_str.to_string(),
            hint: format!("Ensure `{}` is in $PATH", exe_str),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::Generic {
                command: command.to_string(),
            },
            executable: exe,
            args: extra_args,
        })
    }
}

/// Search for executable file in system PATH
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    })
}

/// Built-in packages directory (platform-specific)
fn builtin_packages_dir(app_name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                PathBuf::from("C:\\Temp")
            }
            #[cfg(not(target_os = "windows"))]
            {
                PathBuf::from("/tmp")
            }
        })
        .join(app_name)
        .join("packages")
}

// ---------------------------------------------------------------------------
// Orphan process cleanup
// ---------------------------------------------------------------------------

/// Environment variable name injected into child process (parent PID)
pub const PARENT_PID_ENV: &str = "ACP_PROCESS_MANAGER_PID";

/// Return the current process PID as a string for injection into child env.
pub fn parent_pid_env_value() -> String {
    std::process::id().to_string()
}

/// Orphan cleanup:
/// On AcpProcessManager startup, scan for child processes with PARENT_PID_ENV marker.
/// If their parent PID does not match current PID (leftover from previous crash), kill them.
///
/// Called once in AcpProcessManager::new().
pub async fn cleanup_orphan_processes() {
    tracing::info!("scanning for orphan ACP processes...");
    // Phase 1: no-op placeholder. Phase 2 will implement full platform-specific scanning.
    // TODO: enumerate system processes, find those with PARENT_PID_ENV whose parent PID
    // is not the current process, and send SIGTERM.
}

// ---------------------------------------------------------------------------
// Process state and handle
// ---------------------------------------------------------------------------

/// Child process lifecycle state
#[derive(Debug, Clone, PartialEq)]
pub enum ProcessState {
    /// Running
    Running,
    /// Normal exit (exit code = 0)
    Exited,
    /// Abnormal exit (exit code != 0 or signal)
    Crashed { exit_code: Option<i32> },
    /// Restarting (automatic retry after crash)
    Restarting { attempt: u32 },
}

/// Handle for a single ACP child process
pub struct AcpProcessHandle {
    /// Process ID
    pub pid: u32,
    /// Agent type identifier
    pub agent_kind: String,
    /// Working directory
    pub workspace: PathBuf,
    /// Child process state
    state: Arc<Mutex<ProcessState>>,
    /// Channel to send messages to child process (stdin writer wrapper)
    stdin_tx: mpsc::Sender<AcpOutboundMessage>,
    /// Inbound sender (kept for reference; actual receiving done externally)
    #[allow(dead_code)]
    inbound_tx: mpsc::Sender<AcpInboundMessage>,
}

impl AcpProcessHandle {
    /// Spawn child process, write bootstrap, start reader/writer tasks
    pub async fn spawn(
        agent_cmd: &AgentCliCommand,
        bootstrap: BootstrapPayload,
        inbound_tx: mpsc::Sender<AcpInboundMessage>,
    ) -> Result<Self, AcpError> {
        let mut cmd = tokio::process::Command::new(&agent_cmd.executable);
        cmd.args(&agent_cmd.args)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            // Inject parent PID (orphan detection)
            .env(PARENT_PID_ENV, parent_pid_env_value())
            // Inject workspace
            .env("ACP_WORKSPACE", &bootstrap.workspace)
            // Disable color escape sequences (ensure stdout is pure JSON)
            .env("NO_COLOR", "1")
            .env("TERM", "dumb");

        let mut child: Child = cmd.spawn().map_err(|e| AcpError::SpawnFailed {
            agent: agent_cmd.executable.display().to_string(),
            reason: e.to_string(),
        })?;

        let pid = child.id().unwrap_or(0);
        let stdin = child.stdin.take().unwrap();
        let stdout = child.stdout.take().unwrap();
        let stderr = child.stderr.take().unwrap();

        // Write bootstrap (one JSON line)
        let bootstrap_json =
            serde_json::to_string(&bootstrap).map_err(|e| AcpError::Serialization(e.to_string()))?;

        let mut writer = BufWriter::new(stdin);
        writer
            .write_all(bootstrap_json.as_bytes())
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer
            .write_all(b"\n")
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;
        writer
            .flush()
            .await
            .map_err(|e| AcpError::Io(e.to_string()))?;

        // Wrap stdin write into a channel
        let (stdin_tx, mut stdin_rx) = mpsc::channel::<AcpOutboundMessage>(64);
        let state = Arc::new(Mutex::new(ProcessState::Running));
        let state_clone = state.clone();

        // Task 1: stdin writer
        tokio::spawn(async move {
            while let Some(msg) = stdin_rx.recv().await {
                let line = match serde_json::to_string(&msg) {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::error!("acp serialize outbound: {e}");
                        continue;
                    }
                };
                if writer.write_all(line.as_bytes()).await.is_err() {
                    break;
                }
                if writer.write_all(b"\n").await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
        });

        // Task 2: stdout reader (parse AcpInboundMessage line by line)
        let inbound_tx_clone = inbound_tx.clone();
        let state_for_reader = state.clone();
        tokio::spawn(async move {
            let reader = BufReader::new(stdout);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                let line = line.trim().to_string();
                if line.is_empty() {
                    continue;
                }
                match serde_json::from_str::<AcpInboundMessage>(&line) {
                    Ok(msg) => {
                        let _ = inbound_tx_clone.send(msg).await;
                    }
                    Err(e) => {
                        tracing::warn!("acp parse inbound: {e}, raw: {line}");
                    }
                }
            }
            // stdout closed = process exited
            *state_for_reader.lock().await = ProcessState::Exited;
        });

        // Task 3: stderr logger
        tokio::spawn(async move {
            let reader = BufReader::new(stderr);
            let mut lines = reader.lines();
            while let Ok(Some(line)) = lines.next_line().await {
                tracing::debug!("[acp-stderr][pid={pid}] {line}");
            }
        });

        // Task 4: process wait (detect crash)
        tokio::spawn(async move {
            let status = child.wait().await;
            let exit_code = status.ok().and_then(|s| s.code());
            let new_state = match exit_code {
                Some(0) => ProcessState::Exited,
                code => ProcessState::Crashed { exit_code: code },
            };
            *state_clone.lock().await = new_state;
        });

        Ok(Self {
            pid,
            agent_kind: format!("{:?}", agent_cmd.kind),
            workspace: PathBuf::from(&bootstrap.workspace),
            state,
            stdin_tx,
            inbound_tx,
        })
    }

    /// Get current process state
    pub async fn state(&self) -> ProcessState {
        self.state.lock().await.clone()
    }

    /// Send message to child process (write to stdin)
    pub async fn send(&self, msg: AcpOutboundMessage) -> Result<(), AcpError> {
        self.stdin_tx
            .send(msg)
            .await
            .map_err(|_| AcpError::ProcessDead { pid: self.pid })
    }

    /// Graceful shutdown
    pub async fn shutdown(&self) {
        let _ = self.send(AcpOutboundMessage::Shutdown).await;
    }
}

// ---------------------------------------------------------------------------
// Process manager
// ---------------------------------------------------------------------------

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

/// Global ACP process manager
pub struct AcpProcessManager {
    #[allow(dead_code)]
    config: ProcessManagerConfig,
    /// process_id -> handle
    processes: Arc<Mutex<HashMap<String, AcpProcessHandle>>>,
    /// Broadcast channel: all process messages unified broadcast
    /// (session filters by process_id)
    event_tx: broadcast::Sender<(String, AcpInboundMessage)>,
}

impl AcpProcessManager {
    pub async fn new(config: ProcessManagerConfig) -> Self {
        // Clean up orphan processes on startup
        cleanup_orphan_processes().await;

        let (event_tx, _) = broadcast::channel(1024);
        Self {
            config,
            processes: Arc::new(Mutex::new(HashMap::new())),
            event_tx,
        }
    }

    /// Start a new external Agent child process.
    /// Returns process_id (UUID), usable for subsequent send / shutdown.
    pub async fn spawn(
        &self,
        discovery: &AgentDiscovery,
        kind: ExternalAgentKind,
        bootstrap: BootstrapPayload,
    ) -> Result<String, AcpError> {
        let cmd = discovery.discover(&kind)?;
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
        msg: AcpOutboundMessage,
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
