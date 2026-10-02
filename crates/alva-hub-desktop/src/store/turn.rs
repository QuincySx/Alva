// INPUT:  session/turn/attempt rows in the Hub SQLite schema
// OUTPUT: session/attempt DTOs, resumed turns with atomic automation claims, lifecycle and side-effect merging
// POS:    session timeline persistence cluster of the Hub store.

use std::path::{Path, PathBuf};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};

use serde::{Deserialize, Serialize};

use crate::permissions::PermissionProfile;
use crate::session_workspace::WorkspaceKind;

use super::branch::{EngineTarget, FallbackPolicy, PendingSwitch, StoredBranch};
use super::handoff::{
    build_handoff_package, persist_handoff_package, query_handoffs, query_latest_handoff,
    source_branch_for_handoff, HandoffKind, HandoffPackage,
};
use super::{
    decode_session, insert_engine_branch, insert_switch_timeline, now_unix_ms,
    persist_fallback_policy, query_branch_target, query_fallback_policy, query_pending_switch,
    query_session, query_session_memory, update_engine_branch_options, validate_engine_target,
    STATUS_COMPLETED, STATUS_FAILED, STATUS_RUNNING,
};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HubSessionSummary {
    pub product_session_id: String,
    pub workspace: String,
    pub workspace_kind: WorkspaceKind,
    pub active_branch_id: String,
    pub engine_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub engine_session_id: Option<String>,
    pub status: String,
    pub updated_at: i64,
    pub last_prompt: Option<String>,
    pub last_result: Option<String>,
    /// Session-stable permission contract; branch switches never change it.
    pub permission_profile: PermissionProfile,
    /// Whether handoffs may attach the full (still filtered) transcript.
    pub include_full_transcript: bool,
    /// Owning Project, when the session was created in (or attached to)
    /// project mode. NULL sessions surface in conversation mode (H08).
    pub project_id: Option<String>,
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

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SideEffectEvidence {
    #[serde(default)]
    pub tool_input_ref: Option<super::ToolContentRef>,
    #[serde(default)]
    pub tool_output_ref: Option<super::ToolContentRef>,
    /// Product first-observation clock and attempt-local order; absent in legacy journals.
    #[serde(default)]
    pub first_seen_at: Option<i64>,
    #[serde(default)]
    pub sequence: Option<u64>,
    pub tool_call_id: String,
    pub tool_name: String,
    pub succeeded: Option<bool>,
    pub duration_ms: Option<u64>,
    /// File the tool touched, when the adapter surfaced one in its input.
    #[serde(default)]
    pub path: Option<String>,
    /// Bounded JSON excerpt of the tool input (truncated, 2k chars).
    #[serde(default)]
    pub tool_input: Option<String>,
    /// Bounded excerpt of the tool result (truncated, 2k chars).
    #[serde(default)]
    pub tool_output: Option<String>,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoredTimelineEvent {
    pub event_id: String,
    pub kind: String,
    pub source_branch_id: String,
    pub target_branch_id: Option<String>,
    pub source_engine_id: String,
    pub target_engine_id: String,
    pub reason: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct HubSessionDetail {
    pub session: HubSessionSummary,
    pub turns: Vec<StoredTurn>,
    pub branches: Vec<StoredBranch>,
    pub handoffs: Vec<HandoffPackage>,
    pub timeline_events: Vec<StoredTimelineEvent>,
    pub pending_switch: Option<PendingSwitch>,
    pub fallback_policy: FallbackPolicy,
    /// Session-scoped memory entries, oldest first. Separate from the
    /// handoff snapshot so the drawer can render the live list.
    pub memory: Vec<String>,
}

#[derive(Debug, Clone)]
pub(crate) struct PreparedTurn {
    pub product_session_id: String,
    pub branch_id: String,
    pub turn_id: String,
    pub attempt_id: String,
    pub workspace: PathBuf,
    pub engine_session_id: Option<String>,
    pub target: EngineTarget,
    pub handoff: Option<HandoffPackage>,
    pub native_resume: bool,
    pub permission_profile: PermissionProfile,
}

impl super::HubStore {
    pub fn list_sessions(&self) -> Result<Vec<HubSessionSummary>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
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
        let branches = query_branches(&connection, product_session_id)?;
        let handoffs = query_handoffs(&connection, product_session_id)?;
        let timeline_events = query_timeline_events(&connection, product_session_id)?;
        let pending_switch = query_pending_switch(&connection, product_session_id)?;
        let fallback_policy = query_fallback_policy(&connection, Some(product_session_id))?;
        let memory = query_session_memory(&connection, product_session_id)?;
        Ok(Some(HubSessionDetail {
            session,
            turns,
            branches,
            handoffs,
            timeline_events,
            pending_switch,
            fallback_policy,
            memory,
        }))
    }

    pub fn add_session_memory(
        &self,
        product_session_id: &str,
        entry: &str,
    ) -> Result<Vec<String>, String> {
        let entry = entry.trim();
        if entry.is_empty() {
            return Err("session memory entry must not be empty".to_string());
        }
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin session memory transaction: {error}"))?;
        let session_exists: bool = transaction
            .query_row(
                "SELECT 1 FROM hub_sessions WHERE product_session_id = ?1",
                [product_session_id],
                |_| Ok(true),
            )
            .optional()
            .map_err(|error| format!("check session before adding memory: {error}"))?
            .unwrap_or(false);
        if !session_exists {
            return Err(format!("Hub session '{product_session_id}' does not exist"));
        }
        transaction
            .execute(
                "INSERT INTO hub_memory (product_session_id, entry, created_at)
                     VALUES (?1, ?2, ?3)",
                params![product_session_id, entry, now_unix_ms()],
            )
            .map_err(|error| format!("insert session memory: {error}"))?;
        let memory = query_session_memory(&transaction, product_session_id)?;
        transaction
            .commit()
            .map_err(|error| format!("commit session memory: {error}"))?;
        Ok(memory)
    }

    pub fn latest_attempt_outcome(
        &self,
        engine_id: &str,
    ) -> Result<Option<(String, Option<String>)>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT status, failure_reason FROM engine_attempts
                      WHERE engine_id = ?1 AND status != ?2
                      ORDER BY started_at DESC, completed_at DESC LIMIT 1",
                params![engine_id, STATUS_RUNNING],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()
            .map_err(|error| format!("query latest Hub attempt outcome: {error}"))
    }

    #[cfg(test)]
    pub(crate) fn prepare_new_turn(
        &self,
        product_session_id: &str,
        workspace: &Path,
        prompt: &str,
        target: &EngineTarget,
        permission_profile: PermissionProfile,
    ) -> Result<PreparedTurn, String> {
        self.prepare_new_turn_with_workspace_kind(
            product_session_id,
            workspace,
            prompt,
            target,
            permission_profile,
            WorkspaceKind::Project,
        )
    }

    pub(crate) fn prepare_new_turn_with_workspace_kind(
        &self,
        product_session_id: &str,
        workspace: &Path,
        prompt: &str,
        target: &EngineTarget,
        permission_profile: PermissionProfile,
        workspace_kind: WorkspaceKind,
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
                       (product_session_id, workspace, active_branch_id, status,
                        permission_profile, workspace_kind, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7)",
                params![
                    product_session_id,
                    workspace_text,
                    branch_id,
                    STATUS_RUNNING,
                    permission_profile.as_str(),
                    workspace_kind.as_str(),
                    now
                ],
            )
            .map_err(|error| format!("insert Hub session: {error}"))?;
        insert_engine_branch(
            &transaction,
            &branch_id,
            product_session_id,
            target,
            "active",
            now,
        )?;
        let global_policy = query_fallback_policy(&transaction, None)?;
        let session_policy = FallbackPolicy {
            targets: global_policy.targets,
            updated_at: now,
        };
        persist_fallback_policy(&transaction, Some(product_session_id), &session_policy)?;
        insert_turn_and_attempt(
            &transaction,
            &turn_id,
            &attempt_id,
            product_session_id,
            &branch_id,
            &target.engine_id,
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
            target: target.clone(),
            handoff: None,
            native_resume: false,
            permission_profile,
        })
    }

    pub(crate) fn prepare_resumed_turn(
        &self,
        product_session_id: &str,
        prompt: &str,
    ) -> Result<PreparedTurn, String> {
        self.prepare_resumed_turn_for_automation(product_session_id, prompt, None)
    }

    pub(crate) fn prepare_resumed_turn_for_automation(
        &self,
        product_session_id: &str,
        prompt: &str,
        automation_id: Option<&str>,
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
        if let Some(id) = automation_id {
            if let Err(reason) = super::automation::authorize_claim(&transaction, id, &session, prompt, now) {
                transaction.commit().map_err(|error| format!("commit automation veto: {error}"))?;
                return Err(reason);
            }
        }
        if session.status == STATUS_RUNNING {
            return Err(format!(
                "Hub session '{product_session_id}' is already running"
            ));
        }
        let engine_session_id = session
            .engine_session_id
            .clone()
            .filter(|id| !id.is_empty());
        let target = query_branch_target(&transaction, &session.active_branch_id)?;
        let watermark_turn_rowid = transaction
            .query_row(
                "SELECT watermark_turn_rowid FROM engine_branches WHERE branch_id = ?1",
                [&session.active_branch_id],
                |row| row.get::<_, Option<i64>>(0),
            )
            .map_err(|error| format!("read Hub branch watermark: {error}"))?;
        let handoff = if engine_session_id.is_some() {
            let prior = query_latest_handoff(&transaction, &session.active_branch_id, None)?;
            let source = prior
                .as_ref()
                .map(|package| package.source.clone())
                .unwrap_or_else(|| target.clone());
            let reason = prior
                .as_ref()
                .map(|package| package.reason.as_str())
                .unwrap_or("resume parked native engine branch");
            let package = build_handoff_package(
                &transaction,
                product_session_id,
                &source_branch_for_handoff(&prior, &session.active_branch_id),
                &session.active_branch_id,
                source,
                target.clone(),
                reason,
                now,
                HandoffKind::Delta,
                watermark_turn_rowid,
            )?;
            persist_handoff_package(&transaction, &package)?;
            Some(package)
        } else {
            query_latest_handoff(
                &transaction,
                &session.active_branch_id,
                Some(HandoffKind::Full),
            )?
        };
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
            &session.engine_id,
            prompt,
            now,
        )?;
        if let Some(id) = automation_id {
            transaction.execute(
                "UPDATE automations SET running_turn_id = ?2, next_run_at = NULL,
                     last_run_at = ?3, last_status = 'running', last_error = NULL,
                     revision = revision + 1 WHERE automation_id = ?1",
                params![id, turn_id, now],
            ).map_err(|error| format!("claim automation turn: {error}"))?;
        }
        if let Some(package) = &handoff {
            transaction
                .execute(
                    "UPDATE handoff_packages SET used_at = COALESCE(used_at, ?2)
                          WHERE handoff_id = ?1",
                    params![package.handoff_id, now],
                )
                .map_err(|error| format!("mark Hub handoff consumed: {error}"))?;
        }
        transaction
            .commit()
            .map_err(|error| format!("commit Hub resume transaction: {error}"))?;

        let native_resume = engine_session_id.is_some();
        Ok(PreparedTurn {
            product_session_id: product_session_id.to_string(),
            permission_profile: session.permission_profile,
            branch_id: session.active_branch_id,
            turn_id,
            attempt_id,
            workspace: PathBuf::from(session.workspace),
            engine_session_id,
            target,
            handoff,
            native_resume,
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

    pub(crate) fn upgrade_failed_native_resume(
        &self,
        prepared: &PreparedTurn,
        failure_reason: &str,
    ) -> Result<PreparedTurn, String> {
        if !prepared.native_resume {
            return Err("cannot upgrade a fresh engine attempt to a full handoff".to_string());
        }
        let now = now_unix_ms();
        let fresh_attempt_id = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin native resume upgrade transaction: {error}"))?;
        let session =
            query_session(&transaction, &prepared.product_session_id)?.ok_or_else(|| {
                format!(
                    "Hub session '{}' was not found",
                    prepared.product_session_id
                )
            })?;
        if session.active_branch_id != prepared.branch_id {
            return Err("cannot upgrade a non-active Hub engine branch".to_string());
        }
        let changed = transaction
            .execute(
                "UPDATE engine_attempts
                        SET status = ?2, failure_reason = ?3, completed_at = ?4
                      WHERE attempt_id = ?1 AND status = ?5",
                params![
                    prepared.attempt_id,
                    STATUS_FAILED,
                    failure_reason,
                    now,
                    STATUS_RUNNING
                ],
            )
            .map_err(|error| format!("record failed native resume attempt: {error}"))?;
        if changed != 1 {
            return Err(format!(
                "Hub engine attempt '{}' was not in running state",
                prepared.attempt_id
            ));
        }
        transaction
            .execute(
                "UPDATE engine_branches
                        SET engine_session_id = NULL, updated_at = ?2
                      WHERE branch_id = ?1",
                params![prepared.branch_id, now],
            )
            .map_err(|error| format!("clear invalid native session id: {error}"))?;
        let package = build_handoff_package(
            &transaction,
            &prepared.product_session_id,
            &prepared.branch_id,
            &prepared.branch_id,
            prepared.target.clone(),
            prepared.target.clone(),
            &format!("Native session resume failed: {failure_reason}"),
            now,
            HandoffKind::Full,
            None,
        )?;
        persist_handoff_package(&transaction, &package)?;
        transaction
            .execute(
                "INSERT INTO engine_attempts
                       (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                        status, failure_reason, usage_json, side_effects_json,
                        started_at, completed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, '[]', ?7, NULL)",
                params![
                    fresh_attempt_id,
                    prepared.turn_id,
                    prepared.product_session_id,
                    prepared.branch_id,
                    prepared.target.engine_id,
                    STATUS_RUNNING,
                    now
                ],
            )
            .map_err(|error| format!("create fresh Hub engine attempt: {error}"))?;
        transaction
            .execute(
                "UPDATE hub_sessions SET status = ?2, updated_at = ?3
                      WHERE product_session_id = ?1",
                params![prepared.product_session_id, STATUS_RUNNING, now],
            )
            .map_err(|error| format!("keep Hub session running after resume upgrade: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit native resume upgrade transaction: {error}"))?;

        Ok(PreparedTurn {
            product_session_id: prepared.product_session_id.clone(),
            branch_id: prepared.branch_id.clone(),
            turn_id: prepared.turn_id.clone(),
            attempt_id: fresh_attempt_id,
            workspace: prepared.workspace.clone(),
            engine_session_id: None,
            target: prepared.target.clone(),
            handoff: Some(package),
            native_resume: false,
            permission_profile: prepared.permission_profile,
        })
    }

    /// Atomically continue the same Turn on an authorized fallback target:
    /// the running source attempt is marked failed, the source branch parks
    /// with a watermark, the target branch is created or re-activated, a Full
    /// handoff is persisted BEFORE any execution, a serial attempt starts on
    /// the target, and an `engine_fallback` timeline fact records
    /// source/target/reason/handoff summary. If the target later fails to
    /// start, all of this state survives for manual recovery.
    pub(crate) fn fallback_failed_attempt(
        &self,
        prepared: &PreparedTurn,
        failure_reason: &str,
        target: &EngineTarget,
    ) -> Result<PreparedTurn, String> {
        validate_engine_target(target)?;
        if target.engine_id == prepared.target.engine_id
            && target.model_id == prepared.target.model_id
            && target.provider_id == prepared.target.provider_id
        {
            return Err("automatic fallback requires a different engine target".to_string());
        }
        let now = now_unix_ms();
        let next_attempt_id = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub fallback transaction: {error}"))?;
        let session =
            query_session(&transaction, &prepared.product_session_id)?.ok_or_else(|| {
                format!(
                    "Hub session '{}' was not found",
                    prepared.product_session_id
                )
            })?;
        if session.active_branch_id != prepared.branch_id {
            return Err("cannot fall back from a non-active Hub engine branch".to_string());
        }
        if !query_fallback_policy(&transaction, Some(&prepared.product_session_id))?
            .targets
            .iter()
            .any(|candidate| {
                candidate.engine_id == target.engine_id
                    && candidate.model_id == target.model_id
                    && candidate.provider_id == target.provider_id
                    && candidate.automatic_use_authorized
            })
        {
            return Err(format!(
                "engine '{}' is not authorized for automatic fallback in this session",
                target.engine_id
            ));
        }
        let changed = transaction
            .execute(
                "UPDATE engine_attempts
                        SET status = ?2, failure_reason = ?3, completed_at = ?4
                      WHERE attempt_id = ?1 AND status = ?5",
                params![
                    prepared.attempt_id,
                    STATUS_FAILED,
                    failure_reason,
                    now,
                    STATUS_RUNNING
                ],
            )
            .map_err(|error| format!("record failed Hub fallback source attempt: {error}"))?;
        if changed != 1 {
            return Err(format!(
                "Hub engine attempt '{}' was not in running state",
                prepared.attempt_id
            ));
        }

        let existing_target = transaction
            .query_row(
                "SELECT branch_id FROM engine_branches
                      WHERE product_session_id = ?1 AND engine_id = ?2
                        AND model_id = ?3 AND provider_id = ?4",
                params![
                    prepared.product_session_id,
                    target.engine_id,
                    target.model_id,
                    target.provider_id
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("find Hub fallback target branch: {error}"))?;
        let (target_branch_id, branch_target) = match existing_target {
            Some(branch_id) => {
                update_engine_branch_options(&transaction, &branch_id, target, now)?;
                let branch_target = query_branch_target(&transaction, &branch_id)?;
                (branch_id, branch_target)
            }
            None => {
                let branch_id = uuid::Uuid::new_v4().to_string();
                insert_engine_branch(
                    &transaction,
                    &branch_id,
                    &prepared.product_session_id,
                    target,
                    "parked",
                    now,
                )?;
                (branch_id, target.clone())
            }
        };

        // The watermark must exclude the in-flight turn: the source engine
        // never saw its result — the fallback target completes it — so a later
        // delta on returning to the source branch has to include this turn.
        transaction
            .execute(
                "UPDATE engine_branches
                        SET status = 'parked', updated_at = ?2,
                            watermark_turn_rowid = (
                              SELECT MAX(t.rowid) FROM turns t
                               WHERE t.product_session_id = ?1 AND t.turn_id <> ?4
                            )
                      WHERE branch_id = ?3",
                params![
                    prepared.product_session_id,
                    now,
                    prepared.branch_id,
                    prepared.turn_id
                ],
            )
            .map_err(|error| format!("park Hub fallback source branch: {error}"))?;
        transaction
            .execute(
                "UPDATE engine_branches SET status = 'parked', updated_at = ?2
                      WHERE product_session_id = ?1 AND branch_id <> ?3",
                params![prepared.product_session_id, now, prepared.branch_id],
            )
            .map_err(|error| format!("park inactive Hub engine branches: {error}"))?;
        transaction
            .execute(
                "UPDATE engine_branches SET status = 'active', updated_at = ?2
                      WHERE branch_id = ?1",
                params![target_branch_id, now],
            )
            .map_err(|error| format!("activate Hub fallback target branch: {error}"))?;
        transaction
            .execute(
                "UPDATE hub_sessions
                        SET active_branch_id = ?2, status = ?3, updated_at = ?4
                      WHERE product_session_id = ?1",
                params![
                    prepared.product_session_id,
                    target_branch_id,
                    STATUS_RUNNING,
                    now
                ],
            )
            .map_err(|error| format!("activate Hub fallback session branch: {error}"))?;

        let package = build_handoff_package(
            &transaction,
            &prepared.product_session_id,
            &prepared.branch_id,
            &target_branch_id,
            prepared.target.clone(),
            branch_target.clone(),
            failure_reason,
            now,
            HandoffKind::Full,
            None,
        )?;
        persist_handoff_package(&transaction, &package)?;
        insert_switch_timeline(
            &transaction,
            &prepared.product_session_id,
            "engine_fallback",
            &prepared.branch_id,
            Some(&target_branch_id),
            &prepared.target.engine_id,
            &branch_target.engine_id,
            &format!("{failure_reason} · handoff: {}", package.fallback_summary()),
            now,
        )?;
        transaction
            .execute(
                "INSERT INTO engine_attempts
                       (attempt_id, turn_id, product_session_id, branch_id, engine_id,
                        status, failure_reason, usage_json, side_effects_json,
                        started_at, completed_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, NULL, '[]', ?7, NULL)",
                params![
                    next_attempt_id,
                    prepared.turn_id,
                    prepared.product_session_id,
                    target_branch_id,
                    branch_target.engine_id,
                    STATUS_RUNNING,
                    now
                ],
            )
            .map_err(|error| format!("create Hub fallback engine attempt: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub fallback transaction: {error}"))?;

        Ok(PreparedTurn {
            product_session_id: prepared.product_session_id.clone(),
            branch_id: target_branch_id,
            turn_id: prepared.turn_id.clone(),
            attempt_id: next_attempt_id,
            workspace: prepared.workspace.clone(),
            engine_session_id: None,
            target: branch_target,
            handoff: Some(package),
            native_resume: false,
            permission_profile: prepared.permission_profile,
        })
    }

    pub fn set_transcript_sharing(
        &self,
        product_session_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE hub_sessions SET include_full_transcript = ?2, updated_at = ?3
                      WHERE product_session_id = ?1",
                params![product_session_id, enabled, now_unix_ms()],
            )
            .map_err(|error| format!("persist Hub transcript sharing: {error}"))?;
        if changed != 1 {
            return Err(format!("Hub session '{product_session_id}' was not found"));
        }
        Ok(())
    }

    pub fn record_attempt_cancelled(
        &self,
        product_session_id: &str,
        reason: &str,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub attempt cancel transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        if session.status != STATUS_RUNNING {
            return Err("This Hub session has no running attempt to stop.".to_string());
        }
        insert_switch_timeline(
            &transaction,
            product_session_id,
            "attempt_cancelled",
            &session.active_branch_id,
            None,
            &session.engine_id,
            &session.engine_id,
            reason,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub attempt cancel: {error}"))?;
        Ok(())
    }

    /// The session's currently RUNNING attempt id, if any (unique index
    /// guarantees at most one).
    pub fn running_attempt_id(&self, product_session_id: &str) -> Result<Option<String>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT attempt_id FROM engine_attempts
                  WHERE product_session_id = ?1 AND status = ?2",
                params![product_session_id, STATUS_RUNNING],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("query running attempt: {error}"))
    }

    /// The session's most recently STARTED attempt id (running or terminal).
    pub fn latest_attempt_id(&self, product_session_id: &str) -> Result<Option<String>, String> {
        let connection = self.lock()?;
        connection
            .query_row(
                "SELECT attempt_id FROM engine_attempts
                  WHERE product_session_id = ?1
                  ORDER BY started_at DESC, rowid DESC LIMIT 1",
                params![product_session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("query latest attempt: {error}"))
    }

    /// R77/R91: attempt-bound cancel fact. The caller pins the attempt it
    /// observed when the stop started; the fact is written only if that SAME
    /// attempt is still the session's running one inside this transaction
    /// (same-branch resumes mint new attempts, so the attempt — not the
    /// branch — is the stop identity). A superseded stop (a newer attempt
    /// already claimed the session, or the observed attempt already reached
    /// a terminal state) returns `Ok(false)` and writes nothing — a late
    /// cancel acknowledgement can never stamp a cancellation onto the next
    /// attempt.
    pub fn record_attempt_cancelled_bound(
        &self,
        product_session_id: &str,
        attempt_id: &str,
        reason: &str,
    ) -> Result<bool, String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub attempt cancel transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?;
        let Some(session) = session else {
            return Err(format!("Hub session '{product_session_id}' was not found"));
        };
        let running: Option<String> = transaction
            .query_row(
                "SELECT attempt_id FROM engine_attempts
                  WHERE product_session_id = ?1 AND status = ?2",
                params![product_session_id, STATUS_RUNNING],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("query running attempt: {error}"))?;
        if session.status != STATUS_RUNNING || running.as_deref() != Some(attempt_id) {
            return Ok(false);
        }
        insert_switch_timeline(
            &transaction,
            product_session_id,
            "attempt_cancelled",
            &session.active_branch_id,
            None,
            &session.engine_id,
            &session.engine_id,
            reason,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub attempt cancel: {error}"))?;
        Ok(true)
    }

    /// R87: whether the user stopped THIS attempt — an `attempt_cancelled`
    /// fact recorded at or after the attempt started. The fallback loop
    /// consults this before continuing a continuity chain: a user Stop
    /// (including an adapter's cancel-grace kill) outranks the failure
    /// classification.
    pub fn attempt_has_cancel_fact(
        &self,
        product_session_id: &str,
        attempt_id: &str,
    ) -> Result<bool, String> {
        let connection = self.lock()?;
        let started_at: Option<i64> = connection
            .query_row(
                "SELECT started_at FROM engine_attempts WHERE attempt_id = ?1",
                [attempt_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query attempt '{attempt_id}': {error}"))?;
        let Some(started_at) = started_at else {
            return Ok(false);
        };
        connection
            .query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM timeline_events
                    WHERE product_session_id = ?1
                      AND kind = 'attempt_cancelled'
                      AND created_at >= ?2
                 )",
                params![product_session_id, started_at],
                |row| row.get::<_, bool>(0),
            )
            .map_err(|error| format!("query cancel fact: {error}"))
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

    #[cfg(test)]
    pub(crate) fn record_side_effect(
        &self,
        prepared: &PreparedTurn,
        evidence: SideEffectEvidence,
    ) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin side-effect evidence transaction: {error}"))?;
        record_side_effect_in(&transaction, prepared, evidence)?;
        transaction
            .commit()
            .map_err(|error| format!("commit side-effect evidence: {error}"))
    }
}

pub(super) fn record_side_effect_in(
    transaction: &Transaction<'_>,
    prepared: &PreparedTurn,
    evidence: SideEffectEvidence,
) -> Result<SideEffectEvidence, String> {
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
    let merged = if let Some(existing) = evidence_list
        .iter_mut()
        .find(|item| item.tool_call_id == evidence.tool_call_id)
    {
        // ToolEnd updates carry no path/input; keep what ToolStart saw. Merge input/output.
        let path = evidence.path.clone().or_else(|| existing.path.clone());
        let tool_input = evidence
            .tool_input
            .clone()
            .or_else(|| existing.tool_input.clone());
        let tool_output = evidence
            .tool_output
            .clone()
            .or_else(|| existing.tool_output.clone());
        let first_seen_at = existing.first_seen_at.or(evidence.first_seen_at);
        let sequence = existing.sequence.or(evidence.sequence);
        let tool_input_ref = evidence
            .tool_input_ref
            .clone()
            .or_else(|| existing.tool_input_ref.clone());
        let tool_output_ref = evidence
            .tool_output_ref
            .clone()
            .or_else(|| existing.tool_output_ref.clone());
        *existing = evidence;
        existing.first_seen_at = first_seen_at;
        existing.sequence = sequence;
        existing.path = path;
        existing.tool_input = tool_input;
        existing.tool_output = tool_output;
        existing.tool_input_ref = tool_input_ref;
        existing.tool_output_ref = tool_output_ref;
        existing.clone()
    } else {
        evidence_list.push(evidence.clone());
        evidence
    };
    let encoded = serde_json::to_string(&evidence_list)
        .map_err(|error| format!("serialize side-effect evidence: {error}"))?;
    transaction
        .execute(
            "UPDATE engine_attempts SET side_effects_json = ?2
                      WHERE attempt_id = ?1 AND status = ?3",
            params![prepared.attempt_id, encoded, STATUS_RUNNING],
        )
        .map_err(|error| format!("persist side-effect evidence: {error}"))?;
    Ok(merged)
}

#[allow(clippy::too_many_arguments)]
fn insert_turn_and_attempt(
    transaction: &Transaction<'_>,
    turn_id: &str,
    attempt_id: &str,
    product_session_id: &str,
    branch_id: &str,
    engine_id: &str,
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
                engine_id,
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

fn query_branches(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Vec<StoredBranch>, String> {
    let mut statement = connection
        .prepare(
            "SELECT branch_id, engine_id, model_id, provider_id, account_source,
                    billing_route, automatic_use_authorized, required_capabilities_json,
                    reasoning_effort, fast_mode, engine_session_id, watermark_turn_rowid,
                    status, created_at, updated_at
               FROM engine_branches WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub engine branches: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            let required_capabilities_json: String = row.get(7)?;
            let required_capabilities =
                serde_json::from_str(&required_capabilities_json).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        7,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })?;
            Ok(StoredBranch {
                branch_id: row.get(0)?,
                engine_id: row.get(1)?,
                model_id: row.get(2)?,
                provider_id: row.get(3)?,
                account_source: row.get(4)?,
                billing_route: row.get(5)?,
                automatic_use_authorized: row.get(6)?,
                required_capabilities,
                reasoning_effort: row.get(8)?,
                fast_mode: row.get(9)?,
                engine_session_id: row.get(10)?,
                watermark_turn_rowid: row.get(11)?,
                status: row.get(12)?,
                created_at: row.get(13)?,
                updated_at: row.get(14)?,
            })
        })
        .map_err(|error| format!("query Hub engine branches: {error}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub engine branches: {error}"))
}

fn query_timeline_events(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Vec<StoredTimelineEvent>, String> {
    let mut statement = connection
        .prepare(
            "SELECT event_id, kind, source_branch_id, target_branch_id,
                    source_engine_id, target_engine_id, reason, created_at
               FROM timeline_events WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub timeline events: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok(StoredTimelineEvent {
                event_id: row.get(0)?,
                kind: row.get(1)?,
                source_branch_id: row.get(2)?,
                target_branch_id: row.get(3)?,
                source_engine_id: row.get(4)?,
                target_engine_id: row.get(5)?,
                reason: row.get(6)?,
                created_at: row.get(7)?,
            })
        })
        .map_err(|error| format!("query Hub timeline events: {error}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub timeline events: {error}"))
}
