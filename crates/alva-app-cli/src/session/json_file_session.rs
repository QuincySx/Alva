// INPUT:  alva_kernel_abi::agent_session, serde, serde_json, tokio sync/fs, base64
// OUTPUT: JsonFileAgentSession, private atomic typed-record SessionFile persistence
// POS:    CLI-private AgentSession backend — stores a single session's full
//         canonical typed records + snapshot in one backward-compatible JSON file. Serializes mutation+persist
//         and atomically replaces a mode-0600 file on every write.

use std::path::{Path, PathBuf};

use async_trait::async_trait;
use base64::engine::general_purpose::STANDARD as B64;
use base64::Engine as _;
use serde::{Deserialize, Serialize};
use tokio::io::AsyncWriteExt as _;
use tokio::sync::Mutex;

use alva_kernel_abi::{AgentMessage, AppendReceipt, SessionRecord};
use alva_kernel_core::agent_session::{
    AgentSession, EventMatch, EventQuery, InMemoryAgentSession, SessionError, SessionEvent,
};

/// On-disk representation of a session file.
#[derive(Debug, Serialize, Deserialize)]
struct SessionFile {
    session_id: String,
    parent_session_id: Option<String>,
    /// Canonical typed records used by current writers. Keeping the legacy
    /// event field below makes existing session files forward-readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    records: Option<Vec<SessionRecord>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    events: Vec<SessionEvent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    snapshot_base64: Option<String>,
}

/// CLI-private `AgentSession` backend that persists to a single JSON file.
///
/// All in-memory state is held by an inner `InMemoryAgentSession`. Every
/// write operation (append, rollback, snapshot, clear) triggers a full
/// file rewrite. Legacy infallible session methods retain dirty state in
/// memory and surface persistent failures from `flush`; the fallible typed
/// record path rolls its in-memory batch back when the atomic file replace
/// fails.
pub struct JsonFileAgentSession {
    path: PathBuf,
    inner: InMemoryAgentSession,
    write_lock: Mutex<()>,
}

impl JsonFileAgentSession {
    /// Create a new session at `path` with a fresh random session_id.
    /// The file is not created on disk until the first persist (via any
    /// write method or explicit `flush()`).
    pub fn new_at(path: PathBuf) -> Self {
        Self {
            path,
            inner: InMemoryAgentSession::new(),
            write_lock: Mutex::new(()),
        }
    }

    /// Create a session at `path` with a specific session_id. Used when
    /// resuming an existing session file — the caller passes the session_id
    /// read from the file, and `restore()` will populate the events.
    pub fn with_id_at(path: PathBuf, session_id: String) -> Self {
        Self {
            path,
            inner: InMemoryAgentSession::with_id(session_id),
            write_lock: Mutex::new(()),
        }
    }

    /// Path of the on-disk JSON file for this session.
    pub fn file_path(&self) -> &Path {
        &self.path
    }

    /// Write the current in-memory state to the on-disk file. Best-effort;
    /// errors are logged, not propagated.
    async fn persist_locked(&self) {
        if let Err(e) = self.persist_inner_locked().await {
            tracing::warn!(
                path = %self.path.display(),
                error = %e,
                "JsonFileAgentSession: failed to persist session to disk"
            );
        }
    }

    async fn persist_inner_locked(&self) -> Result<(), SessionError> {
        let records = self.inner.records(0).await;
        // Keep the legacy event projection alongside canonical typed records.
        // Current readers prefer `records`, while older CLI builds can still
        // resume files written by this version without seeing an empty log.
        let events = records
            .iter()
            .cloned()
            .map(SessionRecord::into_event)
            .collect();

        let snapshot = self.inner.load_snapshot().await;

        let file = SessionFile {
            session_id: self.inner.session_id().to_string(),
            parent_session_id: self.inner.parent_session_id().map(String::from),
            records: Some(records),
            events,
            snapshot_base64: snapshot.map(|b| B64.encode(b)),
        };

        let json = serde_json::to_string_pretty(&file).map_err(SessionError::Serialization)?;

        atomic_write_private(&self.path, json.as_bytes())
            .await
            .map_err(SessionError::Io)?;
        Ok(())
    }

    /// Read the on-disk file (if present) and replay every event into the
    /// inner session, then restore the snapshot. Called from `restore()`.
    async fn load_from_disk_locked(&self) -> Result<(), SessionError> {
        if !self.path.exists() {
            return Ok(());
        }
        let contents = tokio::fs::read_to_string(&self.path)
            .await
            .map_err(SessionError::Io)?;
        let file: SessionFile =
            serde_json::from_str(&contents).map_err(SessionError::Serialization)?;

        if let Some(records) = file.records {
            self.inner.clear().await?;
            self.inner.append_records(records).await?;
        } else {
            // Legacy v1 files stored only events. Replay them so both the
            // event log and message projection are rebuilt.
            self.inner.restore_events(file.events).await;
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
        let _guard = self.write_lock.lock().await;
        self.inner.append(event).await;
        self.persist_locked().await;
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        let _guard = self.write_lock.lock().await;
        self.inner.append_message(msg, parent_uuid).await;
        self.persist_locked().await;
    }

    async fn append_records(
        &self,
        records: Vec<SessionRecord>,
    ) -> Result<AppendReceipt, SessionError> {
        let _guard = self.write_lock.lock().await;
        let previous_last_id = self
            .inner
            .records(0)
            .await
            .last()
            .map(|record| record.record_id.clone());
        let previous_snapshot = self.inner.load_snapshot().await;
        let receipt = self.inner.append_records(records).await?;
        match self.persist_inner_locked().await {
            Ok(()) => Ok(receipt),
            Err(error) => {
                if let Some(record_id) = previous_last_id {
                    self.inner.rollback_after(&record_id).await;
                } else {
                    self.inner.clear().await?;
                    if let Some(snapshot) = previous_snapshot {
                        self.inner.save_snapshot(&snapshot).await;
                    }
                }
                Err(error)
            }
        }
    }

    fn supports_atomic_records(&self) -> bool {
        true
    }

    async fn compact_messages(&self, replacement: Vec<AgentMessage>) {
        let _guard = self.write_lock.lock().await;
        // Non-destructive: keep the event log intact, then persist the
        // rewritten projection + preserved log to the JSON file.
        self.inner.compact_messages(replacement).await;
        self.persist_locked().await;
    }

    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.inner.query(filter).await
    }

    async fn count(&self, filter: &EventQuery) -> usize {
        self.inner.count(filter).await
    }

    async fn records(&self, from_seq: u64) -> Vec<SessionRecord> {
        self.inner.records(from_seq).await
    }

    async fn messages(&self) -> Vec<AgentMessage> {
        self.inner.messages().await
    }

    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage> {
        self.inner.recent_messages(n).await
    }

    async fn rollback_after(&self, uuid: &str) -> usize {
        let _guard = self.write_lock.lock().await;
        let dropped = self.inner.rollback_after(uuid).await;
        self.persist_locked().await;
        dropped
    }

    async fn save_snapshot(&self, data: &[u8]) {
        let _guard = self.write_lock.lock().await;
        self.inner.save_snapshot(data).await;
        self.persist_locked().await;
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.inner.load_snapshot().await
    }

    async fn restore(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.load_from_disk_locked().await
    }

    async fn flush(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.persist_inner_locked().await
    }

    async fn close(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.persist_inner_locked().await
    }

    async fn clear(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.inner.clear().await?;
        if self.path.exists() {
            tokio::fs::remove_file(&self.path)
                .await
                .map_err(SessionError::Io)?;
        }
        Ok(())
    }
}

async fn atomic_write_private(path: &Path, contents: &[u8]) -> std::io::Result<()> {
    let parent = path.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "session file must have a parent directory",
        )
    })?;
    match tokio::fs::symlink_metadata(parent).await {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!(
                    "refusing symlinked session parent directory: {}",
                    parent.display()
                ),
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                format!("session parent is not a directory: {}", parent.display()),
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tokio::fs::create_dir_all(parent).await?
        }
        Err(error) => return Err(error),
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt as _;
        tokio::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700)).await?;
    }

    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("session.json");
    let temp_path = parent.join(format!(
        ".{file_name}.{}.tmp",
        uuid::Uuid::new_v4().simple()
    ));
    let result = async {
        let mut options = tokio::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            options.mode(0o600);
        }
        let mut file = options.open(&temp_path).await?;
        file.write_all(contents).await?;
        file.sync_all().await?;
        drop(file);
        #[cfg(windows)]
        if path.exists() {
            tokio::fs::remove_file(path).await?;
        }
        tokio::fs::rename(&temp_path, path).await
    }
    .await;
    if result.is_err() {
        let _ = tokio::fs::remove_file(&temp_path).await;
    }
    result
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
    use alva_kernel_abi::{EventEmitter, ExecutionRecord};
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
            writer
                .append(SessionEvent::user_message(json!("first")))
                .await;
            writer
                .append(SessionEvent::progress(json!({"k": "v"})))
                .await;
        }

        let reader = JsonFileAgentSession::with_id_at(path, "rtrip-id".into());
        reader.restore().await.expect("restore ok");

        let events = reader
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await;
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
        writer
            .append(SessionEvent::user_message(json!("keep-1")))
            .await;
        let pivot = SessionEvent::user_message(json!("pivot"));
        let pivot_uuid = pivot.uuid.clone();
        writer.append(pivot).await;
        writer
            .append(SessionEvent::user_message(json!("drop-1")))
            .await;
        writer
            .append(SessionEvent::user_message(json!("drop-2")))
            .await;

        let dropped = writer.rollback_after(&pivot_uuid).await;
        assert_eq!(dropped, 2, "must drop the two events after pivot");

        // Verify on-disk state matches: a new reader restored from the
        // same path must see only the 2 surviving events.
        let reader = JsonFileAgentSession::with_id_at(path, "rb-id".into());
        reader.restore().await.expect("restore");
        let events = reader
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await;
        assert_eq!(
            events.len(),
            2,
            "disk file must reflect post-rollback state"
        );
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

        let events = s
            .query(&EventQuery {
                limit: usize::MAX,
                ..Default::default()
            })
            .await;
        assert!(events.is_empty(), "no events loaded from non-existent file");
    }

    #[tokio::test]
    async fn concurrent_appends_are_not_lost_or_corrupted() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "concurrent");
        let session = std::sync::Arc::new(JsonFileAgentSession::with_id_at(
            path.clone(),
            "concurrent-id".into(),
        ));

        let mut tasks = Vec::new();
        for i in 0..24 {
            let session = session.clone();
            tasks.push(tokio::spawn(async move {
                session
                    .append(SessionEvent::user_message(json!({ "index": i })))
                    .await;
            }));
        }
        for task in tasks {
            task.await.unwrap();
        }

        let restored = JsonFileAgentSession::with_id_at(path, "concurrent-id".into());
        restored.restore().await.unwrap();
        assert_eq!(restored.count(&EventQuery::default()).await, 24);
    }

    #[tokio::test]
    async fn failed_atomic_record_persist_rolls_back_the_in_memory_batch() {
        let tmp = tempfile::tempdir().unwrap();
        let invalid_parent = tmp.path().join("not-a-directory");
        std::fs::write(&invalid_parent, "blocking file").unwrap();
        let session = JsonFileAgentSession::with_id_at(
            invalid_parent.join("session.json"),
            "atomic-failure".into(),
        );
        let record = SessionRecord::execution(
            "test_record",
            EventEmitter::runtime(),
            ExecutionRecord::Event {
                data: json!({ "value": 1 }),
            },
        );

        let error = session
            .append_records(vec![record])
            .await
            .expect_err("atomic file replacement must fail");

        assert!(error.to_string().contains("not a directory"));
        assert!(session.records(0).await.is_empty());
        assert!(session.messages().await.is_empty());
    }

    #[tokio::test]
    async fn typed_records_keep_their_payload_kind_after_restart() {
        let tmp = tempfile::tempdir().unwrap();
        let path = session_path(tmp.path(), "typed-records");
        let writer = JsonFileAgentSession::with_id_at(path.clone(), "typed-id".into());
        writer
            .append_records(vec![SessionRecord::execution(
                "tool_intent",
                EventEmitter::runtime(),
                ExecutionRecord::Intent {
                    operation_id: "op-1".into(),
                    operation: "read".into(),
                    input: alva_kernel_abi::RecordValue::Inline {
                        value: json!({ "path": "README.md" }),
                    },
                },
            )])
            .await
            .unwrap();

        let reader = JsonFileAgentSession::with_id_at(path, "typed-id".into());
        reader.restore().await.unwrap();
        let records = reader.records(0).await;

        assert!(
            matches!(
                &records[0].payload,
                alva_kernel_abi::SessionRecordPayload::Execution(
                    ExecutionRecord::Intent { operation_id, .. }
                ) if operation_id == "op-1"
            ),
            "restored payload: {:?}",
            records[0].payload
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn persisted_session_and_directory_are_private() {
        use std::os::unix::fs::PermissionsExt as _;

        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("sessions");
        let path = session_path(&dir, "private");
        let session = JsonFileAgentSession::with_id_at(path.clone(), "private-id".into());
        session
            .append(SessionEvent::user_message(json!("private")))
            .await;

        assert_eq!(
            std::fs::metadata(&dir).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn atomic_persist_replaces_symlink_without_touching_its_target() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::tempdir().unwrap();
        let outside = tmp.path().join("outside.txt");
        std::fs::write(&outside, "do-not-overwrite").unwrap();
        let path = session_path(tmp.path(), "linked");
        symlink(&outside, &path).unwrap();

        let session = JsonFileAgentSession::with_id_at(path.clone(), "linked-id".into());
        session
            .append(SessionEvent::user_message(json!("safe")))
            .await;

        assert_eq!(
            std::fs::read_to_string(outside).unwrap(),
            "do-not-overwrite"
        );
        assert!(
            !std::fs::symlink_metadata(path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "atomic rename must replace the link itself"
        );
    }
}
