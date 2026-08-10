// INPUT:  rusqlite, async_trait, serde_json, alva_kernel_abi::agent_session,
//         alva_app_core::session_registry::*
// OUTPUT: SqliteSessionRegistry
// POS:    SessionRegistry trait implementation backed by the same SQLite
//         `sessions` table that SqliteEvalSession writes events to. Stores
//         the SessionRegistry-shaped fields (status / agent_id / title /
//         metadata / stats / usage / timestamps / archived_at) as extra
//         columns alongside the legacy SqliteEvalSessionManager fields.
//
//! SqliteSessionRegistry
//!
//! Persistent SessionRegistry implementation for the Tauri app. Without
//! this, the session list / workspace mapping / titles / metadata
//! evaporate on app restart even though each session's event log is
//! safely persisted — see `mod.rs` doc for context.
//!
//! Reuses the same shared `Arc<Mutex<Connection>>` as `SqliteEvalSession`
//! so reads + writes happen against one connection (WAL-friendly). The
//! `sessions` table is augmented with SessionRegistry-shaped columns
//! (`status`, `agent_id`, `title`, `metadata_json`, `updated_at`,
//! `archived_at`, `stats_json`, `usage_json`); see `schema.rs`.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use rusqlite::{params, Connection, OptionalExtension, Row};
use serde::Serialize;

use alva_kernel_core::agent_session::{AgentSession, SessionError};

use alva_app_core::session_registry::{
    SessionFilter, SessionMetadata, SessionMetadataPatch, SessionOrder, SessionPage,
    SessionRegistry, SessionStatus, ThreadStats, ThreadUsage,
};

use super::sqlite_session::SqliteEvalSession;

pub struct SqliteSessionRegistry {
    conn: Arc<Mutex<Connection>>,
}

impl SqliteSessionRegistry {
    pub fn new(conn: Arc<Mutex<Connection>>) -> Self {
        Self { conn }
    }
}

// ─── Row decode helpers ───────────────────────────────────────────────

/// Column order this module always SELECTs in. Centralised so `decode_row`
/// and every SQL statement agree on the indices.
const META_COLUMNS: &str = "
    session_id, parent_session_id, status, agent_id, title,
    metadata_json, created_at, updated_at, archived_at, stats_json, usage_json
";

fn decode_row(row: &Row<'_>) -> rusqlite::Result<SessionMetadata> {
    let session_id: String = row.get(0)?;
    let parent_session_id: Option<String> = row.get(1)?;
    let status_str: String = row.get(2)?;
    let agent_id: Option<String> = row.get(3)?;
    let title: Option<String> = row.get(4)?;
    let metadata_json: String = row.get(5)?;
    let created_at: i64 = row.get(6)?;
    let updated_at: i64 = row.get(7)?;
    let archived_at: Option<i64> = row.get(8)?;
    let stats_json: String = row.get(9)?;
    let usage_json: String = row.get(10)?;

    let status = parse_status(&status_str);
    let metadata: BTreeMap<String, String> =
        serde_json::from_str(&metadata_json).unwrap_or_default();
    let stats: ThreadStats = serde_json::from_str(&stats_json).unwrap_or_default();
    let usage: ThreadUsage = serde_json::from_str(&usage_json).unwrap_or_default();

    Ok(SessionMetadata {
        session_id,
        parent_session_id,
        status,
        agent_id,
        title,
        metadata,
        created_at,
        updated_at,
        archived_at,
        stats,
        usage,
        session_group_id: None,
        depth: None,
    })
}

/// Map a serialized SessionStatus tag back to the enum. Defaults to `Idle`
/// on unknown values rather than panicking — a forward-compat row written
/// by a newer build should still read back something sensible.
fn parse_status(s: &str) -> SessionStatus {
    match s {
        "running" => SessionStatus::Running,
        "rescheduling" => SessionStatus::Rescheduling,
        "terminated" => SessionStatus::Terminated,
        _ => SessionStatus::Idle,
    }
}

fn json_or_empty<T: Serialize>(v: &T) -> String {
    serde_json::to_string(v).unwrap_or_else(|_| "{}".to_string())
}

// ─── SessionRegistry impl ─────────────────────────────────────────────

#[async_trait]
impl SessionRegistry for SqliteSessionRegistry {
    async fn insert(
        &self,
        _session: Arc<dyn AgentSession>,
        meta: SessionMetadata,
    ) -> Result<(), SessionError> {
        // The `session: Arc<dyn AgentSession>` argument is unused for
        // persistent backends — the AgentSession handle is materialised
        // lazily on `get` from the event log. We only persist metadata
        // here. See the trait doc on "Persistent backends".
        let conn = self.conn.clone();
        let metadata_json = json_or_empty(&meta.metadata);
        let stats_json = json_or_empty(&meta.stats);
        let usage_json = json_or_empty(&meta.usage);
        let status_str = meta.status.as_str().to_string();
        let session_id = meta.session_id.clone();

        let result = tokio::task::spawn_blocking(move || -> Result<(), SessionError> {
            let conn = conn.lock().unwrap();
            // Pre-check for duplicate — trait contract says insert MUST
            // fail on existing id (callers can `get` first to upsert).
            let exists: bool = conn
                .query_row(
                    "SELECT 1 FROM sessions WHERE session_id = ?1",
                    params![session_id],
                    |_| Ok(true),
                )
                .optional()
                .map_err(|e| SessionError::Other(format!("sqlite query: {e}")))?
                .unwrap_or(false);
            if exists {
                return Err(SessionError::Other(format!(
                    "session {session_id} already exists in registry"
                )));
            }
            conn.execute(
                "INSERT INTO sessions (
                    session_id, parent_session_id, status, agent_id, title,
                    metadata_json, created_at, updated_at, archived_at,
                    stats_json, usage_json
                ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    session_id,
                    meta.parent_session_id,
                    status_str,
                    meta.agent_id,
                    meta.title,
                    metadata_json,
                    meta.created_at,
                    meta.updated_at,
                    meta.archived_at,
                    stats_json,
                    usage_json,
                ],
            )
            .map_err(|e| SessionError::Other(format!("sqlite insert: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join: {e}")))?;
        result
    }

    async fn get(&self, session_id: &str) -> Option<Arc<dyn AgentSession>> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let exists = tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.query_row(
                "SELECT 1 FROM sessions WHERE session_id = ?1",
                params![sid],
                |_| Ok(true),
            )
            .optional()
            .ok()
            .flatten()
            .unwrap_or(false)
        })
        .await
        .ok()?;

        if !exists {
            return None;
        }

        let session = Arc::new(SqliteEvalSession::with_id(
            self.conn.clone(),
            session_id.to_string(),
        ));
        // Replay the event log to rehydrate in-memory state. Errors here
        // mean a corrupted log; treat as "not found" for callers (they
        // can `metadata()` separately to discover the row still exists).
        if let Err(e) = session.restore().await {
            tracing::warn!(session_id, error = %e, "registry get: restore failed");
            return None;
        }
        Some(session)
    }

    async fn metadata(&self, session_id: &str) -> Option<SessionMetadata> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let sql = format!("SELECT {META_COLUMNS} FROM sessions WHERE session_id = ?1");
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            conn.query_row(&sql, params![sid], decode_row)
                .optional()
                .ok()
                .flatten()
        })
        .await
        .ok()
        .flatten()
    }

    async fn update(
        &self,
        session_id: &str,
        patch: SessionMetadataPatch,
    ) -> Result<(), SessionError> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let sql = format!("SELECT {META_COLUMNS} FROM sessions WHERE session_id = ?1");

        let result = tokio::task::spawn_blocking(move || -> Result<(), SessionError> {
            let conn = conn.lock().unwrap();
            let mut current: SessionMetadata = conn
                .query_row(&sql, params![sid.clone()], decode_row)
                .optional()
                .map_err(|e| SessionError::Other(format!("sqlite query: {e}")))?
                .ok_or_else(|| SessionError::NotFound(sid.clone()))?;

            if let Some(status) = patch.status {
                current.status = status;
            }
            if let Some(agent_id) = patch.agent_id {
                current.agent_id = agent_id;
            }
            if let Some(title) = patch.title {
                current.title = title;
            }
            if let Some(metadata) = patch.metadata {
                current.metadata = metadata;
            }
            if let Some(stats) = patch.stats {
                current.stats = stats;
            }
            if let Some(usage) = patch.usage {
                current.usage = usage;
            }
            current.updated_at = chrono::Utc::now().timestamp_millis();

            conn.execute(
                "UPDATE sessions SET
                    status = ?2, agent_id = ?3, title = ?4,
                    metadata_json = ?5, stats_json = ?6, usage_json = ?7,
                    updated_at = ?8
                 WHERE session_id = ?1",
                params![
                    sid,
                    current.status.as_str(),
                    current.agent_id,
                    current.title,
                    json_or_empty(&current.metadata),
                    json_or_empty(&current.stats),
                    json_or_empty(&current.usage),
                    current.updated_at,
                ],
            )
            .map_err(|e| SessionError::Other(format!("sqlite update: {e}")))?;
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join: {e}")))?;
        result
    }

    async fn archive(&self, session_id: &str) -> Result<(), SessionError> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let now = chrono::Utc::now().timestamp_millis();
        let result = tokio::task::spawn_blocking(move || -> Result<(), SessionError> {
            let conn = conn.lock().unwrap();
            let n = conn
                .execute(
                    "UPDATE sessions SET archived_at = ?2, updated_at = ?2 WHERE session_id = ?1",
                    params![sid, now],
                )
                .map_err(|e| SessionError::Other(format!("sqlite update: {e}")))?;
            if n == 0 {
                return Err(SessionError::NotFound(sid));
            }
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join: {e}")))?;
        result
    }

    async fn delete(&self, session_id: &str) -> Result<(), SessionError> {
        let conn = self.conn.clone();
        let sid = session_id.to_string();
        let result = tokio::task::spawn_blocking(move || -> Result<(), SessionError> {
            let conn = conn.lock().unwrap();
            let n = conn
                .execute("DELETE FROM sessions WHERE session_id = ?1", params![sid])
                .map_err(|e| SessionError::Other(format!("sqlite delete: {e}")))?;
            if n == 0 {
                return Err(SessionError::NotFound(sid));
            }
            Ok(())
        })
        .await
        .map_err(|e| SessionError::Other(format!("spawn_blocking join: {e}")))?;
        result
    }

    async fn list(&self, filter: &SessionFilter) -> SessionPage {
        let conn = self.conn.clone();
        let (where_clause, params_vec) = build_where(filter);
        let order_sql = match filter.order {
            SessionOrder::Desc => "ORDER BY created_at DESC, session_id DESC",
            SessionOrder::Asc => "ORDER BY created_at ASC, session_id ASC",
        };
        let sql = format!("SELECT {META_COLUMNS} FROM sessions {where_clause} {order_sql}");
        let after = filter.after.clone();
        let limit = filter.limit;
        let order = filter.order;

        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let mut stmt = match conn.prepare(&sql) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "list: prepare failed");
                    return SessionPage {
                        items: Vec::new(),
                        next_cursor: None,
                    };
                }
            };
            // Borrow each param value as rusqlite trait object.
            let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec
                .iter()
                .map(|b| b.as_ref() as &dyn rusqlite::ToSql)
                .collect();
            let rows = match stmt.query_map(param_refs.as_slice(), decode_row) {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(error = %e, "list: query_map failed");
                    return SessionPage {
                        items: Vec::new(),
                        next_cursor: None,
                    };
                }
            };
            let mut items: Vec<SessionMetadata> = rows.filter_map(|r| r.ok()).collect();

            // Cursor: opaque session_id from previous page's last item.
            // Drop everything up to and including the cursor entry.
            if let Some(cursor) = after.as_deref() {
                if let Some(pos) = items.iter().position(|m| m.session_id == cursor) {
                    items.drain(..=pos);
                }
            }
            let _ = order; // tie-break already encoded in SQL ORDER BY

            let next_cursor = if limit > 0 && items.len() > limit {
                let last_id = items[limit - 1].session_id.clone();
                items.truncate(limit);
                Some(last_id)
            } else {
                None
            };
            SessionPage { items, next_cursor }
        })
        .await
        .unwrap_or(SessionPage {
            items: Vec::new(),
            next_cursor: None,
        })
    }

    async fn count(&self, filter: &SessionFilter) -> usize {
        let conn = self.conn.clone();
        let (where_clause, params_vec) = build_where(filter);
        let sql = format!("SELECT COUNT(*) FROM sessions {where_clause}");
        tokio::task::spawn_blocking(move || {
            let conn = conn.lock().unwrap();
            let param_refs: Vec<&dyn rusqlite::ToSql> = params_vec
                .iter()
                .map(|b| b.as_ref() as &dyn rusqlite::ToSql)
                .collect();
            conn.query_row(&sql, param_refs.as_slice(), |row| row.get::<_, i64>(0))
                .map(|n| n as usize)
                .unwrap_or(0)
        })
        .await
        .unwrap_or(0)
    }
}

// ─── Filter SQL helper ────────────────────────────────────────────────

/// Build a `WHERE ...` clause and a matching param vec from a SessionFilter.
/// Returns ("", vec![]) when the filter has no conditions (all-defaults).
/// Note: `filter.after` (cursor) and `filter.limit` are handled in
/// post-processing inside `list`, not here — they're not safe to express
/// purely in SQL while preserving the InMemory impl's tie-break semantics.
fn build_where(filter: &SessionFilter) -> (String, Vec<Box<dyn rusqlite::ToSql + Send>>) {
    let mut conds: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql + Send>> = Vec::new();

    if !filter.include_archived {
        conds.push("archived_at IS NULL".to_string());
    }

    if let Some(statuses) = &filter.statuses {
        if !statuses.is_empty() {
            let placeholders: Vec<String> = (0..statuses.len()).map(|_| "?".to_string()).collect();
            conds.push(format!("status IN ({})", placeholders.join(",")));
            for s in statuses {
                params.push(Box::new(s.as_str().to_string()));
            }
        }
    }

    if let Some(agent_id) = &filter.agent_id {
        conds.push("agent_id = ?".to_string());
        params.push(Box::new(agent_id.clone()));
    }

    if let Some(parent_filter) = &filter.parent_session_id {
        match parent_filter {
            None => {
                // Some(None) → roots only
                conds.push("parent_session_id IS NULL".to_string());
            }
            Some(id) => {
                conds.push("parent_session_id = ?".to_string());
                params.push(Box::new(id.clone()));
            }
        }
    }

    if let Some(after) = filter.created_after {
        conds.push("created_at > ?".to_string());
        params.push(Box::new(after));
    }
    if let Some(before) = filter.created_before {
        conds.push("created_at < ?".to_string());
        params.push(Box::new(before));
    }

    if conds.is_empty() {
        (String::new(), params)
    } else {
        (format!("WHERE {}", conds.join(" AND ")), params)
    }
}

// ─── Tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sqlite_session::schema::init_schema;
    use alva_kernel_core::agent_session::InMemoryAgentSession;

    fn make_registry() -> SqliteSessionRegistry {
        // In-memory DB so each test is hermetic. Schema is the same one
        // init_schema runs against on-disk DBs, so column shape matches
        // production.
        let conn = Connection::open_in_memory().expect("open in-mem DB");
        init_schema(&conn).expect("init schema");
        SqliteSessionRegistry::new(Arc::new(Mutex::new(conn)))
    }

    fn dummy_session() -> Arc<dyn AgentSession> {
        // The session arg to insert() is unused by SqliteSessionRegistry
        // (lazily re-materialised in get). An InMemoryAgentSession with a
        // fresh id is fine as a placeholder.
        Arc::new(InMemoryAgentSession::new())
    }

    fn meta(id: &str) -> SessionMetadata {
        SessionMetadata::new(id)
    }

    #[tokio::test]
    async fn insert_and_metadata_roundtrip() {
        let reg = make_registry();
        let mut m = meta("s1");
        m.title = Some("greetings".into());
        m.agent_id = Some("ag1".into());
        m.metadata.insert("workspace_id".into(), "/tmp/proj".into());
        m.metadata
            .insert("model_id".into(), "anthropic:claude-3-7".into());
        reg.insert(dummy_session(), m.clone()).await.unwrap();

        let back = reg.metadata("s1").await.expect("metadata exists");
        assert_eq!(back.session_id, "s1");
        assert_eq!(back.title.as_deref(), Some("greetings"));
        assert_eq!(back.agent_id.as_deref(), Some("ag1"));
        assert_eq!(
            back.metadata.get("workspace_id").map(String::as_str),
            Some("/tmp/proj")
        );
        assert_eq!(
            back.metadata.get("model_id").map(String::as_str),
            Some("anthropic:claude-3-7")
        );
        assert_eq!(back.status, SessionStatus::Idle);
        assert!(back.archived_at.is_none());
    }

    #[tokio::test]
    async fn insert_duplicate_fails() {
        let reg = make_registry();
        reg.insert(dummy_session(), meta("s1")).await.unwrap();
        // Second insert with same id MUST error per trait contract.
        let err = reg
            .insert(dummy_session(), meta("s1"))
            .await
            .expect_err("duplicate insert should error");
        let msg = format!("{err}");
        assert!(msg.contains("already exists"), "wrong error msg: {msg}");
    }

    #[tokio::test]
    async fn update_patches_fields_and_bumps_updated_at() {
        let reg = make_registry();
        reg.insert(dummy_session(), meta("s1")).await.unwrap();
        let before = reg.metadata("s1").await.unwrap().updated_at;

        // Make sure time advances at least 1ms so updated_at strictly grows.
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;

        reg.update(
            "s1",
            SessionMetadataPatch {
                status: Some(SessionStatus::Running),
                agent_id: Some(Some("ag-new".into())),
                title: Some(Some("renamed".into())),
                metadata: None,
                stats: None,
                usage: None,
            },
        )
        .await
        .unwrap();

        let after = reg.metadata("s1").await.unwrap();
        assert_eq!(after.status, SessionStatus::Running);
        assert_eq!(after.agent_id.as_deref(), Some("ag-new"));
        assert_eq!(after.title.as_deref(), Some("renamed"));
        assert!(after.updated_at > before, "updated_at must grow");
    }

    #[tokio::test]
    async fn update_unknown_returns_not_found() {
        let reg = make_registry();
        let err = reg
            .update("does-not-exist", SessionMetadataPatch::default())
            .await
            .expect_err("unknown id should error");
        match err {
            SessionError::NotFound(id) => assert_eq!(id, "does-not-exist"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn archive_sets_archived_at_and_excludes_from_default_list() {
        let reg = make_registry();
        reg.insert(dummy_session(), meta("s1")).await.unwrap();
        reg.insert(dummy_session(), meta("s2")).await.unwrap();

        reg.archive("s1").await.unwrap();
        let m1 = reg.metadata("s1").await.unwrap();
        assert!(m1.archived_at.is_some(), "archive must stamp archived_at");

        // Default list excludes archived
        let page = reg.list(&SessionFilter::default()).await;
        assert_eq!(page.items.len(), 1, "archived must be hidden by default");
        assert_eq!(page.items[0].session_id, "s2");

        // include_archived=true shows both
        let page_all = reg
            .list(&SessionFilter {
                include_archived: true,
                ..Default::default()
            })
            .await;
        assert_eq!(page_all.items.len(), 2);
    }

    #[tokio::test]
    async fn delete_removes_row_unknown_errors() {
        let reg = make_registry();
        reg.insert(dummy_session(), meta("s1")).await.unwrap();
        reg.delete("s1").await.unwrap();
        assert!(reg.metadata("s1").await.is_none(), "row must be gone");

        let err = reg.delete("s1").await.expect_err("second delete errors");
        match err {
            SessionError::NotFound(id) => assert_eq!(id, "s1"),
            other => panic!("expected NotFound, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn list_filters_by_status_and_agent_id_and_parent() {
        let reg = make_registry();
        // s1: idle, no parent, agent A
        let mut m1 = meta("s1");
        m1.agent_id = Some("A".into());
        reg.insert(dummy_session(), m1).await.unwrap();
        // s2: running, parent s1, agent B
        let mut m2 = meta("s2");
        m2.status = SessionStatus::Running;
        m2.parent_session_id = Some("s1".into());
        m2.agent_id = Some("B".into());
        reg.insert(dummy_session(), m2).await.unwrap();
        // s3: idle, parent s1, agent A
        let mut m3 = meta("s3");
        m3.parent_session_id = Some("s1".into());
        m3.agent_id = Some("A".into());
        reg.insert(dummy_session(), m3).await.unwrap();

        // statuses=[Running]
        let running = reg
            .list(&SessionFilter {
                statuses: Some(vec![SessionStatus::Running]),
                ..Default::default()
            })
            .await;
        assert_eq!(running.items.len(), 1);
        assert_eq!(running.items[0].session_id, "s2");

        // agent_id=A
        let agent_a = reg
            .list(&SessionFilter {
                agent_id: Some("A".into()),
                ..Default::default()
            })
            .await;
        let ids: Vec<&str> = agent_a
            .items
            .iter()
            .map(|m| m.session_id.as_str())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"s1") && ids.contains(&"s3"));

        // parent=Some(s1) → s2 + s3
        let children = reg
            .list(&SessionFilter {
                parent_session_id: Some(Some("s1".into())),
                ..Default::default()
            })
            .await;
        let ids: Vec<&str> = children
            .items
            .iter()
            .map(|m| m.session_id.as_str())
            .collect();
        assert_eq!(ids.len(), 2);
        assert!(ids.contains(&"s2") && ids.contains(&"s3"));

        // parent=Some(None) (roots only) → s1
        let roots = reg
            .list(&SessionFilter {
                parent_session_id: Some(None),
                ..Default::default()
            })
            .await;
        assert_eq!(roots.items.len(), 1);
        assert_eq!(roots.items[0].session_id, "s1");
    }

    #[tokio::test]
    async fn list_pagination_cursor_walks_all_items() {
        let reg = make_registry();
        // Insert 5 sessions with strictly increasing created_at so order
        // is deterministic and ties don't muddle the cursor test.
        for i in 0..5 {
            let mut m = meta(&format!("s{i}"));
            m.created_at = 1000 + i as i64;
            reg.insert(dummy_session(), m).await.unwrap();
        }

        // Page size 2, desc order → expect [s4,s3], [s2,s1], [s0]
        let mut collected: Vec<String> = Vec::new();
        let mut cursor: Option<String> = None;
        for _ in 0..4 {
            let page = reg
                .list(&SessionFilter {
                    limit: 2,
                    after: cursor.clone(),
                    ..Default::default()
                })
                .await;
            for m in &page.items {
                collected.push(m.session_id.clone());
            }
            cursor = page.next_cursor;
            if cursor.is_none() {
                break;
            }
        }
        assert_eq!(collected, vec!["s4", "s3", "s2", "s1", "s0"]);
    }

    #[tokio::test]
    async fn count_matches_filter_size() {
        let reg = make_registry();
        for i in 0..3 {
            reg.insert(dummy_session(), meta(&format!("s{i}")))
                .await
                .unwrap();
        }
        // One running
        reg.update(
            "s1",
            SessionMetadataPatch {
                status: Some(SessionStatus::Running),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        assert_eq!(reg.count(&SessionFilter::default()).await, 3);
        assert_eq!(
            reg.count(&SessionFilter {
                statuses: Some(vec![SessionStatus::Running]),
                ..Default::default()
            })
            .await,
            1
        );
    }

    #[tokio::test]
    async fn get_returns_handle_for_existing_session_none_for_missing() {
        let reg = make_registry();
        reg.insert(dummy_session(), meta("s1")).await.unwrap();
        assert!(reg.get("s1").await.is_some());
        assert!(reg.get("does-not-exist").await.is_none());
    }

    /// Coexistence guard: SqliteEvalSessionManager (legacy) and
    /// SqliteSessionRegistry (trait) share the same SQLite connection +
    /// `sessions` table in production (see AppState::new). A row written
    /// by the legacy `create_session` path must be readable by
    /// `registry.metadata`, otherwise an in-flight Tauri command writing
    /// via the manager would create a row that's invisible to any future
    /// registry consumer. The new columns get their CREATE-TABLE defaults
    /// (status='idle', metadata_json='{}', etc.) so the registry decode
    /// is non-lossy on legacy rows.
    #[tokio::test]
    async fn registry_sees_sessions_created_via_legacy_manager() {
        use crate::sqlite_session::SqliteEvalSessionManager;
        use std::fs;
        // Need an on-disk DB because SqliteEvalSessionManager::open takes
        // a path. Build a unique path under std::env::temp_dir() so the
        // test stays hermetic without pulling in tempfile as a dep.
        let unique = format!(
            "alva-registry-coexist-{}-{}.db",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0),
        );
        let db_path = std::env::temp_dir().join(unique);
        let _ = fs::remove_file(&db_path); // safety: ignore if absent

        let manager = SqliteEvalSessionManager::open(db_path.clone()).expect("manager open");
        let registry = SqliteSessionRegistry::new(manager.conn().clone());

        // Legacy create_session writes 3 columns (session_id / created_at /
        // preview); the rest default per schema.
        let session = manager.create_session("hello legacy").await;
        let sid = session.session_id().to_string();

        // Registry must see the row. Defaults apply for the trait fields.
        let meta = registry
            .metadata(&sid)
            .await
            .expect("registry must see legacy-created session");
        assert_eq!(meta.session_id, sid);
        assert_eq!(meta.status, SessionStatus::Idle);
        assert!(meta.title.is_none());
        assert!(meta.metadata.is_empty());
        assert!(meta.archived_at.is_none());

        // And vice versa: archiving via registry, then the row should
        // still be deletable via legacy delete_session — proving the
        // two views agree on row identity.
        registry.archive(&sid).await.expect("archive ok");
        let after = registry.metadata(&sid).await.unwrap();
        assert!(after.archived_at.is_some());

        // tempdir auto-cleanup
        drop(manager);
        drop(registry);
        let _ = fs::remove_file(&db_path);
    }
}
