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

        // Replay events via `restore_events` so both the event log AND
        // the messages projection get rebuilt. Plain `append()` only
        // writes to the log, leaving the messages cache empty — which
        // the UI reads as "history is gone". See
        // InMemoryAgentSession::restore_events for the contract.
        self.inner.restore_events(file.events).await;

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

#[cfg(test)]
mod tests {
    //! Tests for JsonFileAgentSession — the CLI's on-disk session
    //! backend. Mirror of the deferred-flush contract pinned in
    //! sqlite_session::tests, but for the *eager*-persist model:
    //!   * every write triggers a full file rewrite
    //!   * restore round-trips events + snapshot (base64) via the
    //!     restore_events path (so the messages projection rebuilds)
    //!   * clear deletes the file
    //!
    //! tempfile dev-dep already available in alva-app-cli.
    use super::*;
    use serde_json::json;

    fn session_path(dir: &Path, label: &str) -> PathBuf {
        dir.join(format!("{label}.json"))
    }

    #[tokio::test]
    async fn new_at_assigns_random_id_and_does_not_create_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "fresh");

        let s = JsonFileAgentSession::new_at(path.clone());
        assert!(!s.session_id().is_empty());
        assert_eq!(s.file_path(), path.as_path());
        // No write yet — file should not exist.
        assert!(!path.exists(), "no persist before first write");
    }

    #[tokio::test]
    async fn with_id_at_honors_caller_provided_id() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "explicit");
        let s = JsonFileAgentSession::with_id_at(path, "explicit-id-99".into());
        assert_eq!(s.session_id(), "explicit-id-99");
    }

    #[tokio::test]
    async fn append_eagerly_persists_to_disk() {
        // Core contract: append → file exists immediately (NOT deferred
        // like the sqlite_session backend). Regressions converting this
        // to lazy/deferred would lose data on CLI crash before flush.
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "eager");
        let s = JsonFileAgentSession::with_id_at(path.clone(), "eager-id".into());

        assert!(!path.exists(), "file must not exist before first append");
        s.append(SessionEvent::user_message(json!("hello"))).await;
        assert!(path.exists(), "file must exist after first append");

        // File parses back as a SessionFile with one event.
        let raw = std::fs::read_to_string(&path).unwrap();
        let parsed: SessionFile = serde_json::from_str(&raw).unwrap();
        assert_eq!(parsed.session_id, "eager-id");
        assert_eq!(parsed.events.len(), 1);
    }

    #[tokio::test]
    async fn restore_round_trips_events_into_new_session_instance() {
        // Full re-open cycle: write via one session, drop it, then a
        // new JsonFileAgentSession::with_id_at the same path runs
        // restore() and sees the same events. This is the actual
        // CLI session-resume code path.
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "rtrip");
        {
            let writer = JsonFileAgentSession::with_id_at(path.clone(), "rtrip-id".into());
            writer.append(SessionEvent::user_message(json!("first"))).await;
            writer.append(SessionEvent::progress(json!({"k": "v"}))).await;
        }

        let reader = JsonFileAgentSession::with_id_at(path, "rtrip-id".into());
        reader.restore().await.expect("restore ok");

        let events = reader.query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        }).await;
        assert_eq!(events.len(), 2);
        let types: Vec<_> = events.iter().map(|m| m.event.event_type.clone()).collect();
        assert!(types.iter().any(|t| t == "user"));
        assert!(types.iter().any(|t| t == "progress"));
    }

    #[tokio::test]
    async fn snapshot_bytes_round_trip_via_base64() {
        // Pin: snapshot is encoded as base64 on persist and decoded on
        // restore — any drift between B64.encode and B64.decode would
        // silently corrupt the compact-restore path.
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "snap");
        let snap: Vec<u8> = (0u8..=255).collect(); // full byte-range smoke
        {
            let writer = JsonFileAgentSession::with_id_at(path.clone(), "snap-id".into());
            writer.save_snapshot(&snap).await;
        }

        let reader = JsonFileAgentSession::with_id_at(path, "snap-id".into());
        reader.restore().await.expect("restore ok");
        let got = reader.load_snapshot().await.expect("snapshot present");
        assert_eq!(got, snap, "snapshot bytes must round-trip exactly");
    }

    #[tokio::test]
    async fn rollback_after_re_persists_truncated_events() {
        // Pin: rollback also triggers persist (so the disk file
        // reflects the truncation). Without this, a rollback in
        // memory would diverge from the disk on next restore.
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "rollback");
        let writer = JsonFileAgentSession::with_id_at(path.clone(), "rb-id".into());
        writer.append(SessionEvent::user_message(json!("keep-1"))).await;
        let pivot = SessionEvent::user_message(json!("pivot"));
        let pivot_uuid = pivot.uuid.clone();
        writer.append(pivot).await;
        writer.append(SessionEvent::user_message(json!("drop-1"))).await;
        writer.append(SessionEvent::user_message(json!("drop-2"))).await;

        let dropped = writer.rollback_after(&pivot_uuid).await;
        assert_eq!(dropped, 2, "must drop the two events after pivot");

        // Verify on-disk state matches: a new reader restored from the
        // same path must see only the 2 surviving events.
        let reader = JsonFileAgentSession::with_id_at(path, "rb-id".into());
        reader.restore().await.expect("restore");
        let events = reader.query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        }).await;
        assert_eq!(events.len(), 2, "disk file must reflect post-rollback state");
    }

    #[tokio::test]
    async fn clear_removes_session_file() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "to-clear");
        let s = JsonFileAgentSession::with_id_at(path.clone(), "clear-id".into());
        s.append(SessionEvent::user_message(json!("doomed"))).await;
        assert!(path.exists());

        s.clear().await.expect("clear ok");
        assert!(!path.exists(), "file must be removed after clear");
    }

    #[tokio::test]
    async fn restore_on_missing_file_is_a_no_op_not_error() {
        // Pin: opening a fresh session at a non-existent path then
        // calling restore() must NOT error — this is what happens on
        // first launch with a new session id.
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "missing");
        assert!(!path.exists());

        let s = JsonFileAgentSession::with_id_at(path, "new-id".into());
        s.restore().await.expect("restore on missing must succeed");

        let events = s.query(&EventQuery {
            limit: usize::MAX,
            ..Default::default()
        }).await;
        assert!(events.is_empty(), "no events loaded from non-existent file");
    }
}
