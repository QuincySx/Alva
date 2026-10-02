// INPUT:  rusqlite, serde_json, kernel AgentSession contract, listenable in-memory projection
// OUTPUT: SQLite-backed authoritative Alva AgentSession repository and bounded retrieval/source validation
// POS:    Engine-native durable records, atomic batches/dirty repair, Hub resume/Inspector and reference-first authority.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, Weak};

use alva_agent_core::agent_session::ListenableInMemorySession;
use alva_engine_runtime::{
    ChildSessionLink, EngineInspector, InspectionCapabilities, InspectionSnapshot,
    InspectionStream, InspectionUpdate, RuntimeError,
};
use alva_kernel_abi::agent_session::SessionEventStream;
use alva_kernel_abi::session_store::SessionStore;
use alva_kernel_abi::ConversationLog;
use alva_kernel_abi::{
    AgentMessage, AgentSession, AppendReceipt, BlobReference, EventEmitter, EventMatch, EventQuery,
    ExecutionRecord, RecordValue, SessionError, SessionEvent, SessionRecord, SessionRecordPayload,
};
use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};
use sha2::{Digest, Sha256};
use tokio_stream::{wrappers::UnboundedReceiverStream, StreamExt};

const INLINE_RECORD_LIMIT: usize = 64 * 1024;

#[derive(Clone)]
pub struct SessionRepository {
    path: PathBuf,
    blob_dir: PathBuf,
    connection: Arc<Mutex<Connection>>,
    sessions: Arc<Mutex<HashMap<String, Weak<SqliteAgentSession>>>>,
}

impl SessionRepository {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|error| format!("create {}: {error}", parent.display()))?;
        }
        let blob_dir = path.with_extension("blobs");
        std::fs::create_dir_all(&blob_dir)
            .map_err(|error| format!("create {}: {error}", blob_dir.display()))?;
        let connection = Connection::open(&path)
            .map_err(|error| format!("open native session store {}: {error}", path.display()))?;
        connection
            .busy_timeout(std::time::Duration::from_secs(5))
            .map_err(|error| format!("configure native session store: {error}"))?;
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA foreign_keys = ON;
                 CREATE TABLE IF NOT EXISTS native_sessions (
                    session_id TEXT PRIMARY KEY,
                    parent_session_id TEXT,
                    snapshot BLOB,
                    created_at INTEGER NOT NULL
                 );
                 CREATE TABLE IF NOT EXISTS native_records (
                    session_id TEXT NOT NULL REFERENCES native_sessions(session_id) ON DELETE CASCADE,
                    seq INTEGER NOT NULL,
                    record_json BLOB NOT NULL,
                    PRIMARY KEY(session_id, seq)
                 );
                 CREATE INDEX IF NOT EXISTS native_context_record_id
                 ON native_records(session_id, json_extract(CAST(record_json AS TEXT), '$.record_id'));",
            )
            .map_err(|error| format!("initialize native session store: {error}"))?;
        Ok(Self {
            path,
            blob_dir,
            connection: Arc::new(Mutex::new(connection)),
            sessions: Arc::new(Mutex::new(HashMap::new())),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub async fn open_session(
        &self,
        session_id: impl Into<String>,
        parent_session_id: Option<String>,
    ) -> Result<Arc<SqliteAgentSession>, SessionError> {
        let session_id = session_id.into();
        if let Some(session) = self
            .sessions
            .lock()
            .map_err(lock_error)?
            .get(&session_id)
            .and_then(Weak::upgrade)
        {
            return Ok(session);
        }
        let parent_session_id = {
            let connection = self.connection.lock().map_err(lock_error)?;
            connection
                .execute(
                    "INSERT OR IGNORE INTO native_sessions(session_id, parent_session_id, created_at)
                     VALUES (?1, ?2, ?3)",
                    params![
                        session_id,
                        parent_session_id,
                        chrono::Utc::now().timestamp_millis()
                    ],
                )
                .map_err(sql_error)?;
            connection
                .query_row(
                    "SELECT parent_session_id FROM native_sessions WHERE session_id = ?1",
                    [&session_id],
                    |row| row.get(0),
                )
                .map_err(sql_error)?
        };
        let session = Arc::new(SqliteAgentSession {
            session_id,
            parent_session_id,
            connection: Arc::clone(&self.connection),
            blob_dir: self.blob_dir.clone(),
            sessions: Arc::clone(&self.sessions),
            memory: ListenableInMemorySession::new(),
            write_lock: tokio::sync::Mutex::new(()),
            dirty: AtomicBool::new(false),
            record_listeners: tokio::sync::RwLock::new(Vec::new()),
        });
        session.restore().await?;
        self.sessions
            .lock()
            .map_err(lock_error)?
            .insert(session.session_id.clone(), Arc::downgrade(&session));
        Ok(session)
    }

    pub fn contains(&self, session_id: &str) -> Result<bool, SessionError> {
        let connection = self.connection.lock().map_err(lock_error)?;
        connection
            .query_row(
                "SELECT 1 FROM native_sessions WHERE session_id = ?1",
                [session_id],
                |_| Ok(()),
            )
            .optional()
            .map(|value| value.is_some())
            .map_err(sql_error)
    }

    pub fn records(&self, session_id: &str) -> Result<Vec<SessionRecord>, SessionError> {
        let connection = self.connection.lock().map_err(lock_error)?;
        let mut statement = connection
            .prepare(
                "SELECT record_json FROM native_records
                 WHERE session_id = ?1 ORDER BY seq ASC",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([session_id], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sql_error)?;
        rows.map(|row| {
            let bytes = row.map_err(sql_error)?;
            serde_json::from_slice(&bytes).map_err(SessionError::from)
        })
        .collect()
    }

    // Product retrieval uses the authoritative log, never the compacted live
    // projection. Bound SQL pages and release the connection before blob I/O.
    pub(crate) fn context_page(
        &self, session_id: &str, after: u64, through: Option<u64>,
    ) -> Result<(Vec<SessionRecord>, u64), SessionError> {
        let connection = self.connection.lock().map_err(lock_error)?;
        let high: u64 = connection.query_row(
            "SELECT COALESCE(MAX(seq), 0) FROM native_records WHERE session_id = ?1",
            [session_id], |row| row.get(0),
        ).map_err(sql_error)?;
        let through = through.unwrap_or(high).min(high);
        let mut statement = connection.prepare(
            "SELECT record_json FROM native_records WHERE session_id = ?1 AND seq > ?2 AND seq <= ?3
             AND ((json_extract(CAST(record_json AS TEXT), '$.payload.category') = 'conversation'
                   AND json_extract(CAST(record_json AS TEXT), '$.payload.payload.role') IN ('user','assistant','tool'))
               OR (json_extract(CAST(record_json AS TEXT), '$.payload.category') = 'execution'
                   AND json_extract(CAST(record_json AS TEXT), '$.payload.payload.kind') = 'intent'))
             ORDER BY seq LIMIT 128"
        ).map_err(sql_error)?;
        let rows = statement.query_map(params![session_id, after, through], |row| row.get::<_, Vec<u8>>(0)).map_err(sql_error)?;
        let records = rows.map(|row| serde_json::from_slice(&row.map_err(sql_error)?).map_err(SessionError::from)).collect::<Result<_, _>>()?;
        Ok((records, through))
    }

    pub(crate) fn context_record(
        &self, session_id: &str, record_id: &str,
    ) -> Result<SessionRecord, SessionError> {
        let bytes: Vec<u8> = self.connection.lock().map_err(lock_error)?.query_row(
            "SELECT record_json FROM native_records WHERE session_id = ?1 AND json_extract(CAST(record_json AS TEXT), '$.record_id') = ?2",
            params![session_id, record_id], |row| row.get(0),
        ).optional().map_err(sql_error)?.ok_or_else(|| SessionError::Other("Context record unavailable in this session".into()))?;
        serde_json::from_slice(&bytes).map_err(SessionError::from)
    }

    pub(crate) fn context_value(&self, value: &RecordValue) -> Result<serde_json::Value, SessionError> {
        match value {
            RecordValue::Inline { value } => Ok(value.clone()),
            RecordValue::Blob { reference } => serde_json::from_slice(&read_blob(reference, &self.blob_dir)?).map_err(SessionError::from),
        }
    }

    pub(crate) fn context_tool_source(
        &self, session_id: &str, message: &alva_kernel_abi::Message,
    ) -> Result<(String, String, String), SessionError> {
        // Large agent_message values are content-addressed. Hash the SAME
        // redacted representation used by the writer; do not match by a tool
        // call ID alone (providers may reuse IDs in later turns).
        let mut record = SessionRecord::conversation(AgentMessage::Standard(message.clone()), None);
        redact_record(&mut record);
        let SessionRecordPayload::Conversation(conversation) = record.payload else { unreachable!() };
        let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&conversation.agent_message.unwrap())?));
        let (id, name): (String, String) = self.connection.lock().map_err(lock_error)?.query_row(
            "SELECT json_extract(CAST(c.record_json AS TEXT), '$.record_id'), COALESCE(json_extract(CAST(i.record_json AS TEXT), '$.payload.payload.operation'), '')
             FROM native_records c LEFT JOIN native_records i ON i.session_id = c.session_id
               AND json_extract(CAST(i.record_json AS TEXT), '$.record_id') = json_extract(CAST(c.record_json AS TEXT), '$.parent_record_id')
             WHERE c.session_id = ?1 AND json_extract(CAST(c.record_json AS TEXT), '$.type_name') = 'tool_result'
               AND (json_extract(CAST(c.record_json AS TEXT), '$.payload.payload.agent_message.id') = ?2
                 OR json_extract(CAST(c.record_json AS TEXT), '$.payload.payload.agent_message_blob.sha256') = ?3)
             ORDER BY c.seq DESC LIMIT 1",
            params![session_id, message.id, hash], |row| Ok((row.get(0)?, row.get(1)?)),
        ).optional().map_err(sql_error)?.ok_or_else(|| SessionError::Other("Tool result has no durable context source".into()))?;
        if !self.context_reference_valid(session_id, &id, &hash)? {
            return Err(SessionError::Other("Tool result source identity/content mismatch".into()));
        }
        Ok((id, name, hash))
    }

    pub(crate) fn context_reference_valid(&self, session: &str, id: &str, hash: &str) -> Result<bool, SessionError> {
        let record = self.context_record(session, id)?;
        let SessionRecordPayload::Conversation(conversation) = record.payload else { return Ok(false) };
        if record.type_name != "tool_result" { return Ok(false); }
        let source_hash = if let Some(reference) = conversation.agent_message_blob {
            reference.sha256
        } else if let Some(message) = conversation.agent_message {
            format!("{:x}", Sha256::digest(serde_json::to_vec(&message)?))
        } else { return Ok(false) };
        Ok(source_hash == hash)
    }

    pub fn conversation_excerpt(&self, session_id: &str) -> Result<Option<String>, SessionError> {
        let records = self.records(session_id)?;
        let mut lines = Vec::new();
        let mut chars = 0usize;
        for record in records.iter().rev() {
            let SessionRecordPayload::Conversation(conversation) = &record.payload else {
                continue;
            };
            let texts = match &conversation.content {
                RecordValue::Inline { value } => text_blocks(value),
                RecordValue::Blob { reference } => vec![reference.preview.clone()],
            };
            if texts.is_empty() {
                continue;
            }
            let text = texts.join(" ");
            let preview = text.chars().take(400).collect::<String>();
            let line = format!("[{}] {preview}", conversation.role);
            chars = chars.saturating_add(line.chars().count());
            lines.push(line);
            if chars >= 2_500 || lines.len() >= 8 {
                break;
            }
        }
        lines.reverse();
        Ok((!lines.is_empty()).then(|| lines.join("\n")))
    }

    pub fn delete(&self, session_id: &str) -> Result<(), SessionError> {
        let mut connection = self.connection.lock().map_err(lock_error)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        let session_ids = {
            let mut statement = transaction
                .prepare(
                    "WITH RECURSIVE descendants(session_id) AS (
                       SELECT session_id FROM native_sessions WHERE session_id = ?1
                       UNION ALL
                       SELECT child.session_id FROM native_sessions child
                       JOIN descendants parent ON child.parent_session_id = parent.session_id
                     ) SELECT session_id FROM descendants",
                )
                .map_err(sql_error)?;
            let rows = statement
                .query_map([session_id], |row| row.get::<_, String>(0))
                .map_err(sql_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(sql_error)?;
            rows
        };
        let mut candidate_blobs = std::collections::HashSet::new();
        for native_session_id in &session_ids {
            let mut statement = transaction
                .prepare("SELECT record_json FROM native_records WHERE session_id = ?1")
                .map_err(sql_error)?;
            let rows = statement
                .query_map([native_session_id], |row| row.get::<_, Vec<u8>>(0))
                .map_err(sql_error)?;
            for row in rows {
                let record: SessionRecord = serde_json::from_slice(&row.map_err(sql_error)?)?;
                collect_blob_hashes(&record, &mut candidate_blobs);
            }
            transaction
                .execute(
                    "DELETE FROM native_sessions WHERE session_id = ?1",
                    [native_session_id],
                )
                .map_err(sql_error)?;
        }
        transaction.commit().map_err(sql_error)?;

        for sha256 in candidate_blobs {
            let pattern = format!("%{sha256}%");
            let still_referenced = connection
                .query_row(
                    "SELECT 1 FROM native_records
                      WHERE CAST(record_json AS TEXT) LIKE ?1 LIMIT 1",
                    [pattern],
                    |_| Ok(()),
                )
                .optional()
                .map_err(sql_error)?
                .is_some();
            if !still_referenced {
                let path = self.blob_dir.join(format!("{sha256}.json.zst"));
                match std::fs::remove_file(&path) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => {
                        return Err(SessionError::Other(format!(
                            "delete Inspector blob {}: {error}",
                            path.display()
                        )))
                    }
                }
            }
        }
        self.sessions
            .lock()
            .map_err(lock_error)?
            .retain(|id, _| !session_ids.contains(id));
        Ok(())
    }
}

pub struct SqliteAgentSession {
    session_id: String,
    parent_session_id: Option<String>,
    connection: Arc<Mutex<Connection>>,
    blob_dir: PathBuf,
    sessions: Arc<Mutex<HashMap<String, Weak<SqliteAgentSession>>>>,
    memory: ListenableInMemorySession,
    write_lock: tokio::sync::Mutex<()>,
    dirty: AtomicBool,
    record_listeners: tokio::sync::RwLock<Vec<tokio::sync::mpsc::UnboundedSender<SessionRecord>>>,
}

#[derive(Clone)]
pub struct RepositoryInspector {
    repository: SessionRepository,
}

impl RepositoryInspector {
    pub fn new(repository: SessionRepository) -> Self {
        Self { repository }
    }
}

#[async_trait]
impl EngineInspector for RepositoryInspector {
    fn inspection_capabilities(&self) -> InspectionCapabilities {
        InspectionCapabilities::full()
    }

    async fn snapshot(&self, native_session_id: &str) -> Result<InspectionSnapshot, RuntimeError> {
        if !self
            .repository
            .contains(native_session_id)
            .map_err(|error| RuntimeError::Other(error.to_string()))?
        {
            return Err(RuntimeError::SessionNotFound(native_session_id.to_string()));
        }
        let session = self
            .repository
            .open_session(native_session_id.to_string(), None)
            .await
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        let records = session.records(0).await;
        let children = child_links(native_session_id, &records);
        Ok(InspectionSnapshot {
            engine_id: "alva".to_string(),
            native_session_id: native_session_id.to_string(),
            parent_session_id: session.parent_session_id().map(str::to_string),
            high_water_seq: records.last().map(|record| record.seq).unwrap_or(0),
            records,
            children,
        })
    }

    async fn subscribe(
        &self,
        native_session_id: &str,
        after_seq: u64,
    ) -> Result<InspectionStream, RuntimeError> {
        if !self
            .repository
            .contains(native_session_id)
            .map_err(|error| RuntimeError::Other(error.to_string()))?
        {
            return Err(RuntimeError::SessionNotFound(native_session_id.to_string()));
        }
        let session = self
            .repository
            .open_session(native_session_id.to_string(), None)
            .await
            .map_err(|error| RuntimeError::Other(error.to_string()))?;
        Ok(session.inspection_stream(after_seq).await)
    }

    async fn delete_session(&self, native_session_id: &str) -> Result<(), RuntimeError> {
        self.repository
            .delete(native_session_id)
            .map_err(|error| RuntimeError::Other(error.to_string()))
    }
}

fn child_links(parent_session_id: &str, records: &[SessionRecord]) -> Vec<ChildSessionLink> {
    records
        .iter()
        .filter_map(|record| match &record.payload {
            alva_kernel_abi::SessionRecordPayload::Execution(
                ExecutionRecord::ChildSessionLinked {
                    child_session_id,
                    parent_run_id,
                    spawn_tool_call_id,
                },
            ) => Some(ChildSessionLink {
                child_session_id: child_session_id.clone(),
                parent_session_id: parent_session_id.to_string(),
                parent_run_id: parent_run_id.clone(),
                spawn_tool_call_id: spawn_tool_call_id.clone(),
            }),
            _ => None,
        })
        .collect()
}

impl SqliteAgentSession {
    async fn sync_memory_to_db(&self) -> Result<(), SessionError> {
        let records = self.memory.records(0).await;
        let snapshot = self.memory.load_snapshot().await;
        let prepared: Vec<SessionRecord> = records
            .into_iter()
            .map(|record| self.prepare_record(record))
            .collect::<Result<_, _>>()?;

        let mut connection = self.connection.lock().map_err(lock_error)?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(sql_error)?;
        transaction
            .execute(
                "DELETE FROM native_records WHERE session_id = ?1",
                [&self.session_id],
            )
            .map_err(sql_error)?;
        for record in prepared {
            transaction
                .execute(
                    "INSERT INTO native_records(session_id, seq, record_json) VALUES (?1, ?2, ?3)",
                    params![
                        self.session_id,
                        record.seq as i64,
                        serde_json::to_vec(&record)?
                    ],
                )
                .map_err(sql_error)?;
        }
        transaction
            .execute(
                "UPDATE native_sessions SET snapshot = ?2 WHERE session_id = ?1",
                params![self.session_id, snapshot],
            )
            .map_err(sql_error)?;
        transaction.commit().map_err(sql_error)?;
        self.dirty.store(false, Ordering::Release);
        Ok(())
    }

    fn persisted_records(&self) -> Result<Vec<SessionRecord>, SessionError> {
        let connection = self.connection.lock().map_err(lock_error)?;
        let mut statement = connection
            .prepare(
                "SELECT record_json FROM native_records
                 WHERE session_id = ?1 ORDER BY seq ASC",
            )
            .map_err(sql_error)?;
        let rows = statement
            .query_map([&self.session_id], |row| row.get::<_, Vec<u8>>(0))
            .map_err(sql_error)?;
        rows.map(|row| {
            let bytes = row.map_err(sql_error)?;
            serde_json::from_slice(&bytes).map_err(SessionError::from)
        })
        .collect()
    }

    fn persisted_events(&self) -> Result<Vec<SessionEvent>, SessionError> {
        self.persisted_records().and_then(|records| {
            records
                .into_iter()
                .map(|mut record| {
                    hydrate_record(&mut record, &self.blob_dir)?;
                    Ok(record.into_event())
                })
                .collect()
        })
    }

    fn persist_record(&self, record: SessionRecord) -> Result<SessionRecord, SessionError> {
        let record = self.prepare_record(record)?;
        let bytes = serde_json::to_vec(&record)?;
        let connection = self.connection.lock().map_err(lock_error)?;
        connection
            .execute(
                "INSERT INTO native_records(session_id, seq, record_json) VALUES (?1, ?2, ?3)",
                params![self.session_id, record.seq as i64, bytes],
            )
            .map_err(sql_error)?;
        Ok(record)
    }

    fn prepare_record(&self, mut record: SessionRecord) -> Result<SessionRecord, SessionError> {
        redact_record(&mut record);
        externalize_record_values(&mut record, &self.blob_dir)?;
        Ok(record)
    }

    async fn appended_event(&self, uuid: &str) -> Result<SessionEvent, SessionError> {
        self.memory
            .query(&EventQuery::default())
            .await
            .into_iter()
            .find(|matched| matched.event.uuid == uuid)
            .map(|matched| matched.event)
            .ok_or_else(|| SessionError::Other(format!("committed record {uuid} not found")))
    }

    async fn broadcast_records(&self, records: &[SessionRecord]) {
        let mut listeners = self.record_listeners.write().await;
        listeners.retain(|listener| {
            records
                .iter()
                .all(|record| listener.send(record.clone()).is_ok())
        });
    }

    async fn inspection_stream(&self, after_seq: u64) -> InspectionStream {
        let _guard = self.write_lock.lock().await;
        let initial = self
            .persisted_records()
            .unwrap_or_default()
            .into_iter()
            .filter(move |record| record.seq > after_seq)
            .map(|record| InspectionUpdate::Record {
                record: Box::new(record),
            });
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        self.record_listeners.write().await.push(sender);
        Box::pin(
            tokio_stream::iter(initial).chain(UnboundedReceiverStream::new(receiver).map(
                |record| InspectionUpdate::Record {
                    record: Box::new(record),
                },
            )),
        )
    }
}

#[async_trait]
impl ConversationLog for SqliteAgentSession {
    fn session_id(&self) -> &str {
        &self.session_id
    }
    async fn append(&self, event: SessionEvent) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        let uuid = event.uuid.clone();
        self.memory.append(event).await;
        let persisted = match self.appended_event(&uuid).await {
            Ok(event) => self.persist_record(SessionRecord::from_event(event)),
            Err(error) => Err(error),
        };
        match persisted {
            Ok(record) => self.broadcast_records(&[record]).await,
            Err(error) => {
                self.dirty.store(true, Ordering::Release);
                tracing::error!(session_id = %self.session_id, %error, "persist native session event");
            }
        }
        Ok(())
    }
    async fn append_message(
        &self,
        message: AgentMessage,
        parent_uuid: Option<String>,
    ) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.memory.append_message(message, parent_uuid).await;
        if let Some(event) = self
            .memory
            .query(&EventQuery {
                last_n: Some(1),
                limit: 1,
                ..EventQuery::default()
            })
            .await
            .into_iter()
            .next()
            .map(|matched| matched.event)
        {
            match self.persist_record(SessionRecord::from_event(event)) {
                Ok(record) => self.broadcast_records(&[record]).await,
                Err(error) => {
                    self.dirty.store(true, Ordering::Release);
                    tracing::error!(session_id = %self.session_id, %error, "persist native session message");
                }
            }
        }
        Ok(())
    }
    async fn append_records(
        &self,
        mut records: Vec<SessionRecord>,
    ) -> Result<AppendReceipt, SessionError> {
        if records.is_empty() {
            return Ok(AppendReceipt {
                first_seq: 0,
                last_seq: 0,
                count: 0,
            });
        }
        let _guard = self.write_lock.lock().await;
        if self.dirty.load(Ordering::Acquire) {
            self.sync_memory_to_db().await?;
        }
        // Validate the existing durable projection before committing a batch.
        // After COMMIT there must be no ordinary fallible hydration step that
        // can make the caller believe an already-committed batch failed.
        self.persisted_events()?;
        let (first_seq, committed) = {
            let mut connection = self.connection.lock().map_err(lock_error)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let current: i64 = transaction
                .query_row(
                    "SELECT COALESCE(MAX(seq), 0) FROM native_records WHERE session_id = ?1",
                    [&self.session_id],
                    |row| row.get(0),
                )
                .map_err(sql_error)?;
            let first_seq = current as u64 + 1;
            let mut committed = Vec::with_capacity(records.len());
            for (offset, record) in records.iter_mut().enumerate() {
                record.seq = first_seq + offset as u64;
                let record = self.prepare_record(record.clone())?;
                transaction
                    .execute(
                        "INSERT INTO native_records(session_id, seq, record_json) VALUES (?1, ?2, ?3)",
                        params![
                            self.session_id,
                            record.seq as i64,
                            serde_json::to_vec(&record)?
                        ],
                    )
                    .map_err(sql_error)?;
                committed.push(record);
            }
            transaction.commit().map_err(sql_error)?;
            (first_seq, committed)
        };
        let all = self.persisted_events()?;
        self.memory.restore_events(all).await;
        // R24: 事件重建会把内存的 typed record 投影降级为通用 Event 形态；
        // 用持久层的 typed 记录恢复，否则下一次 flush / dirty-sync 重写 DB
        // 时丢失 ChildSessionLinked 等 typed payload（Inspector 因果链）。
        let typed = self.persisted_records()?;
        self.memory.restore_typed_records(typed).await;
        self.broadcast_records(&committed).await;
        Ok(AppendReceipt {
            first_seq,
            last_seq: first_seq + records.len() as u64 - 1,
            count: records.len(),
        })
    }
    fn supports_atomic_records(&self) -> bool {
        true
    }
    async fn messages(&self) -> Result<Vec<AgentMessage>, SessionError> {
        self.memory.messages().await
    }
    async fn recent_messages(&self, count: usize) -> Result<Vec<AgentMessage>, SessionError> {
        self.memory.recent_messages(count).await
    }
    async fn flush(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        self.sync_memory_to_db().await?;
        let connection = self.connection.lock().map_err(lock_error)?;
        connection
            .execute_batch("PRAGMA wal_checkpoint(PASSIVE);")
            .map_err(sql_error)
    }
}

#[async_trait]
impl SessionStore for SqliteAgentSession {
    fn parent_session_id(&self) -> Option<&str> {
        self.parent_session_id.as_deref()
    }
    async fn create_child_session(
        &self,
        parent_run_id: &str,
        spawn_tool_call_id: &str,
    ) -> Result<Option<Arc<dyn AgentSession>>, SessionError> {
        let child_session_id = uuid::Uuid::new_v4().to_string();
        let repository = SessionRepository {
            path: PathBuf::new(),
            blob_dir: self.blob_dir.clone(),
            connection: Arc::clone(&self.connection),
            sessions: Arc::clone(&self.sessions),
        };
        let child = repository
            .open_session(child_session_id.clone(), Some(self.session_id.clone()))
            .await?;
        let link = SessionRecord::execution(
            "child_session_linked",
            EventEmitter::runtime(),
            ExecutionRecord::ChildSessionLinked {
                child_session_id,
                parent_run_id: parent_run_id.to_string(),
                spawn_tool_call_id: spawn_tool_call_id.to_string(),
            },
        );
        self.append_records(vec![link]).await?;
        Ok(Some(child))
    }
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch> {
        self.memory.query(filter).await
    }
    async fn count(&self, filter: &EventQuery) -> usize {
        self.memory.count(filter).await
    }
    async fn records(&self, from_seq: u64) -> Vec<SessionRecord> {
        self.persisted_records()
            .unwrap_or_default()
            .into_iter()
            .filter(|record| record.seq > from_seq)
            .collect()
    }
    async fn subscribe_events(&self, from_seq: u64) -> SessionEventStream {
        self.memory.subscribe_events(from_seq).await
    }
    async fn rollback_after(&self, uuid: &str) -> usize {
        let _guard = self.write_lock.lock().await;
        let removed = self.memory.rollback_after(uuid).await;
        if let Some(seq) = self
            .memory
            .query(&EventQuery::default())
            .await
            .into_iter()
            .find(|matched| matched.event.uuid == uuid)
            .map(|matched| matched.event.seq)
        {
            match self.connection.lock() {
                Ok(connection) => {
                    if let Err(error) = connection.execute(
                        "DELETE FROM native_records WHERE session_id = ?1 AND seq > ?2",
                        params![self.session_id, seq as i64],
                    ) {
                        self.dirty.store(true, Ordering::Release);
                        tracing::error!(session_id = %self.session_id, %error, "rollback native session records");
                    }
                }
                Err(_) => self.dirty.store(true, Ordering::Release),
            }
        }
        removed
    }
    async fn save_snapshot(&self, data: &[u8]) {
        self.memory.save_snapshot(data).await;
        match self.connection.lock() {
            Ok(connection) => {
                if let Err(error) = connection.execute(
                    "UPDATE native_sessions SET snapshot = ?2 WHERE session_id = ?1",
                    params![self.session_id, data],
                ) {
                    self.dirty.store(true, Ordering::Release);
                    tracing::error!(session_id = %self.session_id, %error, "persist native session snapshot");
                }
            }
            Err(_) => self.dirty.store(true, Ordering::Release),
        }
    }
    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.connection.lock().ok().and_then(|connection| {
            connection
                .query_row(
                    "SELECT snapshot FROM native_sessions WHERE session_id = ?1",
                    [&self.session_id],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten()
                .flatten()
        })
    }
    async fn restore(&self) -> Result<(), SessionError> {
        self.memory.restore_events(self.persisted_events()?).await;
        Ok(())
    }
    async fn close(&self) -> Result<(), SessionError> {
        self.flush().await
    }
    async fn clear(&self) -> Result<(), SessionError> {
        let _guard = self.write_lock.lock().await;
        let repository = SessionRepository {
            path: PathBuf::new(),
            blob_dir: self.blob_dir.clone(),
            connection: Arc::clone(&self.connection),
            sessions: Arc::clone(&self.sessions),
        };
        repository.delete(&self.session_id)?;
        self.memory.clear().await
    }
    async fn compact_messages(&self, replacement: Vec<AgentMessage>) {
        let _guard = self.write_lock.lock().await;
        let high_water = self
            .persisted_records()
            .ok()
            .and_then(|records| records.last().map(|record| record.seq))
            .unwrap_or(0);
        self.memory.compact_messages(replacement).await;
        let records = self.memory.records(high_water).await;
        if records.is_empty() {
            return;
        }
        let committed = (|| -> Result<Vec<SessionRecord>, SessionError> {
            let mut connection = self.connection.lock().map_err(lock_error)?;
            let transaction = connection
                .transaction_with_behavior(TransactionBehavior::Immediate)
                .map_err(sql_error)?;
            let mut committed = Vec::with_capacity(records.len());
            for record in records {
                let record = self.prepare_record(record)?;
                transaction
                    .execute(
                        "INSERT INTO native_records(session_id, seq, record_json) VALUES (?1, ?2, ?3)",
                        params![
                            self.session_id,
                            record.seq as i64,
                            serde_json::to_vec(&record)?
                        ],
                    )
                    .map_err(sql_error)?;
                committed.push(record);
            }
            transaction.commit().map_err(sql_error)?;
            Ok(committed)
        })();
        match committed {
            Ok(records) => self.broadcast_records(&records).await,
            Err(error) => {
                self.dirty.store(true, Ordering::Release);
                tracing::error!(
                    session_id = %self.session_id,
                    %error,
                    "persist compacted native session projection"
                );
            }
        }
    }
}

impl AgentSession for SqliteAgentSession {}

fn lock_error<T>(_: std::sync::PoisonError<T>) -> SessionError {
    SessionError::Other("native session store lock poisoned".to_string())
}

fn sql_error(error: rusqlite::Error) -> SessionError {
    SessionError::Other(format!("native session SQLite error: {error}"))
}

fn redact_record(record: &mut SessionRecord) {
    match &mut record.payload {
        SessionRecordPayload::Conversation(conversation) => {
            redact_record_value(&mut conversation.content);
            if let Some(message) = &mut conversation.agent_message {
                if let Ok(mut value) = serde_json::to_value(&*message) {
                    redact_json(&mut value, None);
                    if let Ok(redacted) = serde_json::from_value(value) {
                        *message = redacted;
                    }
                }
            }
        }
        SessionRecordPayload::Execution(execution) => match execution {
            ExecutionRecord::Event { data } => redact_json(data, None),
            ExecutionRecord::Intent { input, .. } => redact_record_value(input),
            ExecutionRecord::Outcome { output, .. } => redact_record_value(output),
            ExecutionRecord::ChildSessionLinked { .. }
            | ExecutionRecord::ConversationHeadMoved { .. } => {}
        },
        SessionRecordPayload::Extension(extension) => redact_json(&mut extension.payload, None),
    }
}

fn redact_record_value(value: &mut RecordValue) {
    if let RecordValue::Inline { value } = value {
        redact_json(value, None);
    }
}

fn redact_json(value: &mut serde_json::Value, key: Option<&str>) {
    if key.is_some_and(is_secret_key) {
        *value = serde_json::Value::String("[REDACTED]".to_string());
        return;
    }
    match value {
        serde_json::Value::Object(object) => {
            for (key, value) in object {
                redact_json(value, Some(key));
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                redact_json(value, None);
            }
        }
        serde_json::Value::String(text) if contains_inline_secret(text) => {
            *text = "[REDACTED]".to_string();
        }
        _ => {}
    }
}

fn is_secret_key(key: &str) -> bool {
    let normalized = key.to_ascii_lowercase().replace(['-', '.'], "_");
    normalized == "authorization"
        || normalized == "cookie"
        || normalized == "set_cookie"
        || normalized.contains("api_key")
        || normalized.contains("access_token")
        || normalized.contains("refresh_token")
        || normalized.contains("client_secret")
        || normalized.ends_with("_password")
        || normalized == "password"
        || normalized.ends_with("_secret")
}

fn contains_inline_secret(text: &str) -> bool {
    let normalized = text.to_ascii_lowercase();
    [
        "authorization: bearer ",
        "authorization=bearer ",
        "api_key=",
        "apikey=",
        "access_token=",
        "client_secret=",
    ]
    .iter()
    .any(|needle| normalized.contains(needle))
}

fn externalize_record_values(
    record: &mut SessionRecord,
    blob_dir: &Path,
) -> Result<(), SessionError> {
    match &mut record.payload {
        SessionRecordPayload::Conversation(conversation) => {
            externalize_value(&mut conversation.content, blob_dir)?;
            if let Some(message) = &conversation.agent_message {
                let bytes = serde_json::to_vec(message)?;
                if bytes.len() > INLINE_RECORD_LIMIT {
                    conversation.agent_message_blob = Some(write_blob(&bytes, blob_dir)?);
                    conversation.agent_message = None;
                }
            }
        }
        SessionRecordPayload::Execution(ExecutionRecord::Intent { input, .. }) => {
            externalize_value(input, blob_dir)?;
        }
        SessionRecordPayload::Execution(ExecutionRecord::Outcome { output, .. }) => {
            externalize_value(output, blob_dir)?;
        }
        _ => {}
    }
    Ok(())
}

fn externalize_value(value: &mut RecordValue, blob_dir: &Path) -> Result<(), SessionError> {
    let RecordValue::Inline { value: inline } = value else {
        return Ok(());
    };
    let bytes = serde_json::to_vec(inline)?;
    if bytes.len() <= INLINE_RECORD_LIMIT {
        return Ok(());
    }
    let reference = write_blob(&bytes, blob_dir)?;
    *value = RecordValue::Blob { reference };
    Ok(())
}

fn write_blob(bytes: &[u8], blob_dir: &Path) -> Result<BlobReference, SessionError> {
    let digest = Sha256::digest(bytes);
    let sha256 = digest
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let path = blob_dir.join(format!("{sha256}.json.zst"));
    if !path.exists() {
        let compressed = zstd::stream::encode_all(bytes, 3)
            .map_err(|error| SessionError::Other(format!("compress Inspector blob: {error}")))?;
        let temporary = blob_dir.join(format!(".{sha256}.tmp"));
        std::fs::write(&temporary, compressed).map_err(|error| {
            SessionError::Other(format!(
                "write Inspector blob {}: {error}",
                temporary.display()
            ))
        })?;
        std::fs::rename(&temporary, &path).map_err(|error| {
            SessionError::Other(format!(
                "publish Inspector blob {}: {error}",
                path.display()
            ))
        })?;
    }
    let preview = String::from_utf8_lossy(bytes).chars().take(256).collect();
    Ok(BlobReference {
        sha256,
        media_type: "application/json".to_string(),
        size_bytes: bytes.len() as u64,
        preview,
    })
}

fn hydrate_record(record: &mut SessionRecord, blob_dir: &Path) -> Result<(), SessionError> {
    let SessionRecordPayload::Conversation(conversation) = &mut record.payload else {
        return Ok(());
    };
    if let RecordValue::Blob { reference } = &conversation.content {
        conversation.content = RecordValue::Inline {
            value: serde_json::from_slice(&read_blob(reference, blob_dir)?)?,
        };
    }
    if let Some(reference) = &conversation.agent_message_blob {
        conversation.agent_message =
            Some(serde_json::from_slice(&read_blob(reference, blob_dir)?)?);
    }
    Ok(())
}

fn read_blob(reference: &BlobReference, blob_dir: &Path) -> Result<Vec<u8>, SessionError> {
    use std::io::Read;
    const MAX_BLOB: u64 = 64 * 1024 * 1024;
    if reference.sha256.len() != 64 || !reference.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        || reference.size_bytes > MAX_BLOB {
        return Err(SessionError::Other("Invalid or oversized context blob reference".into()));
    }
    let path = blob_dir.join(format!("{}.json.zst", reference.sha256));
    let file = std::fs::File::open(&path).map_err(|error| {
        SessionError::Other(format!("read Inspector blob {}: {error}", path.display()))
    })?;
    let mut compressed = Vec::new();
    const MAX_COMPRESSED: u64 = MAX_BLOB + 1024 * 1024;
    file.take(MAX_COMPRESSED + 1).read_to_end(&mut compressed)
        .map_err(|_| SessionError::Other("Context compressed blob read failed".into()))?;
    if compressed.len() as u64 > MAX_COMPRESSED {
        return Err(SessionError::Other("Context compressed blob exceeds read budget".into()));
    }
    let mut bytes = Vec::new();
    zstd::stream::read::Decoder::new(compressed.as_slice())
        .map_err(|_| SessionError::Other("Context blob decoder unavailable".into()))?
        .take(MAX_BLOB + 1).read_to_end(&mut bytes)
        .map_err(|_| SessionError::Other("Context blob decoding failed".into()))?;
    if bytes.len() as u64 != reference.size_bytes || bytes.len() as u64 > MAX_BLOB {
        return Err(SessionError::Other("Context blob size mismatch".into()));
    }
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if digest != reference.sha256 {
        return Err(SessionError::Other(format!(
            "Inspector blob hash mismatch for {}",
            reference.sha256
        )));
    }
    Ok(bytes)
}

fn collect_blob_hashes(record: &SessionRecord, hashes: &mut std::collections::HashSet<String>) {
    fn collect_value(value: &RecordValue, hashes: &mut std::collections::HashSet<String>) {
        if let RecordValue::Blob { reference } = value {
            hashes.insert(reference.sha256.clone());
        }
    }
    match &record.payload {
        SessionRecordPayload::Conversation(conversation) => {
            collect_value(&conversation.content, hashes);
            if let Some(reference) = &conversation.agent_message_blob {
                hashes.insert(reference.sha256.clone());
            }
        }
        SessionRecordPayload::Execution(ExecutionRecord::Intent { input, .. }) => {
            collect_value(input, hashes)
        }
        SessionRecordPayload::Execution(ExecutionRecord::Outcome { output, .. }) => {
            collect_value(output, hashes)
        }
        _ => {}
    }
}

fn text_blocks(value: &serde_json::Value) -> Vec<String> {
    let Some(content) = value.as_array() else {
        return Vec::new();
    };
    content
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .and_then(serde_json::Value::as_str)
                .or_else(|| {
                    block
                        .get("Text")
                        .and_then(|text| text.get("text"))
                        .and_then(serde_json::Value::as_str)
                })
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{EventEmitter, ExecutionRecord, SessionRecordPayload};

    /// R24: typed `ChildSessionLinked` payloads must survive the
    /// append_records → plain event append → flush → reopen round trip.
    ///
    /// flush() rewrites the whole table from the in-memory mirror; the
    /// post-commit refresh used to rebuild that mirror from EVENTS, which
    /// degrades typed records to generic `ExecutionRecord::Event` — every
    /// causal link then vanished from the durable projection at turn end and
    /// the recursive Inspector traversal broke after restart.
    #[tokio::test]
    async fn typed_child_links_survive_flush_and_repository_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("records.db");
        let repository = SessionRepository::open(&path).unwrap();
        let root = repository.open_session("root", None).await.unwrap();
        let child = root
            .create_child_session("run-1", "spawn-1")
            .await
            .unwrap()
            .unwrap();
        let child_id = child.session_id().to_string();

        // The same sequence a real run performs: typed batch commits during
        // tool execution, skeleton events during the loop, flush at turn end.
        root.append(alva_kernel_abi::SessionEvent::new_runtime("iteration_start"))
            .await
            .unwrap();
        root.flush().await.unwrap();
        drop(root);
        drop(repository);

        let reopened = SessionRepository::open(&path).unwrap();
        let root = reopened.open_session("root", None).await.unwrap();
        let links: Vec<String> = root
            .records(0)
            .await
            .iter()
            .filter_map(|record| {
                if let SessionRecordPayload::Execution(ExecutionRecord::ChildSessionLinked {
                    child_session_id,
                    ..
                }) = &record.payload
                {
                    Some(child_session_id.clone())
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(
            links,
            vec![child_id.clone()],
            "typed causal link degraded to a generic event by flush"
        );
        assert!(reopened.contains(&child_id).unwrap());
    }

    #[tokio::test]
    async fn atomic_records_survive_repository_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("records.db");
        let repository = SessionRepository::open(&path).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        let records = ["intent", "outcome"]
            .into_iter()
            .map(|type_name| {
                SessionRecord::execution(
                    type_name,
                    EventEmitter::runtime(),
                    ExecutionRecord::Event {
                        data: serde_json::json!({}),
                    },
                )
            })
            .collect();
        let receipt = session.append_records(records).await.unwrap();
        assert_eq!(receipt.count, 2);
        drop(session);
        drop(repository);

        let repository = SessionRepository::open(&path).unwrap();
        assert_eq!(repository.records("s1").unwrap().len(), 2);
        let restored = repository.open_session("s1", None).await.unwrap();
        assert_eq!(restored.records(0).await.len(), 2);
    }

    #[tokio::test]
    async fn durable_records_are_redacted_and_large_values_use_compressed_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("records.db");
        let repository = SessionRepository::open(&path).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        let secret = "super-secret-token";
        let record = SessionRecord::execution(
            "tool_intent",
            EventEmitter::runtime(),
            ExecutionRecord::Intent {
                operation_id: "op-1".to_string(),
                operation: "upload".to_string(),
                input: RecordValue::Inline {
                    value: serde_json::json!({
                        "authorization": secret,
                        "payload": "x".repeat(INLINE_RECORD_LIMIT + 1),
                    }),
                },
            },
        );
        session.append_records(vec![record]).await.unwrap();

        let disk = std::fs::read(&path).unwrap();
        assert!(!String::from_utf8_lossy(&disk).contains(secret));
        let records = session.records(0).await;
        let reference = match &records[0].payload {
            SessionRecordPayload::Execution(ExecutionRecord::Intent {
                input: RecordValue::Blob { reference },
                ..
            }) => reference,
            payload => panic!("expected blob-backed intent, got {payload:?}"),
        };
        assert!(!reference.preview.contains(secret));
        let blob = repository
            .blob_dir
            .join(format!("{}.json.zst", reference.sha256));
        let decompressed = zstd::stream::decode_all(std::fs::File::open(blob).unwrap()).unwrap();
        assert!(!String::from_utf8_lossy(&decompressed).contains(secret));
        assert!(String::from_utf8_lossy(&decompressed).contains("[REDACTED]"));
    }

    #[tokio::test]
    async fn repository_inspector_tails_the_active_typed_session() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        let inspector = RepositoryInspector::new(repository.clone());
        let mut stream = inspector.subscribe("s1", 0).await.unwrap();

        session
            .append_records(vec![SessionRecord::execution(
                "tool_intent",
                EventEmitter::runtime(),
                ExecutionRecord::Intent {
                    operation_id: "op-1".to_string(),
                    operation: "read".to_string(),
                    input: RecordValue::Inline {
                        value: serde_json::json!({"path": "README.md"}),
                    },
                },
            )])
            .await
            .unwrap();

        let update = tokio::time::timeout(std::time::Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .unwrap();
        let InspectionUpdate::Record { record } = update else {
            panic!("expected record update");
        };
        assert!(matches!(
            record.payload,
            SessionRecordPayload::Execution(ExecutionRecord::Intent { .. })
        ));
    }

    #[tokio::test]
    async fn repository_inspector_never_creates_a_missing_session_when_subscribing() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let inspector = RepositoryInspector::new(repository.clone());

        let error = match inspector.subscribe("missing", 0).await {
            Ok(_) => panic!("missing Inspector session must not create a subscription"),
            Err(error) => error,
        };

        assert!(matches!(error, RuntimeError::SessionNotFound(session) if session == "missing"));
        assert!(!repository.contains("missing").unwrap());
    }

    #[tokio::test]
    async fn large_conversation_restores_from_blob_without_inline_duplication() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        let text = "response ".repeat(INLINE_RECORD_LIMIT / 4);
        let message = alva_kernel_abi::AgentMessage::Standard(alva_kernel_abi::Message {
            id: "message-1".to_string(),
            role: alva_kernel_abi::MessageRole::Assistant,
            content: vec![alva_kernel_abi::ContentBlock::Text { text: text.clone() }],
            tool_call_id: None,
            usage: None,
            timestamp: 1,
        });
        session
            .append_records(vec![SessionRecord::conversation(message, None)])
            .await
            .unwrap();

        let records = session.records(0).await;
        match &records[0].payload {
            SessionRecordPayload::Conversation(conversation) => {
                assert!(conversation.agent_message.is_none());
                assert!(conversation.agent_message_blob.is_some());
                assert!(matches!(conversation.content, RecordValue::Blob { .. }));
            }
            payload => panic!("expected conversation record, got {payload:?}"),
        }
        let messages = session.messages().await.unwrap();
        assert_eq!(messages.len(), 1);
        let restored = match &messages[0] {
            alva_kernel_abi::AgentMessage::Standard(message) => message.text_content(),
            _ => String::new(),
        };
        assert_eq!(restored, text);
        assert!(repository
            .conversation_excerpt("s1")
            .unwrap()
            .unwrap()
            .contains("response"));
    }

    #[tokio::test]
    async fn compacted_message_projection_survives_repository_reopen() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("records.db");
        let repository = SessionRepository::open(&path).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        let message = |id: &str, text: &str| {
            AgentMessage::Standard(alva_kernel_abi::Message {
                id: id.to_string(),
                role: alva_kernel_abi::MessageRole::User,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: text.to_string(),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 1,
            })
        };
        session.append_message(message("old", "old"), None).await;
        session
            .append(SessionEvent::new_runtime("compaction"))
            .await;
        session
            .compact_messages(vec![message("summary", "summary")])
            .await;
        drop(session);
        drop(repository);

        let repository = SessionRepository::open(&path).unwrap();
        let restored = repository.open_session("s1", None).await.unwrap();
        let messages = restored.messages().await.unwrap();

        assert_eq!(messages.len(), 1);
        let AgentMessage::Standard(message) = &messages[0] else {
            panic!("compacted projection must restore a standard message");
        };
        assert_eq!(message.text_content(), "summary");
    }

    #[tokio::test]
    async fn flush_repairs_a_transient_infallible_append_failure() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let session = repository.open_session("s1", None).await.unwrap();
        {
            let connection = repository.connection.lock().unwrap();
            connection.execute("DROP TABLE native_records", []).unwrap();
        }

        session
            .append(SessionEvent::new_runtime("transient_failure"))
            .await;
        assert!(session.dirty.load(Ordering::Acquire));

        {
            let connection = repository.connection.lock().unwrap();
            connection
                .execute_batch(
                    "CREATE TABLE native_records (
                        session_id TEXT NOT NULL REFERENCES native_sessions(session_id) ON DELETE CASCADE,
                        seq INTEGER NOT NULL,
                        record_json BLOB NOT NULL,
                        PRIMARY KEY(session_id, seq)
                    );",
                )
                .unwrap();
        }

        session.flush().await.unwrap();
        assert!(!session.dirty.load(Ordering::Acquire));
        assert_eq!(repository.records("s1").unwrap().len(), 1);
    }

    #[tokio::test]
    async fn deleting_a_root_recursively_removes_children_and_unreferenced_blobs() {
        let directory = tempfile::tempdir().unwrap();
        let repository = SessionRepository::open(directory.path().join("records.db")).unwrap();
        let root = repository.open_session("root", None).await.unwrap();
        let child = root
            .create_child_session("run-1", "spawn-1")
            .await
            .unwrap()
            .unwrap();
        let child_id = child.session_id().to_string();
        child
            .append_records(vec![SessionRecord::execution(
                "large",
                EventEmitter::runtime(),
                ExecutionRecord::Outcome {
                    operation_id: "op-1".to_string(),
                    success: true,
                    output: RecordValue::Inline {
                        value: serde_json::json!({"value": "x".repeat(INLINE_RECORD_LIMIT + 1)}),
                    },
                },
            )])
            .await
            .unwrap();
        assert!(std::fs::read_dir(&repository.blob_dir)
            .unwrap()
            .next()
            .is_some());

        root.clear().await.unwrap();

        assert!(!repository.contains("root").unwrap());
        assert!(!repository.contains(&child_id).unwrap());
        assert!(std::fs::read_dir(&repository.blob_dir)
            .unwrap()
            .next()
            .is_none());
    }
}
