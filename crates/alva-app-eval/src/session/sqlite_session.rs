// INPUT:  rusqlite, alva_kernel_abi::agent_session, tokio::task::spawn_blocking
// OUTPUT: SqliteEvalSession
// POS:    Eval-private AgentSession backend that stores events in SQLite.
//
//         Strategy: deferred flush.
//         All writes (append, append_message, rollback_after, save_snapshot, clear)
//         are delegated to an inner InMemoryAgentSession.  Nothing is written to
//         SQLite until flush() / close() is called, at which point the full
//         in-memory event log is bulk-inserted (DELETE existing + INSERT all).
//
//         This is efficient for eval: the user only needs the persisted record
//         after the run finishes, not mid-run.  Live progress is served from
//         the mpsc channel map, not the session.

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{params, Connection};

use alva_kernel_abi::agent_session::{
    AgentSession, EventMatch, EventQuery, InMemoryAgentSession, SessionError, SessionEvent,
    EventEmitter,
};
use alva_kernel_abi::AgentMessage;

/// Eval-private `AgentSession` backend backed by a shared SQLite connection.
///
/// In-memory state is held by an inner `InMemoryAgentSession`. Events are
/// persisted to SQLite only on `flush()` / `close()`.
pub struct SqliteEvalSession {
    /// Shared connection — the same `Connection` is used by the manager and
    /// all sessions. Protected by a blocking Mutex because rusqlite is sync
    /// and all SQL runs inside `spawn_blocking`.
    conn: Arc<Mutex<Connection>>,
    inner: InMemoryAgentSession,
}

impl SqliteEvalSession {
    /// Create a fresh session with a random session_id.
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self {
            conn,
            inner: InMemoryAgentSession::new(),
        }
    }

    /// Create a session with a specific id (used when loading from DB).
    pub fn with_id(conn: Arc<Mutex<Connection>>, session_id: String) -> Self {
        Self {
            conn,
            inner: InMemoryAgentSession::with_id(session_id),
        }
    }

    /// Ensure a row exists in the `sessions` table for this session.
    async fn ensure_session_row(&self) {
        let conn = self.conn.clone();
        let session_id = self.inner.session_id().to_string();
        let parent = self.inner.parent_session_id().map(String::from);
        let now = chrono::Utc::now().timestamp_millis();
        let _ = tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let conn = conn.lock().unwrap();
            conn.execute(
                "INSERT OR IGNORE INTO sessions (session_id, parent_session_id, created_at)
                 VALUES (?1, ?2, ?3)",
                params![session_id, parent, now],
            )?;
            Ok(())
        })
        .await;
    }

    /// Persist the full in-memory state to SQLite in a single transaction.
    /// Replaces any existing rows for this session (DELETE + bulk INSERT).
    async fn persist_to_db(&self) -> Result<(), SessionError> {
        self.ensure_session_row().await;

        // Pull all events from inner.
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
        let conn = self.conn.clone();
        let session_id = self.inner.session_id().to_string();

        tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let conn = conn.lock().unwrap();

            // Replace strategy: delete existing then bulk insert.
            conn.execute(
                "DELETE FROM events WHERE session_id = ?1",
                params![session_id],
            )?;

            // Use an unchecked_transaction for performance. We're inside
            // spawn_blocking so it's safe to hold the transaction until commit.
            let tx = conn.unchecked_transaction()?;
            for event in &events {
                let emitter_json =
                    serde_json::to_string(&event.emitter).unwrap_or_default();
                let message_json = event
                    .message
                    .as_ref()
                    .and_then(|m| serde_json::to_string(m).ok());
                let data_json = event
                    .data
                    .as_ref()
                    .and_then(|d| serde_json::to_string(d).ok());
                tx.execute(
                    "INSERT INTO events
                         (session_id, seq, uuid, parent_uuid, timestamp, event_type,
                          emitter_json, message_json, data_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                    params![
                        session_id,
                        event.seq as i64,
                        event.uuid,
                        event.parent_uuid,
                        event.timestamp,
                        event.event_type,
                        emitter_json,
                        message_json,
                        data_json,
                    ],
                )?;
            }

            if let Some(snap) = snapshot {
                let now = chrono::Utc::now().timestamp_millis();
                tx.execute(
                    "INSERT OR REPLACE INTO snapshots (session_id, data, updated_at)
                     VALUES (?1, ?2, ?3)",
                    params![session_id, snap, now],
                )?;
            }

            tx.commit()?;
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join error: {}", e)))?
        .map_err(|e| SessionError::Other(format!("SQLite error: {}", e)))?;

        Ok(())
    }

    /// Restore events and snapshot from SQLite into the inner session.
    async fn load_from_db(&self) -> Result<(), SessionError> {
        let conn = self.conn.clone();
        let session_id = self.inner.session_id().to_string();

        let (events, snapshot) = tokio::task::spawn_blocking(
            move || -> rusqlite::Result<(Vec<SessionEvent>, Option<Vec<u8>>)> {
                let conn = conn.lock().unwrap();

                // Load events ordered by seq.
                let mut stmt = conn.prepare(
                    "SELECT seq, uuid, parent_uuid, timestamp, event_type,
                            emitter_json, message_json, data_json
                     FROM events
                     WHERE session_id = ?1
                     ORDER BY seq",
                )?;

                let rows = stmt.query_map(params![session_id], |row| {
                    let seq: i64 = row.get(0)?;
                    let uuid: String = row.get(1)?;
                    let parent_uuid: Option<String> = row.get(2)?;
                    let timestamp: i64 = row.get(3)?;
                    let event_type: String = row.get(4)?;
                    let emitter_json: String = row.get(5)?;
                    let message_json: Option<String> = row.get(6)?;
                    let data_json: Option<String> = row.get(7)?;

                    // Deserialize emitter; fall back to runtime emitter on error.
                    let emitter: EventEmitter =
                        serde_json::from_str(&emitter_json)
                            .unwrap_or_else(|_| EventEmitter::runtime());

                    let message = message_json.as_deref().and_then(|s| {
                        serde_json::from_str(s).ok()
                    });

                    let data: Option<serde_json::Value> = data_json
                        .as_deref()
                        .and_then(|s| serde_json::from_str(s).ok());

                    Ok(SessionEvent {
                        seq: seq as u64,
                        uuid,
                        parent_uuid,
                        timestamp,
                        event_type,
                        emitter,
                        message,
                        data,
                    })
                })?;

                let mut events = Vec::new();
                for row in rows {
                    events.push(row?);
                }

                // Load snapshot if present.
                let snapshot: Option<Vec<u8>> = conn
                    .query_row(
                        "SELECT data FROM snapshots WHERE session_id = ?1",
                        params![session_id],
                        |row| row.get(0),
                    )
                    .ok();

                Ok((events, snapshot))
            },
        )
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join error: {}", e)))?
        .map_err(|e| SessionError::Other(format!("SQLite error: {}", e)))?;

        // Replay events into the inner session so the message cache is rebuilt.
        for event in events {
            self.inner.append(event).await;
        }

        if let Some(snap) = snapshot {
            self.inner.save_snapshot(&snap).await;
        }

        Ok(())
    }
}

#[async_trait]
impl AgentSession for SqliteEvalSession {
    fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    fn parent_session_id(&self) -> Option<&str> {
        self.inner.parent_session_id()
    }

    // ----- Write (deferred: inner only, no DB until flush) -----

    async fn append(&self, event: SessionEvent) {
        self.inner.append(event).await;
    }

    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>) {
        self.inner.append_message(msg, parent_uuid).await;
    }

    // ----- Read (delegate to inner) -----

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

    // ----- Write correction (deferred: inner only) -----

    async fn rollback_after(&self, uuid: &str) -> usize {
        self.inner.rollback_after(uuid).await
    }

    async fn save_snapshot(&self, data: &[u8]) {
        self.inner.save_snapshot(data).await;
    }

    async fn load_snapshot(&self) -> Option<Vec<u8>> {
        self.inner.load_snapshot().await
    }

    // ----- Lifecycle -----

    async fn restore(&self) -> Result<(), SessionError> {
        self.load_from_db().await
    }

    /// Bulk-persist all in-memory events to SQLite.
    async fn flush(&self) -> Result<(), SessionError> {
        self.persist_to_db().await
    }

    /// Flush then release (nothing extra to release since connection is shared).
    async fn close(&self) -> Result<(), SessionError> {
        self.persist_to_db().await
    }

    /// Clear in-memory state and delete all DB rows for this session.
    async fn clear(&self) -> Result<(), SessionError> {
        self.inner.clear().await?;

        let conn = self.conn.clone();
        let session_id = self.inner.session_id().to_string();
        tokio::task::spawn_blocking(move || -> rusqlite::Result<()> {
            let conn = conn.lock().unwrap();
            // CASCADE delete handles events + snapshots.
            conn.execute(
                "DELETE FROM sessions WHERE session_id = ?1",
                params![session_id],
            )?;
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join error: {}", e)))?
        .map_err(|e| SessionError::Other(format!("SQLite error: {}", e)))?;

        Ok(())
    }
}
