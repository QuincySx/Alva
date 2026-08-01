// INPUT:  rusqlite, serde, filesystem paths, UUID/time primitives
// OUTPUT: HubStore plus durable Session/Branch/Turn/Attempt DTOs and crash repair
// POS:    Hub Desktop-private SQLite state machine; independent of native Alva Desktop storage.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

const ENGINE_ID: &str = "alva";
const SCHEMA_VERSION: i64 = 2;
const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_INTERRUPTED: &str = "interrupted";
const RESTART_REASON: &str =
    "ALVA Hub Desktop restarted before the engine attempt reached a terminal state.";

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HubSessionSummary {
    pub product_session_id: String,
    pub workspace: String,
    pub active_branch_id: String,
    pub engine_id: String,
    pub engine_session_id: Option<String>,
    pub status: String,
    pub updated_at: i64,
    pub last_prompt: Option<String>,
    pub last_result: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoredTurn {
    pub turn_id: String,
    pub prompt: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    pub attempts: Vec<StoredAttempt>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StoredUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_cost_usd: Option<f64>,
    pub duration_ms: u64,
    pub num_turns: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SideEffectEvidence {
    pub tool_call_id: String,
    pub tool_name: String,
    pub succeeded: Option<bool>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct StoredAttempt {
    pub attempt_id: String,
    pub turn_id: String,
    pub branch_id: String,
    pub engine_id: String,
    pub status: String,
    pub failure_reason: Option<String>,
    pub usage: Option<StoredUsage>,
    pub side_effects: Vec<SideEffectEvidence>,
    pub started_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HubSessionDetail {
    pub session: HubSessionSummary,
    pub turns: Vec<StoredTurn>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedTurn {
    pub product_session_id: String,
    pub branch_id: String,
    pub turn_id: String,
    pub attempt_id: String,
    pub workspace: PathBuf,
    pub engine_session_id: Option<String>,
}

#[derive(Clone)]
pub struct HubStore {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
    _instance_lock: Arc<File>,
}

impl HubStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        ensure_private_parent(&path)?;
        validate_database_file(&path)?;
        ensure_private_database_file(&path)?;
        let instance_lock = acquire_instance_lock(&path)?;

        let mut connection = Connection::open(&path)
            .map_err(|error| format!("open Hub database {}: {error}", path.display()))?;
        let schema_version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .map_err(|error| format!("read Hub database schema version: {error}"))?;
        if schema_version > SCHEMA_VERSION {
            return Err(format!(
                "Hub database schema version {schema_version} is newer than supported version {SCHEMA_VERSION}"
            ));
        }
        connection
            .busy_timeout(Duration::from_secs(5))
            .map_err(|error| format!("configure Hub database timeout: {error}"))?;
        initialize_schema_and_recover(&mut connection, schema_version, &path)?;

        Ok(Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
            _instance_lock: Arc::new(instance_lock),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list_sessions(&self) -> Result<Vec<HubSessionSummary>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT hs.product_session_id, hs.workspace, hs.active_branch_id,
                        eb.engine_id, eb.engine_session_id, hs.status, hs.updated_at,
                        (SELECT prompt FROM turns t
                           WHERE t.product_session_id = hs.product_session_id
                           ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1),
                        (SELECT final_result FROM turns t
                           WHERE t.product_session_id = hs.product_session_id
                           ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1)
                   FROM hub_sessions hs
                   JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
                  ORDER BY hs.updated_at DESC, hs.product_session_id ASC",
            )
            .map_err(|error| format!("prepare Hub session list: {error}"))?;
        let rows = statement
            .query_map([], decode_session)
            .map_err(|error| format!("query Hub sessions: {error}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub sessions: {error}"))
    }

    pub fn get_session(
        &self,
        product_session_id: &str,
    ) -> Result<Option<HubSessionDetail>, String> {
        let connection = self.lock()?;
        let session = query_session(&connection, product_session_id)?;
        let Some(session) = session else {
            return Ok(None);
        };
        let mut statement = connection
            .prepare(
                "SELECT turn_id, prompt, final_result, error_message, status,
                        created_at, completed_at
                   FROM turns
                  WHERE product_session_id = ?1
                  ORDER BY created_at ASC, rowid ASC",
            )
            .map_err(|error| format!("prepare Hub turn history: {error}"))?;
        let rows = statement
            .query_map([product_session_id], |row| {
                Ok(StoredTurn {
                    turn_id: row.get(0)?,
                    prompt: row.get(1)?,
                    result: row.get(2)?,
                    error: row.get(3)?,
                    status: row.get(4)?,
                    created_at: row.get(5)?,
                    completed_at: row.get(6)?,
                    attempts: Vec::new(),
                })
            })
            .map_err(|error| format!("query Hub turn history: {error}"))?;
        let mut turns = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub turn history: {error}"))?;
        drop(statement);
        for turn in &mut turns {
            turn.attempts = query_attempts(&connection, &turn.turn_id)?;
        }
        Ok(Some(HubSessionDetail { session, turns }))
    }

    pub(crate) fn prepare_new_turn(
        &self,
        product_session_id: &str,
        workspace: &Path,
        prompt: &str,
    ) -> Result<PreparedTurn, String> {
        let branch_id = uuid::Uuid::new_v4().to_string();
        let turn_id = uuid::Uuid::new_v4().to_string();
        let attempt_id = uuid::Uuid::new_v4().to_string();
        let now = now_unix_ms();
        let workspace_text = workspace.to_string_lossy().into_owned();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub session transaction: {error}"))?;
        transaction
            .execute(
                "INSERT INTO hub_sessions
                   (product_session_id, workspace, active_branch_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
                params![
                    product_session_id,
                    workspace_text,
                    branch_id,
                    STATUS_RUNNING,
                    now
                ],
            )
            .map_err(|error| format!("insert Hub session: {error}"))?;
        transaction
            .execute(
                "INSERT INTO engine_branches
                   (branch_id, product_session_id, engine_id, engine_session_id,
                    status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, NULL, 'active', ?4, ?4)",
                params![branch_id, product_session_id, ENGINE_ID, now],
            )
            .map_err(|error| format!("insert Hub engine branch: {error}"))?;
        insert_turn_and_attempt(
            &transaction,
            &turn_id,
            &attempt_id,
            product_session_id,
            &branch_id,
            prompt,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub session transaction: {error}"))?;

        Ok(PreparedTurn {
            product_session_id: product_session_id.to_string(),
            branch_id,
            turn_id,
            attempt_id,
            workspace: workspace.to_path_buf(),
            engine_session_id: None,
        })
    }

    pub(crate) fn prepare_resumed_turn(
        &self,
        product_session_id: &str,
        prompt: &str,
    ) -> Result<PreparedTurn, String> {
        let turn_id = uuid::Uuid::new_v4().to_string();
        let attempt_id = uuid::Uuid::new_v4().to_string();
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub resume transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        if session.status == STATUS_RUNNING {
            return Err(format!(
                "Hub session '{product_session_id}' is already running"
            ));
        }
        let engine_session_id = session
            .engine_session_id
            .clone()
            .filter(|id| !id.is_empty());
        transaction
            .execute(
                "UPDATE hub_sessions SET status = ?2, updated_at = ?3
                  WHERE product_session_id = ?1",
                params![product_session_id, STATUS_RUNNING, now],
            )
            .map_err(|error| format!("mark Hub session running: {error}"))?;
        insert_turn_and_attempt(
            &transaction,
            &turn_id,
            &attempt_id,
            product_session_id,
            &session.active_branch_id,
            prompt,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub resume transaction: {error}"))?;

        Ok(PreparedTurn {
            product_session_id: product_session_id.to_string(),
            branch_id: session.active_branch_id,
            turn_id,
            attempt_id,
            workspace: PathBuf::from(session.workspace),
            engine_session_id,
        })
    }

    pub(crate) fn finish_turn(
        &self,
        prepared: &PreparedTurn,
        engine_session_id: Option<&str>,
        result: Option<&str>,
        error: Option<&str>,
        usage: Option<&StoredUsage>,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let status = if error.is_some() {
            STATUS_FAILED
        } else {
            STATUS_COMPLETED
        };
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub completion transaction: {error}"))?;
        if let Some(engine_session_id) = engine_session_id.filter(|id| !id.is_empty()) {
            transaction
                .execute(
                    "UPDATE engine_branches
                        SET engine_session_id = ?2, updated_at = ?3
                      WHERE branch_id = ?1",
                    params![prepared.branch_id, engine_session_id, now],
                )
                .map_err(|error| format!("persist engine session id: {error}"))?;
        }
        let usage_json = usage
            .map(serde_json::to_string)
            .transpose()
            .map_err(|error| format!("serialize engine attempt usage: {error}"))?;
        let attempt_changed = transaction
            .execute(
                "UPDATE engine_attempts
                    SET status = ?2, failure_reason = ?3, usage_json = ?4, completed_at = ?5
                  WHERE attempt_id = ?1 AND status = ?6",
                params![
                    prepared.attempt_id,
                    status,
                    error,
                    usage_json,
                    now,
                    STATUS_RUNNING
                ],
            )
            .map_err(|error| format!("complete Hub engine attempt: {error}"))?;
        if attempt_changed != 1 {
            return Err(format!(
                "Hub engine attempt '{}' was not in running state",
                prepared.attempt_id
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE turns
                    SET final_result = ?2, error_message = ?3, status = ?4, completed_at = ?5
                  WHERE turn_id = ?1 AND status = ?6",
                params![prepared.turn_id, result, error, status, now, STATUS_RUNNING],
            )
            .map_err(|error| format!("complete Hub turn: {error}"))?;
        if changed != 1 {
            return Err(format!(
                "Hub turn '{}' was not in running state",
                prepared.turn_id
            ));
        }
        transaction
            .execute(
                "UPDATE hub_sessions SET status = ?2, updated_at = ?3
                  WHERE product_session_id = ?1",
                params![prepared.product_session_id, status, now],
            )
            .map_err(|error| format!("complete Hub session: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub completion transaction: {error}"))
    }

    pub(crate) fn record_engine_session(
        &self,
        prepared: &PreparedTurn,
        engine_session_id: &str,
    ) -> Result<(), String> {
        if engine_session_id.trim().is_empty() {
            return Ok(());
        }
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE engine_branches
                    SET engine_session_id = ?2, updated_at = ?3
                  WHERE branch_id = ?1",
                params![prepared.branch_id, engine_session_id, now_unix_ms()],
            )
            .map_err(|error| format!("persist running engine session id: {error}"))?;
        if changed != 1 {
            return Err(format!(
                "Hub engine branch '{}' was not found",
                prepared.branch_id
            ));
        }
        Ok(())
    }

    pub(crate) fn record_side_effect(
        &self,
        prepared: &PreparedTurn,
        evidence: SideEffectEvidence,
    ) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin side-effect evidence transaction: {error}"))?;
        let encoded = transaction
            .query_row(
                "SELECT side_effects_json FROM engine_attempts
                  WHERE attempt_id = ?1 AND status = ?2",
                params![prepared.attempt_id, STATUS_RUNNING],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("read side-effect evidence: {error}"))?
            .ok_or_else(|| {
                format!(
                    "Hub engine attempt '{}' was not in running state",
                    prepared.attempt_id
                )
            })?;
        let mut evidence_list: Vec<SideEffectEvidence> = serde_json::from_str(&encoded)
            .map_err(|error| format!("decode side-effect evidence: {error}"))?;
        if let Some(existing) = evidence_list
            .iter_mut()
            .find(|item| item.tool_call_id == evidence.tool_call_id)
        {
            *existing = evidence;
        } else {
            evidence_list.push(evidence);
        }
        let encoded = serde_json::to_string(&evidence_list)
            .map_err(|error| format!("serialize side-effect evidence: {error}"))?;
        transaction
            .execute(
                "UPDATE engine_attempts SET side_effects_json = ?2
                  WHERE attempt_id = ?1 AND status = ?3",
                params![prepared.attempt_id, encoded, STATUS_RUNNING],
            )
            .map_err(|error| format!("persist side-effect evidence: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit side-effect evidence: {error}"))
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "Hub database lock poisoned".to_string())
    }
}

fn insert_turn_and_attempt(
    transaction: &Transaction<'_>,
    turn_id: &str,
    attempt_id: &str,
    product_session_id: &str,
    branch_id: &str,
    prompt: &str,
    now: i64,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO turns
               (turn_id, product_session_id, branch_id, prompt, final_result,
                error_message, status, created_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, NULL, NULL, ?5, ?6, NULL)",
            params![
                turn_id,
                product_session_id,
                branch_id,
                prompt,
                STATUS_RUNNING,
                now
            ],
        )
        .map_err(|error| format!("insert Hub turn: {error}"))?;
    transaction
        .execute(
            "INSERT INTO engine_attempts
               (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                status, failure_reason, usage_json, side_effects_json,
                started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, '[]', ?7, NULL)",
            params![
                attempt_id,
                turn_id,
                product_session_id,
                branch_id,
                ENGINE_ID,
                STATUS_RUNNING,
                now
            ],
        )
        .map_err(|error| format!("insert Hub engine attempt: {error}"))?;
    Ok(())
}

fn query_attempts(connection: &Connection, turn_id: &str) -> Result<Vec<StoredAttempt>, String> {
    let mut statement = connection
        .prepare(
            "SELECT attempt_id, turn_id, branch_id, engine_id, status,
                    failure_reason, usage_json, side_effects_json,
                    started_at, completed_at
               FROM engine_attempts
              WHERE turn_id = ?1
              ORDER BY started_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub engine attempts: {error}"))?;
    let rows = statement
        .query_map([turn_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, String>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, Option<String>>(5)?,
                row.get::<_, Option<String>>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, i64>(8)?,
                row.get::<_, Option<i64>>(9)?,
            ))
        })
        .map_err(|error| format!("query Hub engine attempts: {error}"))?;
    rows.map(|row| {
        let (
            attempt_id,
            turn_id,
            branch_id,
            engine_id,
            status,
            failure_reason,
            usage_json,
            side_effects_json,
            started_at,
            completed_at,
        ) = row.map_err(|error| format!("decode Hub engine attempt: {error}"))?;
        let usage = usage_json
            .map(|json| serde_json::from_str(&json))
            .transpose()
            .map_err(|error| format!("decode Hub engine attempt usage: {error}"))?;
        let side_effects = serde_json::from_str(&side_effects_json)
            .map_err(|error| format!("decode Hub side-effect evidence: {error}"))?;
        Ok(StoredAttempt {
            attempt_id,
            turn_id,
            branch_id,
            engine_id,
            status,
            failure_reason,
            usage,
            side_effects,
            started_at,
            completed_at,
        })
    })
    .collect()
}

fn query_session(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Option<HubSessionSummary>, String> {
    connection
        .query_row(
            "SELECT hs.product_session_id, hs.workspace, hs.active_branch_id,
                    eb.engine_id, eb.engine_session_id, hs.status, hs.updated_at,
                    (SELECT prompt FROM turns t
                       WHERE t.product_session_id = hs.product_session_id
                       ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1),
                    (SELECT final_result FROM turns t
                       WHERE t.product_session_id = hs.product_session_id
                       ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1)
               FROM hub_sessions hs
               JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
              WHERE hs.product_session_id = ?1",
            [product_session_id],
            decode_session,
        )
        .optional()
        .map_err(|error| format!("query Hub session '{product_session_id}': {error}"))
}

fn decode_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<HubSessionSummary> {
    Ok(HubSessionSummary {
        product_session_id: row.get(0)?,
        workspace: row.get(1)?,
        active_branch_id: row.get(2)?,
        engine_id: row.get(3)?,
        engine_session_id: row.get(4)?,
        status: row.get(5)?,
        updated_at: row.get(6)?,
        last_prompt: row.get(7)?,
        last_result: row.get(8)?,
    })
}

fn initialize_schema_and_recover(
    connection: &mut Connection,
    schema_version: i64,
    path: &Path,
) -> Result<(), String> {
    connection
        .execute_batch("PRAGMA foreign_keys = ON; PRAGMA synchronous = FULL;")
        .map_err(|error| format!("configure Hub database {}: {error}", path.display()))?;
    let now = now_unix_ms();
    let transaction = connection
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(|error| format!("begin Hub database migration: {error}"))?;
    transaction
        .execute_batch(
            "CREATE TABLE IF NOT EXISTS hub_sessions (
               product_session_id TEXT PRIMARY KEY,
               workspace TEXT NOT NULL,
               active_branch_id TEXT NOT NULL UNIQUE,
               status TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL
             );
             CREATE TABLE IF NOT EXISTS engine_branches (
               branch_id TEXT PRIMARY KEY,
               product_session_id TEXT NOT NULL,
               engine_id TEXT NOT NULL,
               engine_session_id TEXT,
               status TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               updated_at INTEGER NOT NULL,
               FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id)
             );
             CREATE TABLE IF NOT EXISTS turns (
               turn_id TEXT PRIMARY KEY,
               product_session_id TEXT NOT NULL,
               branch_id TEXT NOT NULL,
               prompt TEXT NOT NULL,
               final_result TEXT,
               error_message TEXT,
               status TEXT NOT NULL,
               created_at INTEGER NOT NULL,
               completed_at INTEGER,
               FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id),
               FOREIGN KEY(branch_id) REFERENCES engine_branches(branch_id)
             );
             CREATE INDEX IF NOT EXISTS idx_turns_product_created
               ON turns(product_session_id, created_at);",
        )
        .map_err(|error| format!("initialize Hub database {}: {error}", path.display()))?;

    if schema_version < 2 {
        transaction
            .execute_batch(
                "CREATE TABLE engine_attempts (
                   attempt_id TEXT PRIMARY KEY,
                   turn_id TEXT NOT NULL,
                   product_session_id TEXT NOT NULL,
                   branch_id TEXT NOT NULL,
                   engine_id TEXT NOT NULL,
                   status TEXT NOT NULL,
                   failure_reason TEXT,
                   usage_json TEXT,
                   side_effects_json TEXT NOT NULL DEFAULT '[]',
                   started_at INTEGER NOT NULL,
                   completed_at INTEGER,
                   FOREIGN KEY(turn_id) REFERENCES turns(turn_id),
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id),
                   FOREIGN KEY(branch_id) REFERENCES engine_branches(branch_id)
                 );
                 INSERT INTO engine_attempts
                   (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                    status, failure_reason, usage_json, side_effects_json,
                    started_at, completed_at)
                 SELECT 'migrated-' || t.turn_id, t.turn_id, t.product_session_id,
                        t.branch_id, eb.engine_id, t.status, t.error_message,
                        NULL, '[]', t.created_at, t.completed_at
                   FROM turns t
                   JOIN engine_branches eb ON eb.branch_id = t.branch_id;
                 CREATE INDEX idx_engine_attempts_turn_started
                   ON engine_attempts(turn_id, started_at);
                 CREATE UNIQUE INDEX idx_engine_attempts_one_running_per_session
                   ON engine_attempts(product_session_id)
                   WHERE status = 'running';",
            )
            .map_err(|error| format!("migrate Hub database to schema 2: {error}"))?;
    }

    repair_interrupted_attempts(&transaction, now)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| format!("record Hub database schema version: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("commit Hub database migration: {error}"))
}

fn repair_interrupted_attempts(transaction: &Transaction<'_>, now: i64) -> Result<(), String> {
    transaction
        .execute(
            "UPDATE engine_attempts
                SET status = ?1, failure_reason = COALESCE(failure_reason, ?2),
                    completed_at = COALESCE(completed_at, ?3)
              WHERE status = ?4",
            params![STATUS_INTERRUPTED, RESTART_REASON, now, STATUS_RUNNING],
        )
        .map_err(|error| format!("repair interrupted engine attempts: {error}"))?;
    transaction
        .execute(
            "UPDATE turns
                SET status = ?1, error_message = COALESCE(error_message, ?2),
                    completed_at = COALESCE(completed_at, ?3)
              WHERE status = ?4
                AND EXISTS (
                  SELECT 1 FROM engine_attempts ea
                   WHERE ea.turn_id = turns.turn_id AND ea.status = ?1
                )",
            params![STATUS_INTERRUPTED, RESTART_REASON, now, STATUS_RUNNING],
        )
        .map_err(|error| format!("repair interrupted turns: {error}"))?;
    transaction
        .execute(
            "UPDATE hub_sessions
                SET status = ?1, updated_at = ?2
              WHERE status = ?3
                AND EXISTS (
                  SELECT 1 FROM turns t
                   WHERE t.product_session_id = hub_sessions.product_session_id
                     AND t.status = ?1
                )",
            params![STATUS_INTERRUPTED, now, STATUS_RUNNING],
        )
        .map_err(|error| format!("repair interrupted Hub sessions: {error}"))?;
    Ok(())
}

fn acquire_instance_lock(database_path: &Path) -> Result<File, String> {
    let file_name = database_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("alva-hub.db");
    let lock_path = database_path.with_file_name(format!(".{file_name}.instance.lock"));
    match fs::symlink_metadata(&lock_path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse symlinked Hub instance lock {}",
                lock_path.display()
            ));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "Hub instance lock is not a regular file: {}",
                lock_path.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!(
                "inspect Hub instance lock {}: {error}",
                lock_path.display()
            ));
        }
    }
    let mut options = OpenOptions::new();
    options.read(true).write(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let file = options
        .open(&lock_path)
        .map_err(|error| format!("open Hub instance lock {}: {error}", lock_path.display()))?;
    set_private_file_permissions(&lock_path)?;
    file.try_lock().map_err(|error| {
        format!(
            "ALVA Hub Desktop database {} is already open: {error}",
            database_path.display()
        )
    })?;
    Ok(file)
}

fn ensure_private_parent(path: &Path) -> Result<(), String> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| {
            format!(
                "Hub database requires an explicit parent: {}",
                path.display()
            )
        })?;
    match fs::symlink_metadata(parent) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!(
                "refuse symlinked Hub data directory {}",
                parent.display()
            ));
        }
        Ok(metadata) if !metadata.is_dir() => {
            return Err(format!(
                "Hub data path is not a directory: {}",
                parent.display()
            ));
        }
        Ok(_) => {}
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            fs::create_dir_all(parent).map_err(|error| {
                format!("create Hub data directory {}: {error}", parent.display())
            })?;
        }
        Err(error) => {
            return Err(format!(
                "inspect Hub data directory {}: {error}",
                parent.display()
            ));
        }
    }
    set_private_dir_permissions(parent)
}

fn validate_database_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            Err(format!("refuse symlinked Hub database {}", path.display()))
        }
        Ok(metadata) if !metadata.is_file() => Err(format!(
            "Hub database is not a regular file: {}",
            path.display()
        )),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(format!("inspect Hub database {}: {error}", path.display())),
    }
}

fn ensure_private_database_file(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => {
            return Err(format!("refuse symlinked Hub database {}", path.display()));
        }
        Ok(metadata) if !metadata.is_file() => {
            return Err(format!(
                "Hub database is not a regular file: {}",
                path.display()
            ));
        }
        Ok(_) => return set_private_file_permissions(path),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => {
            return Err(format!("inspect Hub database {}: {error}", path.display()));
        }
    }

    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    match options.open(path) {
        Ok(file) => {
            drop(file);
            set_private_file_permissions(path)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            validate_database_file(path)?;
            set_private_file_permissions(path)
        }
        Err(error) => Err(format!(
            "create private Hub database {}: {error}",
            path.display()
        )),
    }
}

#[cfg(unix)]
fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("chmod 0700 {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("chmod 0600 {}: {error}", path.display()))
}

#[cfg(not(unix))]
fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn database_and_parent_are_private() {
        use std::os::unix::fs::PermissionsExt;

        let root = tempfile::tempdir().unwrap();
        let data = root.path().join("hub-data");
        let database = data.join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();

        assert_eq!(
            fs::metadata(&data).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            fs::metadata(store.path()).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }

    #[cfg(unix)]
    #[test]
    fn refuses_symlinked_database_and_parent() {
        use std::os::unix::fs::symlink;

        let root = tempfile::tempdir().unwrap();
        let outside = tempfile::tempdir().unwrap();
        let linked_parent = root.path().join("linked");
        symlink(outside.path(), &linked_parent).unwrap();
        let parent_error = HubStore::open(linked_parent.join("alva-hub.db"))
            .err()
            .expect("symlinked parent must be rejected");
        assert!(parent_error.contains("symlinked Hub data directory"));

        let data = root.path().join("data");
        fs::create_dir(&data).unwrap();
        let outside_file = outside.path().join("outside.db");
        fs::write(&outside_file, b"keep").unwrap();
        let database = data.join("alva-hub.db");
        symlink(&outside_file, &database).unwrap();
        let file_error = HubStore::open(&database)
            .err()
            .expect("symlinked database must be rejected");
        assert!(file_error.contains("symlinked Hub database"));
        assert_eq!(fs::read(outside_file).unwrap(), b"keep");
    }

    #[test]
    fn independent_connection_observes_atomic_turn_and_attempt_transaction() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let first = HubStore::open(&database).unwrap();
        let second = Connection::open(&database).unwrap();

        let prepared = first
            .prepare_new_turn("product-1", workspace.path(), "hello")
            .unwrap();
        let visible: (i64, i64) = second
            .query_row(
                "SELECT (SELECT COUNT(*) FROM turns WHERE product_session_id = ?1),
                        (SELECT COUNT(*) FROM engine_attempts WHERE product_session_id = ?1)",
                ["product-1"],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .unwrap();
        assert_eq!(visible, (1, 1));
        let duplicate = second.execute(
            "INSERT INTO engine_attempts
               (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                status, side_effects_json, started_at)
             VALUES ('overlap', ?1, 'product-1', ?2, 'alva', 'running', '[]', 1)",
            params![prepared.turn_id, prepared.branch_id],
        );
        assert!(
            duplicate.is_err(),
            "the database must reject two active attempts"
        );

        first
            .finish_turn(&prepared, Some("engine-1"), Some("done"), None, None)
            .unwrap();
        let completed = first.get_session("product-1").unwrap().unwrap();
        assert_eq!(completed.session.status, STATUS_COMPLETED);
        assert_eq!(
            completed.session.engine_session_id.as_deref(),
            Some("engine-1")
        );
        assert_eq!(completed.turns[0].result.as_deref(), Some("done"));

        let running = first.prepare_resumed_turn("product-1", "next").unwrap();
        let busy_error = first
            .prepare_resumed_turn("product-1", "overlap")
            .expect_err("the session must not start a second active attempt");
        assert!(busy_error.contains("already running"));
        first
            .finish_turn(&running, Some("engine-1"), Some("next done"), None, None)
            .unwrap();
    }

    #[test]
    fn reopening_repairs_an_abandoned_attempt_and_preserves_evidence() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn("product-crash", workspace.path(), "change the project")
            .unwrap();
        store
            .record_engine_session(&prepared, "native-crash")
            .unwrap();
        store
            .record_side_effect(
                &prepared,
                SideEffectEvidence {
                    tool_call_id: "tool-1".into(),
                    tool_name: "write_file".into(),
                    succeeded: None,
                    duration_ms: Some(12),
                },
            )
            .unwrap();

        let running = store.get_session("product-crash").unwrap().unwrap();
        assert_eq!(running.turns[0].status, STATUS_RUNNING);
        assert_eq!(running.turns[0].attempts.len(), 1);
        assert_eq!(running.turns[0].attempts[0].status, STATUS_RUNNING);
        drop(store);

        let reopened = HubStore::open(&database).unwrap();
        let recovered = reopened.get_session("product-crash").unwrap().unwrap();
        assert_eq!(recovered.session.status, STATUS_INTERRUPTED);
        assert_eq!(recovered.turns[0].status, STATUS_INTERRUPTED);
        assert_eq!(recovered.turns[0].attempts[0].status, STATUS_INTERRUPTED);
        assert!(recovered.turns[0].attempts[0]
            .failure_reason
            .as_deref()
            .unwrap()
            .contains("restarted"));
        assert_eq!(recovered.turns[0].attempts[0].side_effects.len(), 1);
        assert_eq!(
            recovered.turns[0].attempts[0].side_effects[0].succeeded,
            None
        );

        let continued = reopened
            .prepare_resumed_turn("product-crash", "continue safely")
            .unwrap();
        assert_eq!(continued.engine_session_id.as_deref(), Some("native-crash"));
    }

    #[test]
    fn live_store_owns_the_recovery_lock() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let _store = HubStore::open(&database).unwrap();

        let error = HubStore::open(&database)
            .err()
            .expect("a second live Desktop store must not repair active attempts");
        assert!(error.contains("already open"));
    }

    #[test]
    fn migrates_schema_one_turns_into_attempts_without_losing_history() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection
            .execute_batch(
                "CREATE TABLE hub_sessions (
                   product_session_id TEXT PRIMARY KEY,
                   workspace TEXT NOT NULL,
                   active_branch_id TEXT NOT NULL UNIQUE,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE engine_branches (
                   branch_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   engine_id TEXT NOT NULL,
                   engine_session_id TEXT,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 CREATE TABLE turns (
                   turn_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   branch_id TEXT NOT NULL,
                   prompt TEXT NOT NULL,
                   final_result TEXT,
                   error_message TEXT,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   completed_at INTEGER
                 );
                 INSERT INTO hub_sessions VALUES
                   ('product-v1', '/tmp/workspace', 'branch-v1', 'completed', 1, 2);
                 INSERT INTO engine_branches VALUES
                   ('branch-v1', 'product-v1', 'alva', 'native-v1', 'active', 1, 2);
                 INSERT INTO turns VALUES
                   ('turn-v1', 'product-v1', 'branch-v1', 'old prompt',
                    'old result', NULL, 'completed', 1, 2);
                 PRAGMA user_version = 1;",
            )
            .unwrap();
        drop(connection);

        let store = HubStore::open(&database).unwrap();
        let detail = store.get_session("product-v1").unwrap().unwrap();
        assert_eq!(detail.turns[0].result.as_deref(), Some("old result"));
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.turns[0].attempts[0].status, STATUS_COMPLETED);
        let version = store
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn rejects_a_future_schema_without_downgrading_it() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 3).unwrap();
        drop(connection);

        let error = HubStore::open(&database)
            .err()
            .expect("future schema must be rejected");
        assert!(error.contains("newer than supported"));

        let connection = Connection::open(&database).unwrap();
        let version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, 3);
    }
}
