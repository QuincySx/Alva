// INPUT:  rusqlite, serde, filesystem paths, UUID/time primitives
// OUTPUT: HubStore plus durable HubSession/EngineBranch/Turn product DTOs
// POS:    Hub Desktop-private SQLite boundary; independent of native Alva Desktop storage.

use std::fs::{self, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::Serialize;

const ENGINE_ID: &str = "alva";
const SCHEMA_VERSION: i64 = 1;
const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";

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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoredTurn {
    pub turn_id: String,
    pub prompt: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub status: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HubSessionDetail {
    pub session: HubSessionSummary,
    pub turns: Vec<StoredTurn>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedTurn {
    pub product_session_id: String,
    pub branch_id: String,
    pub turn_id: String,
    pub workspace: PathBuf,
    pub engine_session_id: Option<String>,
}

#[derive(Clone)]
pub struct HubStore {
    path: PathBuf,
    connection: Arc<Mutex<Connection>>,
}

impl HubStore {
    pub fn open(path: impl Into<PathBuf>) -> Result<Self, String> {
        let path = path.into();
        ensure_private_parent(&path)?;
        validate_database_file(&path)?;
        ensure_private_database_file(&path)?;

        let connection = Connection::open(&path)
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
        connection
            .execute_batch(
                "PRAGMA foreign_keys = ON;
                 PRAGMA synchronous = FULL;
                 BEGIN IMMEDIATE;
                 CREATE TABLE IF NOT EXISTS hub_sessions (
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
                   ON turns(product_session_id, created_at);
                 PRAGMA user_version = 1;
                 COMMIT;",
            )
            .map_err(|error| format!("initialize Hub database {}: {error}", path.display()))?;

        Ok(Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
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
                })
            })
            .map_err(|error| format!("query Hub turn history: {error}"))?;
        let turns = rows
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub turn history: {error}"))?;
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
        let now = now_unix_ms();
        let workspace_text = workspace.to_string_lossy().into_owned();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
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
        insert_turn(
            &transaction,
            &turn_id,
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
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
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
            .filter(|id| !id.is_empty())
            .ok_or_else(|| {
                format!("Hub session '{product_session_id}' has no resumable engine session")
            })?;
        transaction
            .execute(
                "UPDATE hub_sessions SET status = ?2, updated_at = ?3
                  WHERE product_session_id = ?1",
                params![product_session_id, STATUS_RUNNING, now],
            )
            .map_err(|error| format!("mark Hub session running: {error}"))?;
        insert_turn(
            &transaction,
            &turn_id,
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
            workspace: PathBuf::from(session.workspace),
            engine_session_id: Some(engine_session_id),
        })
    }

    pub(crate) fn finish_turn(
        &self,
        prepared: &PreparedTurn,
        engine_session_id: Option<&str>,
        result: Option<&str>,
        error: Option<&str>,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let status = if error.is_some() {
            STATUS_FAILED
        } else {
            STATUS_COMPLETED
        };
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
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

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "Hub database lock poisoned".to_string())
    }
}

fn insert_turn(
    transaction: &Transaction<'_>,
    turn_id: &str,
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
    Ok(())
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
    fn two_connections_observe_one_atomic_session_transaction() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let first = HubStore::open(&database).unwrap();
        let second = HubStore::open(&database).unwrap();

        let prepared = first
            .prepare_new_turn("product-1", workspace.path(), "hello")
            .unwrap();
        let visible = second.get_session("product-1").unwrap().unwrap();
        assert_eq!(visible.turns.len(), 1);
        assert_eq!(visible.session.status, STATUS_RUNNING);

        second
            .finish_turn(&prepared, Some("engine-1"), Some("done"), None)
            .unwrap();
        let completed = first.get_session("product-1").unwrap().unwrap();
        assert_eq!(completed.session.status, STATUS_COMPLETED);
        assert_eq!(
            completed.session.engine_session_id.as_deref(),
            Some("engine-1")
        );
        assert_eq!(completed.turns[0].result.as_deref(), Some("done"));

        let running = first.prepare_resumed_turn("product-1", "next").unwrap();
        let busy_error = second
            .prepare_resumed_turn("product-1", "overlap")
            .expect_err("a second connection must not resume a running session");
        assert!(busy_error.contains("already running"));
        first
            .finish_turn(&running, Some("engine-1"), Some("next done"), None)
            .unwrap();
    }

    #[test]
    fn rejects_a_future_schema_without_downgrading_it() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 2).unwrap();
        drop(connection);

        let error = HubStore::open(&database)
            .err()
            .expect("future schema must be rejected");
        assert!(error.contains("newer than supported"));

        let connection = Connection::open(&database).unwrap();
        let version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, 2);
    }
}
