// INPUT:  alva_kernel_abi::agent_session, serde, serde_json, tokio::fs, base64
// OUTPUT: JsonFileAgentSession, SessionFile
// POS:    CLI-private AgentSession backend — stores a single session's full
//         event log + snapshot in one JSON file. Wraps InMemoryAgentSession
//         for in-memory state, persists to disk on every write.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use serde::{Deserialize, Serialize};

use alva_kernel_abi::agent_session::{
    AgentSession, EventMatch, EventQuery, InMemoryAgentSession, SessionError,
    SessionEvent,
};
use alva_kernel_abi::AgentMessage;

/// On-disk representation of a session file.
#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    session_id: String,
    parent_session_id: Option<String>,
    events: Vec<SessionEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot_base64: Option<String>,
}

/// CLI-private `AgentSession` backend that persists to a single JSON file.
///
/// All in-memory state is held by an inner `InMemoryAgentSession`. Every
/// write operation (append, rollback, snapshot, clear) triggers a full
/// file rewrite. Failures during persistence are logged but do not
/// propagate — the in-memory state always reflects the most recent writes
/// even if persistence lags.
pub struct JsonFileAgentSession {
    path: PathBuf,
    inner: InMemoryAgentSession,
}

impl JsonFileAgentSession {
    /// Create a new session at `path` with a fresh random session_id.
    /// The file is not created on disk until the first persist (via any
    /// write method or explicit `flush()`).
    pub fn new_at(path: PathBuf) -> Self {
        Self {
            path,
            inner: InMemoryAgentSession::new(),
        }
    }

    /// Create a session at `path` with a specific session_id. Used when
    /// resuming an existing session file — the caller passes the session_id
    /// read from the file, and `restore()` will populate the events.
    pub fn with_id_at(path: PathBuf, session_id: String) -> Self {
        Self {
            path,
            inner: InMemoryAgentSession::with_id(session_id),
        }
    }

    /// Path of the on-disk JSON file for this session.
    pub fn file_path(&self) -> &Path {
        &self.path
    }

    /// Write the current in-memory state to the on-disk file. Best-effort;
    /// errors are logged, not propagated.
    async fn persist(&self) {
        if let Err(e) = self.persist_inner().await {
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "JsonFileAgentSession: failed to persist session to disk"
            );
        }
    }

    async fn persist_inner(&self) -> Result<(), SessionError> {
        // Pull all events via a single query. The in-memory backend serves
        // this from its Vec in O(n) without allocations beyond the clones.
        let events: Vec<SessionEvent> = self
            .inner
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await
            .into_iter()
            .map(|m: EventMatch| m.event)
            .collect();

        let snapshot = self.inner.load_snapshot().await;

        let file = SessionFile {
            session_id: self.inner.session_id().to_string(),
            parent_session_id: self.inner.parent_session_id().map(String::from),
            events,
            snapshot_base64: snapshot.map(|b| B64.encode(b)),
        };

        let json = serde_json::to_string_pretty(&file)
            .map_err(SessionError::Serialization)?;

        if let Some(parent) = self.path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(SessionError::Io)?;
        }
        tokio::fs::write(&self.path, json)
            .await
            .map_err(SessionError::Io)?;
        Ok(())
    }

    /// Read the on-disk file (if present) and replay every event into the
    /// inner session, then restore the snapshot. Called from `restore()`.
    async fn load_from_disk(&self) -> Result<(), SessionError> {
        if !self.path.exists() {
            return Ok(());
        }
        let contents = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(SessionError::Io)?;
        let file: SessionFile = serde_json::from_str(&contents)
            .map_err(SessionError::Serialization)?;

        // Replay events. Each append reassigns seq inside the inner session;
        // that's fine because after the full replay, seq_counter is back in
        // sync with the number of events.
        for event in file.events {
            self.inner.append(event).await;
        }

        if let Some(snap_b64) = file.snapshot_base64 {
            let bytes = B64
                .decode(snap_b64.as_bytes())
                .map_err(|e| SessionError::Other(format!("invalid snapshot base64: {}", e)))?;
            self.inner.save_snapshot(&bytes).await;
        }

        Ok(())
    }
}

#[async_trait]
impl AgentSession for JsonFileAgentSession {
    fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    fn parent_session_id(&self) -> Option<&str> {
        self.inner.parent_session_id()
    }

    async fn append(&self, event: SessionEvent) {
        self.inner.append(event).await;
        self.persist().await;
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        self.inner.append_message(msg, parent_uuid).await;
        self.persist().await;
    }

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.inner.query(filter).await
    }

    async fn count(&self, filter: &EventQuery) -> usize {
        self.inner.count(filter).await
    }

    async fn messages(&self) -> Vec<AgentMessage> {
        self.inner.messages().await
    }

    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage> {
        self.inner.recent_messages(n).await
    }

    async fn rollback_after(&self, uuid: &str) -> usize {
        let dropped = self.inner.rollback_after(uuid).await;
        self.persist().await;
        dropped
    }

    async fn save_snapshot(&self, data: &[u8]) {
        self.inner.save_snapshot(data).await;
        self.persist().await;
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.inner.load_snapshot().await
    }

    async fn restore(&self) -> Result<(), SessionError> {
        self.load_from_disk().await
    }

    async fn flush(&self) -> Result<(), SessionError> {
        self.persist_inner().await
    }

    async fn close(&self) -> Result<(), SessionError> {
        self.persist_inner().await
    }

    async fn clear(&self) -> Result<(), SessionError> {
        self.inner.clear().await?;
        if self.path.exists() {
            tokio::fs::remove_file(&self.path)
                .await
                .map_err(SessionError::Io)?;
        }
        Ok(())
    }
}
