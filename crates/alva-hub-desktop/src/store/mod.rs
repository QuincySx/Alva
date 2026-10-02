// INPUT:  rusqlite, serde, filesystem paths, UUID/time primitives
// OUTPUT: HubStore, automations/ALVA tool settings, session input-file DTOs and durable orchestration DTOs
// POS:    Hub Desktop-private multi-engine state machine, authorization snapshots, and crash repair owner.

use std::fs::File;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::permissions::PermissionProfile;
use crate::readiness::{ENGINE_CAPABILITIES, MODEL_CAPABILITIES};
use crate::session_workspace::WorkspaceKind;

pub(crate) use branch::{default_account_source, default_billing_route};
use db::{
    acquire_instance_lock, ensure_private_database_file, ensure_private_parent,
    initialize_schema_and_recover, validate_database_file,
};

mod branch;
mod automation;
mod tool_settings;
mod db;
mod handoff;
mod orchestration;
mod projects;
mod team;
mod tool_content;
mod message_journal;
mod input_attachments;
mod turn;

pub use branch::{
    EngineTarget, FallbackPolicy, PendingSwitch, StoredBranch, SwitchDisposition, SwitchOutcome,
};
pub use automation::{Automation, AutomationAction};
pub use tool_settings::{HubToolSettings, ToolComponent, ManagedMcpServer, ToolSettingsAction};
// HandoffPackage 的嵌套值类型。产品代码经 HandoffPackage 构造/读取它们，
// 仅在 redact 的构造测试里以 `crate::store::*` 全限定直接引用，故 lib 下 unused。
#[allow(unused_imports)]
pub use handoff::{
    HandoffCompletedWork, HandoffKind, HandoffPackage, HandoffPendingWork, McpSnapshot,
    SkillSnapshot, TranscriptTurn,
};
pub use projects::ProjectSummary;
pub use team::{TaskClaim, TeamRunDetail, TeamWorker, WorkerLifecycle, WorkerTask};
pub use tool_content::{ToolContentChunk, ToolContentRef};
pub use message_journal::{MessageJournalPage, MessageJournalRecord};
pub use input_attachments::{validate_input_attachments, InputAttachment, InputAttachmentRef, InputAttachmentContent};
pub(crate) use tool_content::ToolContentKind;
pub(crate) use turn::PreparedTurn;
pub use turn::{
    HubSessionDetail, HubSessionSummary, SideEffectEvidence, StoredAttempt, StoredTimelineEvent,
    StoredTurn, StoredUsage,
};

pub(crate) const SCHEMA_VERSION: i64 = 23;
pub(crate) const STATUS_RUNNING: &str = "running";
pub(crate) const STATUS_COMPLETED: &str = "completed";
pub(crate) const STATUS_FAILED: &str = "failed";
pub(crate) const STATUS_INTERRUPTED: &str = "interrupted";
pub(crate) const RESTART_REASON: &str =
    "ALVA Hub Desktop restarted before the engine attempt reached a terminal state.";

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

        let store = Self {
            path,
            connection: Arc::new(Mutex::new(connection)),
            _instance_lock: Arc::new(instance_lock),
        };
        store.apply_ready_pending_switches()?;
        Ok(store)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn lock(&self) -> Result<MutexGuard<'_, Connection>, String> {
        self.connection
            .lock()
            .map_err(|_| "Hub database lock poisoned".to_string())
    }
}

// ---- shared data-access layer (used by >=2 persistence clusters) ----

pub(crate) fn now_unix_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().min(i64::MAX as u128) as i64)
        .unwrap_or(0)
}

pub(crate) fn bounded_text(text: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let mut chars = text.chars();
    let bounded: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
}

pub(crate) fn validate_engine_target(target: &EngineTarget) -> Result<(), String> {
    EngineTarget::for_engine(&target.engine_id)?;
    if target.model_id.trim().is_empty() || target.provider_id.trim().is_empty() {
        return Err(format!(
            "engine target '{}' requires a model policy and provider/account source",
            target.engine_id
        ));
    }
    if target.account_source.trim().is_empty() || target.billing_route.trim().is_empty() {
        return Err(format!(
            "engine target '{}' requires visible account and billing metadata",
            target.engine_id
        ));
    }
    // Whitelist both vocabularies this field legitimately carries. Without this
    // a typo like "tool_calls" (the engine word is "tool_control", the model
    // word is "tools") is stored happily and then silently never satisfied.
    if let Some(unknown) = target.required_capabilities.iter().find(|capability| {
        let capability = capability.trim();
        capability.is_empty()
            || !(ENGINE_CAPABILITIES.contains(&capability)
                || MODEL_CAPABILITIES.contains(&capability))
    }) {
        return Err(format!(
            "engine target '{}' has an unknown required capability '{}'; engine capabilities are [{}], model capabilities are [{}]",
            target.engine_id,
            unknown,
            ENGINE_CAPABILITIES.join(", "),
            MODEL_CAPABILITIES.join(", ")
        ));
    }
    if target.reasoning_effort.as_ref().is_some_and(|effort| {
        !matches!(
            effort.as_str(),
            "off" | "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max" | "ultra"
        )
    }) {
        return Err(format!(
            "engine target '{}' has an invalid reasoning effort",
            target.engine_id
        ));
    }
    Ok(())
}

pub(crate) fn insert_engine_branch(
    transaction: &Transaction<'_>,
    branch_id: &str,
    product_session_id: &str,
    target: &EngineTarget,
    status: &str,
    now: i64,
) -> Result<(), String> {
    validate_engine_target(target)?;
    let required_capabilities_json = serde_json::to_string(&target.required_capabilities)
        .map_err(|error| format!("serialize Hub branch capabilities: {error}"))?;
    transaction
        .execute(
            "INSERT INTO engine_branches
               (branch_id, product_session_id, engine_id, model_id, provider_id,
                account_source, billing_route, automatic_use_authorized,
                required_capabilities_json, reasoning_effort, fast_mode,
                engine_session_id, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, ?12, ?13, ?13)",
            params![
                branch_id,
                product_session_id,
                target.engine_id,
                target.model_id,
                target.provider_id,
                target.account_source,
                target.billing_route,
                target.automatic_use_authorized,
                required_capabilities_json,
                target.reasoning_effort,
                target.fast_mode,
                status,
                now
            ],
        )
        .map_err(|error| format!("insert Hub engine branch: {error}"))?;
    Ok(())
}

pub(crate) fn query_branch_target(
    connection: &Connection,
    branch_id: &str,
) -> Result<EngineTarget, String> {
    let row = connection
        .query_row(
            "SELECT engine_id, model_id, provider_id, account_source, billing_route,
                    automatic_use_authorized, required_capabilities_json,
                    reasoning_effort, fast_mode
               FROM engine_branches WHERE branch_id = ?1",
            [branch_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, bool>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, Option<String>>(7)?,
                    row.get::<_, bool>(8)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("query Hub branch target: {error}"))?
        .ok_or_else(|| format!("Hub engine branch '{branch_id}' was not found"))?;
    let required_capabilities = serde_json::from_str(&row.6)
        .map_err(|error| format!("decode Hub branch capabilities: {error}"))?;
    Ok(EngineTarget {
        engine_id: row.0,
        model_id: row.1,
        provider_id: row.2,
        account_source: row.3,
        billing_route: row.4,
        automatic_use_authorized: row.5,
        required_capabilities,
        reasoning_effort: row.7,
        fast_mode: row.8,
    })
}

pub(crate) fn update_engine_branch_options(
    transaction: &Transaction<'_>,
    branch_id: &str,
    target: &EngineTarget,
    now: i64,
) -> Result<(), String> {
    let changed = transaction
        .execute(
            "UPDATE engine_branches
                SET reasoning_effort = ?2, fast_mode = ?3, updated_at = ?4
              WHERE branch_id = ?1",
            params![branch_id, target.reasoning_effort, target.fast_mode, now],
        )
        .map_err(|error| format!("update Hub branch model options: {error}"))?;
    if changed == 1 {
        Ok(())
    } else {
        Err(format!("Hub engine branch '{branch_id}' was not found"))
    }
}

pub(crate) fn legacy_engine_target(
    engine_id: String,
    model_id: String,
    provider_id: String,
) -> EngineTarget {
    EngineTarget {
        engine_id,
        model_id,
        provider_id,
        account_source: default_account_source(),
        billing_route: default_billing_route(),
        automatic_use_authorized: false,
        required_capabilities: Vec::new(),
        reasoning_effort: None,
        fast_mode: false,
    }
}

pub(crate) fn validate_fallback_targets(targets: &[EngineTarget]) -> Result<(), String> {
    for (index, target) in targets.iter().enumerate() {
        validate_engine_target(target)?;
        if targets[..index].iter().any(|previous| {
            previous.engine_id == target.engine_id
                && previous.model_id == target.model_id
                && previous.provider_id == target.provider_id
        }) {
            return Err(format!(
                "fallback policy repeats engine target '{}'/{}",
                target.engine_id, target.model_id
            ));
        }
    }
    Ok(())
}

pub(crate) fn query_fallback_policy(
    connection: &Connection,
    product_session_id: Option<&str>,
) -> Result<FallbackPolicy, String> {
    let row = match product_session_id {
        Some(product_session_id) => connection.query_row(
            "SELECT targets_json, updated_at FROM fallback_policies
              WHERE scope = 'session' AND product_session_id = ?1",
            [product_session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        ),
        None => connection.query_row(
            "SELECT targets_json, updated_at FROM fallback_policies
              WHERE scope = 'global' LIMIT 1",
            [],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        ),
    }
    .optional()
    .map_err(|error| format!("read Hub fallback policy: {error}"))?;
    match row {
        Some((targets_json, updated_at)) => {
            let targets = serde_json::from_str(&targets_json)
                .map_err(|error| format!("decode Hub fallback policy: {error}"))?;
            Ok(FallbackPolicy {
                targets,
                updated_at,
            })
        }
        None => Ok(FallbackPolicy {
            targets: Vec::new(),
            updated_at: 0,
        }),
    }
}

pub(crate) fn persist_fallback_policy(
    transaction: &Transaction<'_>,
    product_session_id: Option<&str>,
    policy: &FallbackPolicy,
) -> Result<(), String> {
    let targets_json = serde_json::to_string(&policy.targets)
        .map_err(|error| format!("serialize Hub fallback policy: {error}"))?;
    match product_session_id {
        Some(product_session_id) => transaction
            .execute(
                "INSERT INTO fallback_policies
                   (policy_id, scope, product_session_id, targets_json, updated_at)
                 VALUES (?1, 'session', ?2, ?3, ?4)
                 ON CONFLICT(policy_id) DO UPDATE SET
                   targets_json = excluded.targets_json, updated_at = excluded.updated_at",
                params![
                    format!("session-{product_session_id}"),
                    product_session_id,
                    targets_json,
                    policy.updated_at
                ],
            )
            .map_err(|error| format!("persist Hub session fallback policy: {error}"))?,
        None => transaction
            .execute(
                "INSERT INTO fallback_policies
                   (policy_id, scope, product_session_id, targets_json, updated_at)
                 VALUES ('global-default', 'global', NULL, ?1, ?2)
                 ON CONFLICT(policy_id) DO UPDATE SET
                   targets_json = excluded.targets_json, updated_at = excluded.updated_at",
                params![targets_json, policy.updated_at],
            )
            .map_err(|error| format!("persist global Hub fallback policy: {error}"))?,
    };
    Ok(())
}

pub(crate) fn insert_switch_timeline(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    kind: &str,
    source_branch_id: &str,
    target_branch_id: Option<&str>,
    source_engine_id: &str,
    target_engine_id: &str,
    reason: &str,
    now: i64,
) -> Result<(), String> {
    transaction
        .execute(
            "INSERT INTO timeline_events
               (event_id, product_session_id, kind, source_branch_id,
                target_branch_id, source_engine_id, target_engine_id, reason, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            params![
                uuid::Uuid::new_v4().to_string(),
                product_session_id,
                kind,
                source_branch_id,
                target_branch_id,
                source_engine_id,
                target_engine_id,
                reason,
                now
            ],
        )
        .map_err(|error| format!("persist Hub switch timeline event: {error}"))?;
    Ok(())
}

pub(crate) fn query_pending_switch(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Option<PendingSwitch>, String> {
    let pending = connection
        .query_row(
            "SELECT switch_id, source_branch_id, target_branch_id, target_engine_id,
                    target_model_id, target_provider_id, reason, requested_at
               FROM engine_switches
              WHERE product_session_id = ?1 AND status = 'queued'",
            [product_session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<String>>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("query pending Hub engine switch: {error}"))?;
    let Some((
        switch_id,
        source_branch_id,
        target_branch_id,
        engine_id,
        model_id,
        provider_id,
        reason,
        requested_at,
    )) = pending
    else {
        return Ok(None);
    };
    let target = match target_branch_id {
        Some(branch_id) => query_branch_target(connection, &branch_id)?,
        None => legacy_engine_target(engine_id, model_id, provider_id),
    };
    Ok(Some(PendingSwitch {
        switch_id,
        source_branch_id,
        target,
        reason,
        requested_at,
    }))
}

pub(crate) fn query_session(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Option<HubSessionSummary>, String> {
    connection
        .query_row(
            "SELECT hs.product_session_id, hs.workspace, hs.active_branch_id,
                    eb.engine_id, eb.model_id, eb.provider_id, eb.engine_session_id,
                    hs.status, hs.updated_at,
                    (SELECT prompt FROM turns t
                       WHERE t.product_session_id = hs.product_session_id
                       ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1),
                    (SELECT final_result FROM turns t
                       WHERE t.product_session_id = hs.product_session_id
                       ORDER BY t.created_at DESC, t.rowid DESC LIMIT 1),
                    hs.permission_profile, hs.include_full_transcript,
                    hs.workspace_kind, hs.project_id
               FROM hub_sessions hs
               JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
              WHERE hs.product_session_id = ?1",
            [product_session_id],
            decode_session,
        )
        .optional()
        .map_err(|error| format!("query Hub session '{product_session_id}': {error}"))
}

pub(crate) fn decode_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<HubSessionSummary> {
    let profile_text: String = row.get(11)?;
    let workspace_kind_text: String = row.get(13)?;
    Ok(HubSessionSummary {
        product_session_id: row.get(0)?,
        workspace: row.get(1)?,
        workspace_kind: WorkspaceKind::parse(&workspace_kind_text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(13, rusqlite::types::Type::Text, error.into())
        })?,
        active_branch_id: row.get(2)?,
        engine_id: row.get(3)?,
        model_id: row.get(4)?,
        provider_id: row.get(5)?,
        engine_session_id: row.get(6)?,
        status: row.get(7)?,
        updated_at: row.get(8)?,
        last_prompt: row.get(9)?,
        last_result: row.get(10)?,
        permission_profile: PermissionProfile::parse(&profile_text).map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(11, rusqlite::types::Type::Text, error.into())
        })?,
        include_full_transcript: row.get(12)?,
        project_id: row.get(14)?,
    })
}

pub(crate) fn query_session_memory(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Vec<String>, String> {
    let mut statement = connection
        .prepare(
            "SELECT entry FROM hub_memory
              WHERE product_session_id = ?1
              ORDER BY memory_id ASC",
        )
        .map_err(|error| format!("prepare session memory query: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("query session memory: {error}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode session memory: {error}"))
}

#[cfg(test)]
mod tests;
