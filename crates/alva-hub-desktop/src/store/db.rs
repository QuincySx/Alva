// INPUT:  connection/schema/file-path primitives
// OUTPUT: schema initialization + migration, instance lock, private file enforcement, crash repair
// POS:    storage kernel of the Hub store — owns Connection + file security + repair.

use std::fs::{self, File, OpenOptions};
use std::path::Path;

use rusqlite::{params, Connection, Transaction, TransactionBehavior};

use super::{now_unix_ms, RESTART_REASON, SCHEMA_VERSION, STATUS_INTERRUPTED, STATUS_RUNNING};

pub(super) fn initialize_schema_and_recover(
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

    if schema_version < 3 {
        transaction
            .execute_batch(
                "ALTER TABLE engine_branches
                   ADD COLUMN model_id TEXT NOT NULL DEFAULT 'engine-default';
                 ALTER TABLE engine_branches
                   ADD COLUMN provider_id TEXT NOT NULL DEFAULT 'engine-default';
                 UPDATE engine_branches
                    SET model_id = 'workspace-config', provider_id = 'workspace-config'
                  WHERE engine_id = 'alva';
                 CREATE UNIQUE INDEX idx_engine_branches_target
                   ON engine_branches(product_session_id, engine_id, model_id, provider_id);
                 CREATE TABLE handoff_packages (
                   handoff_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   source_branch_id TEXT NOT NULL,
                   target_branch_id TEXT NOT NULL,
                   reason TEXT NOT NULL,
                   package_json TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   used_at INTEGER,
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id),
                   FOREIGN KEY(source_branch_id) REFERENCES engine_branches(branch_id),
                   FOREIGN KEY(target_branch_id) REFERENCES engine_branches(branch_id)
                 );
                 CREATE INDEX idx_handoffs_target_created
                   ON handoff_packages(target_branch_id, created_at);
                 CREATE TABLE engine_switches (
                   switch_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   source_branch_id TEXT NOT NULL,
                   target_branch_id TEXT,
                   target_engine_id TEXT NOT NULL,
                   target_model_id TEXT NOT NULL,
                   target_provider_id TEXT NOT NULL,
                   reason TEXT NOT NULL,
                   status TEXT NOT NULL,
                   handoff_id TEXT,
                   requested_at INTEGER NOT NULL,
                   applied_at INTEGER,
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id),
                   FOREIGN KEY(source_branch_id) REFERENCES engine_branches(branch_id),
                   FOREIGN KEY(target_branch_id) REFERENCES engine_branches(branch_id),
                   FOREIGN KEY(handoff_id) REFERENCES handoff_packages(handoff_id)
                 );
                 CREATE UNIQUE INDEX idx_engine_switches_one_queued
                   ON engine_switches(product_session_id) WHERE status = 'queued';
                 CREATE TABLE timeline_events (
                   event_id TEXT PRIMARY KEY,
                   product_session_id TEXT NOT NULL,
                   kind TEXT NOT NULL,
                   source_branch_id TEXT NOT NULL,
                   target_branch_id TEXT,
                   source_engine_id TEXT NOT NULL,
                   target_engine_id TEXT NOT NULL,
                   reason TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id),
                   FOREIGN KEY(source_branch_id) REFERENCES engine_branches(branch_id),
                   FOREIGN KEY(target_branch_id) REFERENCES engine_branches(branch_id)
                 );
                 CREATE INDEX idx_timeline_events_product_created
                   ON timeline_events(product_session_id, created_at);",
            )
            .map_err(|error| format!("migrate Hub database to schema 3: {error}"))?;
    }

    if schema_version < 4 {
        transaction
            .execute_batch(
                "ALTER TABLE engine_branches
                   ADD COLUMN watermark_turn_rowid INTEGER;
                 ALTER TABLE handoff_packages
                   ADD COLUMN kind TEXT NOT NULL DEFAULT 'full';
                 CREATE INDEX idx_handoffs_target_kind_created
                   ON handoff_packages(target_branch_id, kind, created_at);",
            )
            .map_err(|error| format!("migrate Hub database to schema 4: {error}"))?;
    }

    if schema_version < 5 {
        transaction
            .execute_batch(
                "CREATE TABLE fallback_policies (
                   policy_id TEXT PRIMARY KEY,
                   scope TEXT NOT NULL CHECK(scope IN ('global', 'session')),
                   product_session_id TEXT UNIQUE,
                   targets_json TEXT NOT NULL,
                   updated_at INTEGER NOT NULL,
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id)
                 );
                 INSERT INTO fallback_policies
                   (policy_id, scope, product_session_id, targets_json, updated_at)
                 VALUES ('global-default', 'global', NULL, '[]', 0);
                 INSERT INTO fallback_policies
                   (policy_id, scope, product_session_id, targets_json, updated_at)
                 SELECT 'session-' || product_session_id, 'session', product_session_id, '[]', 0
                   FROM hub_sessions;",
            )
            .map_err(|error| format!("migrate Hub database to schema 5: {error}"))?;
        transaction
            .execute(
                "UPDATE fallback_policies SET updated_at = ?1
                  WHERE scope = 'session' AND updated_at = 0",
                [now],
            )
            .map_err(|error| format!("timestamp migrated Hub fallback policies: {error}"))?;
    }

    if schema_version < 6 {
        // Existing sessions keep the pre-profile de-facto behavior:
        // workspace-bounded side effects without prompting.
        transaction
            .execute_batch(
                "ALTER TABLE hub_sessions
                   ADD COLUMN permission_profile TEXT NOT NULL DEFAULT 'workspace_write';",
            )
            .map_err(|error| format!("migrate Hub database to schema 6: {error}"))?;
    }

    if schema_version < 7 {
        // Transcript sharing is opt-in; existing sessions stay filtered-only.
        transaction
            .execute_batch(
                "ALTER TABLE hub_sessions
                   ADD COLUMN include_full_transcript INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(|error| format!("migrate Hub database to schema 7: {error}"))?;
    }

    if schema_version < 8 {
        transaction
            .execute_batch(
                "CREATE TABLE team_runs (
                   team_run_id TEXT PRIMARY KEY,
                   lead_session_id TEXT NOT NULL,
                   status TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   FOREIGN KEY(lead_session_id) REFERENCES hub_sessions(product_session_id)
                 );
                 CREATE TABLE team_workers (
                   worker_id TEXT PRIMARY KEY,
                   team_run_id TEXT NOT NULL,
                   worker_session_id TEXT NOT NULL UNIQUE,
                   created_at INTEGER NOT NULL,
                   FOREIGN KEY(team_run_id) REFERENCES team_runs(team_run_id),
                   FOREIGN KEY(worker_session_id) REFERENCES hub_sessions(product_session_id)
                 );
                 CREATE TABLE worker_tasks (
                   task_id TEXT PRIMARY KEY,
                   team_run_id TEXT NOT NULL,
                   worker_id TEXT NOT NULL,
                   scope TEXT NOT NULL,
                   status TEXT NOT NULL,
                   result TEXT,
                   changed_files_json TEXT NOT NULL DEFAULT '[]',
                   verification_json TEXT NOT NULL DEFAULT '[]',
                   created_at INTEGER NOT NULL,
                   completed_at INTEGER,
                   FOREIGN KEY(team_run_id) REFERENCES team_runs(team_run_id),
                   FOREIGN KEY(worker_id) REFERENCES team_workers(worker_id)
                 );",
            )
            .map_err(|error| format!("migrate Hub database to schema 8: {error}"))?;
    }

    if schema_version < 9 {
        // Claim time, so a queued task's evidence window starts when it
        // actually began running — not when it was enqueued behind others.
        transaction
            .execute_batch("ALTER TABLE worker_tasks ADD COLUMN started_at INTEGER;")
            .map_err(|error| format!("migrate Hub database to schema 9: {error}"))?;
    }

    if schema_version < 10 {
        transaction
            .execute_batch(
                "ALTER TABLE worker_tasks
                   ADD COLUMN claims_json TEXT NOT NULL DEFAULT '[]';",
            )
            .map_err(|error| format!("migrate Hub database to schema 10: {error}"))?;
    }

    if schema_version < 11 {
        // Isolated workers keep their managed worktree identity here; NULL
        // means the worker shares the session workspace.
        transaction
            .execute_batch(
                "ALTER TABLE team_workers ADD COLUMN worktree_path TEXT;
                 ALTER TABLE team_workers ADD COLUMN worktree_branch TEXT;",
            )
            .map_err(|error| format!("migrate Hub database to schema 11: {error}"))?;
    }

    if schema_version < 12 {
        // Worker lifecycle is independent of runtime liveness and of task
        // terminal state: existing workers start active.
        transaction
            .execute_batch(
                "ALTER TABLE team_workers
                   ADD COLUMN lifecycle TEXT NOT NULL DEFAULT 'active';",
            )
            .map_err(|error| format!("migrate Hub database to schema 12: {error}"))?;
    }

    if schema_version < 13 {
        // Autonomy is a separate, explicit opt-in — never implied by
        // creating a run or a worker.
        transaction
            .execute_batch(
                "ALTER TABLE team_runs ADD COLUMN autonomous INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(|error| format!("migrate Hub database to schema 13: {error}"))?;
    }

    if schema_version < 14 {
        // Cross-engine session memory: corrections and preferences a user
        // wants every engine in this session to respect. It travels with
        // every handoff so a fallback engine starts with the accumulated
        // memory, not from zero.
        transaction
            .execute_batch(
                "CREATE TABLE hub_memory (
                   memory_id INTEGER PRIMARY KEY AUTOINCREMENT,
                   product_session_id TEXT NOT NULL,
                   entry TEXT NOT NULL,
                   created_at INTEGER NOT NULL,
                   FOREIGN KEY(product_session_id) REFERENCES hub_sessions(product_session_id)
                 );
                 CREATE INDEX idx_hub_memory_session
                   ON hub_memory(product_session_id, memory_id);",
            )
            .map_err(|error| format!("migrate Hub database to schema 14: {error}"))?;
    }

    if schema_version < 15 {
        // A branch is the durable binding to one exact EngineTarget. Legacy
        // rows have only target identity, so their unavailable metadata is
        // represented explicitly instead of being guessed from current
        // provider configuration.
        transaction
            .execute_batch(
                "ALTER TABLE engine_branches
                   ADD COLUMN account_source TEXT NOT NULL DEFAULT 'Unconfigured account';
                 ALTER TABLE engine_branches
                   ADD COLUMN billing_route TEXT NOT NULL DEFAULT 'Unconfigured billing route';
                 ALTER TABLE engine_branches
                   ADD COLUMN automatic_use_authorized INTEGER NOT NULL DEFAULT 0;
                 ALTER TABLE engine_branches
                   ADD COLUMN required_capabilities_json TEXT NOT NULL DEFAULT '[]';",
            )
            .map_err(|error| format!("migrate Hub database to schema 15: {error}"))?;
    }

    if schema_version < 16 {
        // Model execution options belong to the durable branch so resumed
        // turns rebuild the engine with the same effort and service tier.
        transaction
            .execute_batch(
                "ALTER TABLE engine_branches ADD COLUMN reasoning_effort TEXT;
                 ALTER TABLE engine_branches
                   ADD COLUMN fast_mode INTEGER NOT NULL DEFAULT 0;",
            )
            .map_err(|error| format!("migrate Hub database to schema 16: {error}"))?;
    }

    if schema_version < 17 {
        // Legacy sessions always pointed at a user-selected project. Only
        // the explicit managed-workspace command creates session-kind rows.
        transaction
            .execute_batch(
                "ALTER TABLE hub_sessions
                   ADD COLUMN workspace_kind TEXT NOT NULL DEFAULT 'project';",
            )
            .map_err(|error| format!("migrate Hub database to schema 17: {error}"))?;
    }

    if schema_version < 18 {
        // H04: the Project aggregate. Purely additive — a new table plus a
        // nullable session column — so pre-18 builds refuse the schema via
        // the version gate without losing data, and legacy sessions read
        // back unchanged with a NULL project.
        transaction
            .execute_batch(
                "CREATE TABLE projects (
                   project_id TEXT PRIMARY KEY,
                   display_name TEXT NOT NULL,
                   workspace_path TEXT NOT NULL,
                   workspace_canonical TEXT NOT NULL UNIQUE,
                   created_at INTEGER NOT NULL,
                   updated_at INTEGER NOT NULL
                 );
                 ALTER TABLE hub_sessions
                   ADD COLUMN project_id TEXT REFERENCES projects(project_id);
                 CREATE INDEX idx_hub_sessions_project
                   ON hub_sessions(project_id);",
            )
            .map_err(|error| format!("migrate Hub database to schema 18: {error}"))?;
    }

    if schema_version < 19 {
        transaction
            .execute_batch(
                "CREATE TABLE tool_contents (
                content_id TEXT PRIMARY KEY,
                attempt_id TEXT NOT NULL REFERENCES engine_attempts(attempt_id),
                tool_call_id TEXT NOT NULL,
                kind TEXT NOT NULL CHECK(kind IN ('input', 'output')),
                total_bytes INTEGER NOT NULL,
                chunk_count INTEGER NOT NULL
             );
             CREATE INDEX idx_tool_contents_attempt ON tool_contents(attempt_id);
             CREATE TABLE tool_content_chunks (
                content_id TEXT NOT NULL REFERENCES tool_contents(content_id),
                chunk_index INTEGER NOT NULL,
                body TEXT NOT NULL,
                PRIMARY KEY(content_id, chunk_index)
             );",
            )
            .map_err(|error| format!("migrate Hub database to schema 19: {error}"))?;
    }

    repair_interrupted_attempts(&transaction, now)?;
    transaction
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(|error| format!("record Hub database schema version: {error}"))?;
    transaction
        .commit()
        .map_err(|error| format!("commit Hub database migration: {error}"))
}

pub(super) fn repair_interrupted_attempts(
    transaction: &Transaction<'_>,
    now: i64,
) -> Result<(), String> {
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
    transaction
        .execute(
            "UPDATE worker_tasks
                SET status = ?1, result = COALESCE(result, ?2),
                    completed_at = COALESCE(completed_at, ?3)
              WHERE status = ?4",
            params![STATUS_INTERRUPTED, RESTART_REASON, now, STATUS_RUNNING],
        )
        .map_err(|error| format!("repair interrupted Hub worker tasks: {error}"))?;
    Ok(())
}

pub(super) fn acquire_instance_lock(database_path: &Path) -> Result<File, String> {
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

pub(super) fn ensure_private_parent(path: &Path) -> Result<(), String> {
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

pub(super) fn validate_database_file(path: &Path) -> Result<(), String> {
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

pub(super) fn ensure_private_database_file(path: &Path) -> Result<(), String> {
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
pub(super) fn set_private_dir_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("chmod 0700 {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(super) fn set_private_dir_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
pub(super) fn set_private_file_permissions(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("chmod 0600 {}: {error}", path.display()))
}

#[cfg(not(unix))]
pub(super) fn set_private_file_permissions(_path: &Path) -> Result<(), String> {
    Ok(())
}
