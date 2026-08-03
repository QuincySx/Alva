// INPUT:  rusqlite, serde, filesystem paths, UUID/time primitives
// OUTPUT: HubStore plus durable branch/policy/handoff/timeline/turn/attempt DTOs
// POS:    Hub Desktop-private multi-engine state machine, authorization snapshots, and crash repair owner.

use std::fs::{self, File, OpenOptions};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{params, Connection, OptionalExtension, Transaction, TransactionBehavior};
use serde::{Deserialize, Serialize};

use crate::claims::ResourceClaim;
use crate::permissions::PermissionProfile;

const SCHEMA_VERSION: i64 = 10;
const STATUS_RUNNING: &str = "running";
const STATUS_COMPLETED: &str = "completed";
const STATUS_FAILED: &str = "failed";
const STATUS_INTERRUPTED: &str = "interrupted";
const RESTART_REASON: &str =
    "ALVA Hub Desktop restarted before the engine attempt reached a terminal state.";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EngineTarget {
    pub engine_id: String,
    pub model_id: String,
    pub provider_id: String,
    #[serde(default = "default_account_source")]
    pub account_source: String,
    #[serde(default = "default_billing_route")]
    pub billing_route: String,
    #[serde(default)]
    pub automatic_use_authorized: bool,
    #[serde(default)]
    pub required_capabilities: Vec<String>,
}

fn default_account_source() -> String {
    "Unconfigured account".to_string()
}

fn default_billing_route() -> String {
    "Unconfigured billing route".to_string()
}

impl EngineTarget {
    pub fn for_engine(engine_id: &str) -> Result<Self, String> {
        let (model_id, provider_id, account_source, billing_route) = match engine_id {
            "alva" => (
                "workspace-config",
                "workspace-config",
                "Workspace provider configuration",
                "Workspace provider account",
            ),
            "claude" => (
                "engine-default",
                "claude-cli",
                "Claude Code CLI account",
                "Anthropic account via Claude CLI",
            ),
            "codex" => (
                "engine-default",
                "codex-cli",
                "Codex CLI account",
                "OpenAI account via Codex CLI",
            ),
            other => {
                return Err(format!(
                    "unsupported Hub engine '{other}'; expected alva, claude, or codex"
                ));
            }
        };
        Ok(Self {
            engine_id: engine_id.to_string(),
            model_id: model_id.to_string(),
            provider_id: provider_id.to_string(),
            account_source: account_source.to_string(),
            billing_route: billing_route.to_string(),
            automatic_use_authorized: false,
            required_capabilities: Vec::new(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub targets: Vec<EngineTarget>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct HubSessionSummary {
    pub product_session_id: String,
    pub workspace: String,
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
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoredBranch {
    pub branch_id: String,
    pub engine_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub engine_session_id: Option<String>,
    pub watermark_turn_rowid: Option<i64>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum HandoffKind {
    #[default]
    Full,
    Delta,
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
    /// File the tool touched, when the adapter surfaced one in its input.
    #[serde(default)]
    pub path: Option<String>,
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffCompletedWork {
    pub prompt: String,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffPendingWork {
    pub prompt: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct HandoffPackage {
    pub handoff_id: String,
    pub product_session_id: String,
    pub source_branch_id: String,
    pub target_branch_id: String,
    pub source: EngineTarget,
    pub target: EngineTarget,
    #[serde(default)]
    pub kind: HandoffKind,
    pub reason: String,
    pub user_goal: Option<String>,
    pub latest_instruction: Option<String>,
    pub accepted_decisions: Vec<String>,
    pub completed_work: Vec<HandoffCompletedWork>,
    pub pending_work: Vec<HandoffPendingWork>,
    pub changed_files: Vec<String>,
    pub recent_commands: Vec<String>,
    pub verification_results: Vec<String>,
    pub workspace_summary: String,
    pub known_side_effects: Vec<SideEffectEvidence>,
    pub team_snapshot: Option<String>,
    /// Full filtered turn transcript — populated only when the user has
    /// explicitly enabled transcript sharing for the session.
    #[serde(default)]
    pub transcript: Vec<TranscriptTurn>,
    pub created_at: i64,
    pub used_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TranscriptTurn {
    pub prompt: String,
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TeamRunDetail {
    pub team_run_id: String,
    /// Exactly one lead per run — the HubSession the tech lead works in.
    pub lead_session_id: String,
    pub status: String,
    pub workers: Vec<TeamWorker>,
    pub tasks: Vec<WorkerTask>,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamWorker {
    pub worker_id: String,
    /// A Hub-owned durable session — full branch/turn/attempt machinery, not
    /// an engine-private subagent.
    pub worker_session_id: String,
    pub engine_id: String,
    pub created_at: i64,
}

/// Outcome of trying to take a worker's next queued task.
#[derive(Debug, Clone, PartialEq)]
pub enum TaskClaim {
    Claimed(Box<WorkerTask>),
    /// The head of the queue cannot start yet; these resources are held.
    Blocked {
        task_id: String,
        blocked_by: Vec<BlockedResource>,
    },
    Empty,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct BlockedResource {
    pub resource: String,
    pub holder_task_id: String,
    pub holder_worker_id: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct WorkerTask {
    pub task_id: String,
    pub team_run_id: String,
    pub worker_id: String,
    pub scope: String,
    pub status: String,
    pub result: Option<String>,
    pub changed_files: Vec<String>,
    pub verification: Vec<SideEffectEvidence>,
    /// Resources this task declared before it may start.
    pub claims: Vec<ResourceClaim>,
    pub created_at: i64,
    pub completed_at: Option<i64>,
}

impl HandoffPackage {
    /// One-line summary shared by the live fallback card and the persisted
    /// `engine_fallback` timeline fact.
    pub(crate) fn fallback_summary(&self) -> String {
        format!(
            "full context · {} completed · {} pending",
            self.completed_work.len(),
            self.pending_work.len()
        )
    }

    pub(crate) fn render_for_engine(&self) -> String {
        let encoded = serde_json::to_string_pretty(self)
            .unwrap_or_else(|_| "{\"error\":\"handoff serialization failed\"}".to_string());
        let kind = match self.kind {
            HandoffKind::Full => "full",
            HandoffKind::Delta => "delta",
        };
        format!("<alva-hub-handoff kind=\"{kind}\">\n{encoded}\n</alva-hub-handoff>")
    }
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PendingSwitch {
    pub switch_id: String,
    pub source_branch_id: String,
    pub target: EngineTarget,
    pub reason: String,
    pub requested_at: i64,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SwitchDisposition {
    Applied,
    Queued,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SwitchOutcome {
    pub disposition: SwitchDisposition,
    pub active_branch_id: String,
    pub active_engine_id: String,
    pub target_engine_id: String,
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
                        hs.permission_profile, hs.include_full_transcript
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
        Ok(Some(HubSessionDetail {
            session,
            turns,
            branches,
            handoffs,
            timeline_events,
            pending_switch,
            fallback_policy,
        }))
    }

    pub fn global_fallback_policy(&self) -> Result<FallbackPolicy, String> {
        let connection = self.lock()?;
        query_fallback_policy(&connection, None)
    }

    pub fn set_global_fallback_policy(
        &self,
        targets: Vec<EngineTarget>,
    ) -> Result<FallbackPolicy, String> {
        validate_fallback_targets(&targets)?;
        let policy = FallbackPolicy {
            targets,
            updated_at: now_unix_ms(),
        };
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin global fallback policy transaction: {error}"))?;
        persist_fallback_policy(&transaction, None, &policy)?;
        transaction
            .commit()
            .map_err(|error| format!("commit global fallback policy: {error}"))?;
        Ok(policy)
    }

    pub fn set_session_fallback_policy(
        &self,
        product_session_id: &str,
        targets: Vec<EngineTarget>,
    ) -> Result<FallbackPolicy, String> {
        validate_fallback_targets(&targets)?;
        let policy = FallbackPolicy {
            targets,
            updated_at: now_unix_ms(),
        };
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin session fallback policy transaction: {error}"))?;
        if query_session(&transaction, product_session_id)?.is_none() {
            return Err(format!("Hub session '{product_session_id}' was not found"));
        }
        persist_fallback_policy(&transaction, Some(product_session_id), &policy)?;
        transaction
            .commit()
            .map_err(|error| format!("commit session fallback policy: {error}"))?;
        Ok(policy)
    }

    pub fn is_authorized_for_automatic_fallback(
        &self,
        product_session_id: &str,
        target: &EngineTarget,
    ) -> Result<bool, String> {
        validate_engine_target(target)?;
        let connection = self.lock()?;
        if query_session(&connection, product_session_id)?.is_none() {
            return Err(format!("Hub session '{product_session_id}' was not found"));
        }
        let policy = query_fallback_policy(&connection, Some(product_session_id))?;
        Ok(policy
            .targets
            .iter()
            .any(|candidate| candidate == target && candidate.automatic_use_authorized))
    }

    /// Most recent terminal attempt outcome (status, failure_reason) for an
    /// engine across all sessions — read-only stream-health evidence for the
    /// readiness dashboard.
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

    pub fn request_engine_switch(
        &self,
        product_session_id: &str,
        target: &EngineTarget,
        reason: &str,
    ) -> Result<SwitchOutcome, String> {
        validate_engine_target(target)?;
        let reason = reason.trim();
        if reason.is_empty() {
            return Err("manual engine switch requires a reason".to_string());
        }
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub engine switch transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        if session.engine_id == target.engine_id
            && session.model_id == target.model_id
            && session.provider_id == target.provider_id
        {
            if let Some(pending) = query_pending_switch(&transaction, product_session_id)? {
                transaction
                    .execute(
                        "UPDATE engine_switches
                            SET status = 'cancelled', applied_at = ?2
                          WHERE switch_id = ?1 AND status = 'queued'",
                        params![pending.switch_id, now],
                    )
                    .map_err(|error| format!("cancel queued Hub engine switch: {error}"))?;
                insert_switch_timeline(
                    &transaction,
                    product_session_id,
                    "engine_switch_cancelled",
                    &session.active_branch_id,
                    None,
                    &session.engine_id,
                    &pending.target.engine_id,
                    reason,
                    now,
                )?;
                transaction
                    .commit()
                    .map_err(|error| format!("commit cancelled Hub engine switch: {error}"))?;
            }
            return Ok(SwitchOutcome {
                disposition: SwitchDisposition::Applied,
                active_branch_id: session.active_branch_id,
                active_engine_id: session.engine_id,
                target_engine_id: target.engine_id.clone(),
            });
        }

        // Manual switches obey the same permission contract as automatic
        // fallback: a target that can only weaken the session profile would
        // brick the session (every later run fails engine assembly), so it
        // is rejected here with the mapping's reason.
        crate::permissions::enforcement_for(&target.engine_id, session.permission_profile)
            .map_err(|reason| {
                format!(
                    "cannot switch Hub session '{product_session_id}' to '{}': {reason}",
                    target.engine_id
                )
            })?;

        let switch_id = upsert_queued_switch(
            &transaction,
            product_session_id,
            &session.active_branch_id,
            target,
            reason,
            now,
        )?;
        if session.status == STATUS_RUNNING {
            insert_switch_timeline(
                &transaction,
                product_session_id,
                "engine_switch_queued",
                &session.active_branch_id,
                None,
                &session.engine_id,
                &target.engine_id,
                reason,
                now,
            )?;
            transaction
                .commit()
                .map_err(|error| format!("commit queued Hub engine switch: {error}"))?;
            return Ok(SwitchOutcome {
                disposition: SwitchDisposition::Queued,
                active_branch_id: session.active_branch_id,
                active_engine_id: session.engine_id,
                target_engine_id: target.engine_id.clone(),
            });
        }

        let outcome = apply_switch_in_transaction(
            &transaction,
            product_session_id,
            &switch_id,
            &session,
            target,
            reason,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub engine switch: {error}"))?;
        Ok(outcome)
    }

    pub(crate) fn apply_pending_switch(
        &self,
        product_session_id: &str,
    ) -> Result<Option<SwitchOutcome>, String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin pending Hub engine switch transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        if session.status == STATUS_RUNNING {
            return Ok(None);
        }
        let Some(pending) = query_pending_switch(&transaction, product_session_id)? else {
            return Ok(None);
        };
        let outcome = apply_switch_in_transaction(
            &transaction,
            product_session_id,
            &pending.switch_id,
            &session,
            &pending.target,
            &pending.reason,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit pending Hub engine switch: {error}"))?;
        Ok(Some(outcome))
    }

    fn apply_ready_pending_switches(&self) -> Result<(), String> {
        let product_session_ids = {
            let connection = self.lock()?;
            let mut statement = connection
                .prepare(
                    "SELECT es.product_session_id
                       FROM engine_switches es
                       JOIN hub_sessions hs
                         ON hs.product_session_id = es.product_session_id
                      WHERE es.status = 'queued' AND hs.status <> ?1
                      ORDER BY es.requested_at ASC, es.rowid ASC",
                )
                .map_err(|error| format!("prepare recoverable Hub engine switches: {error}"))?;
            let rows = statement
                .query_map([STATUS_RUNNING], |row| row.get::<_, String>(0))
                .map_err(|error| format!("query recoverable Hub engine switches: {error}"))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| format!("decode recoverable Hub engine switches: {error}"))?
        };
        for product_session_id in product_session_ids {
            self.apply_pending_switch(&product_session_id)?;
        }
        Ok(())
    }

    pub(crate) fn prepare_new_turn(
        &self,
        product_session_id: &str,
        workspace: &Path,
        prompt: &str,
        target: &EngineTarget,
        permission_profile: PermissionProfile,
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
                    permission_profile, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    product_session_id,
                    workspace_text,
                    branch_id,
                    STATUS_RUNNING,
                    permission_profile.as_str(),
                    now
                ],
            )
            .map_err(|error| format!("insert Hub session: {error}"))?;
        transaction
            .execute(
                "INSERT INTO engine_branches
                   (branch_id, product_session_id, engine_id, model_id, provider_id,
                    engine_session_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'active', ?6, ?6)",
                params![
                    branch_id,
                    product_session_id,
                    target.engine_id,
                    target.model_id,
                    target.provider_id,
                    now
                ],
            )
            .map_err(|error| format!("insert Hub engine branch: {error}"))?;
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
        let mut target = EngineTarget::for_engine(&session.engine_id)?;
        target.model_id = session.model_id.clone();
        target.provider_id = session.provider_id.clone();
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
        let target_branch_id = match existing_target {
            Some(branch_id) => branch_id,
            None => {
                let branch_id = uuid::Uuid::new_v4().to_string();
                transaction
                    .execute(
                        "INSERT INTO engine_branches
                           (branch_id, product_session_id, engine_id, model_id, provider_id,
                            engine_session_id, status, created_at, updated_at)
                         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'parked', ?6, ?6)",
                        params![
                            branch_id,
                            prepared.product_session_id,
                            target.engine_id,
                            target.model_id,
                            target.provider_id,
                            now
                        ],
                    )
                    .map_err(|error| format!("create Hub fallback target branch: {error}"))?;
                branch_id
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
            target.clone(),
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
            &target.engine_id,
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
                    target.engine_id,
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
            target: target.clone(),
            handoff: Some(package),
            native_resume: false,
            permission_profile: prepared.permission_profile,
        })
    }

    /// Create a TeamRun on a lead session together with one Hub-owned durable
    /// worker session on its own EngineTarget. The worker inherits the lead's
    /// workspace and permission profile and snapshots the global fallback
    /// policy like any other session.
    pub fn create_team_run(
        &self,
        lead_session_id: &str,
        worker_target: &EngineTarget,
    ) -> Result<TeamRunDetail, String> {
        validate_engine_target(worker_target)?;
        let now = now_unix_ms();
        let team_run_id = uuid::Uuid::new_v4().to_string();
        let worker_id = uuid::Uuid::new_v4().to_string();
        let worker_session_id = uuid::Uuid::new_v4().to_string();
        let worker_branch_id = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub team run transaction: {error}"))?;
        let lead = query_session(&transaction, lead_session_id)?
            .ok_or_else(|| format!("Hub session '{lead_session_id}' was not found"))?;
        transaction
            .execute(
                "INSERT INTO team_runs (team_run_id, lead_session_id, status, created_at)
                 VALUES (?1, ?2, 'active', ?3)",
                params![team_run_id, lead_session_id, now],
            )
            .map_err(|error| format!("insert Hub team run: {error}"))?;
        transaction
            .execute(
                "INSERT INTO hub_sessions
                   (product_session_id, workspace, active_branch_id, status,
                    permission_profile, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    worker_session_id,
                    lead.workspace,
                    worker_branch_id,
                    STATUS_COMPLETED,
                    lead.permission_profile.as_str(),
                    now
                ],
            )
            .map_err(|error| format!("insert Hub worker session: {error}"))?;
        transaction
            .execute(
                "INSERT INTO engine_branches
                   (branch_id, product_session_id, engine_id, model_id, provider_id,
                    engine_session_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'active', ?6, ?6)",
                params![
                    worker_branch_id,
                    worker_session_id,
                    worker_target.engine_id,
                    worker_target.model_id,
                    worker_target.provider_id,
                    now
                ],
            )
            .map_err(|error| format!("insert Hub worker branch: {error}"))?;
        let global_policy = query_fallback_policy(&transaction, None)?;
        persist_fallback_policy(
            &transaction,
            Some(&worker_session_id),
            &FallbackPolicy {
                targets: global_policy.targets,
                updated_at: now,
            },
        )?;
        transaction
            .execute(
                "INSERT INTO team_workers (worker_id, team_run_id, worker_session_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![worker_id, team_run_id, worker_session_id, now],
            )
            .map_err(|error| format!("insert Hub team worker: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub team run: {error}"))?;
        drop(connection);
        self.get_team_run(&team_run_id)?
            .ok_or_else(|| "team run vanished after creation".to_string())
    }

    pub fn get_team_run(&self, team_run_id: &str) -> Result<Option<TeamRunDetail>, String> {
        let connection = self.lock()?;
        query_team_run(&connection, team_run_id)
    }

    /// Latest team run whose lead is this session, if any.
    pub fn team_run_for_lead(
        &self,
        lead_session_id: &str,
    ) -> Result<Option<TeamRunDetail>, String> {
        let connection = self.lock()?;
        let team_run_id = connection
            .query_row(
                "SELECT team_run_id FROM team_runs
                  WHERE lead_session_id = ?1
                  ORDER BY created_at DESC, rowid DESC LIMIT 1",
                [lead_session_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("query Hub team run for lead: {error}"))?;
        match team_run_id {
            Some(team_run_id) => query_team_run(&connection, &team_run_id),
            None => Ok(None),
        }
    }

    /// Add another Hub-owned durable worker session (its own EngineTarget)
    /// to an existing team run — the batch's parallel intent.
    pub fn add_team_worker(
        &self,
        team_run_id: &str,
        worker_target: &EngineTarget,
    ) -> Result<TeamRunDetail, String> {
        validate_engine_target(worker_target)?;
        let now = now_unix_ms();
        let worker_id = uuid::Uuid::new_v4().to_string();
        let worker_session_id = uuid::Uuid::new_v4().to_string();
        let worker_branch_id = uuid::Uuid::new_v4().to_string();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub add-worker transaction: {error}"))?;
        let lead_session_id = transaction
            .query_row(
                "SELECT lead_session_id FROM team_runs WHERE team_run_id = ?1",
                [team_run_id],
                |row| row.get::<_, String>(0),
            )
            .optional()
            .map_err(|error| format!("query Hub team run: {error}"))?
            .ok_or_else(|| format!("team run '{team_run_id}' was not found"))?;
        let lead = query_session(&transaction, &lead_session_id)?
            .ok_or_else(|| format!("Hub session '{lead_session_id}' was not found"))?;
        transaction
            .execute(
                "INSERT INTO hub_sessions
                   (product_session_id, workspace, active_branch_id, status,
                    permission_profile, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    worker_session_id,
                    lead.workspace,
                    worker_branch_id,
                    STATUS_COMPLETED,
                    lead.permission_profile.as_str(),
                    now
                ],
            )
            .map_err(|error| format!("insert Hub worker session: {error}"))?;
        transaction
            .execute(
                "INSERT INTO engine_branches
                   (branch_id, product_session_id, engine_id, model_id, provider_id,
                    engine_session_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'active', ?6, ?6)",
                params![
                    worker_branch_id,
                    worker_session_id,
                    worker_target.engine_id,
                    worker_target.model_id,
                    worker_target.provider_id,
                    now
                ],
            )
            .map_err(|error| format!("insert Hub worker branch: {error}"))?;
        let global_policy = query_fallback_policy(&transaction, None)?;
        persist_fallback_policy(
            &transaction,
            Some(&worker_session_id),
            &FallbackPolicy {
                targets: global_policy.targets,
                updated_at: now,
            },
        )?;
        transaction
            .execute(
                "INSERT INTO team_workers (worker_id, team_run_id, worker_session_id, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![worker_id, team_run_id, worker_session_id, now],
            )
            .map_err(|error| format!("insert Hub team worker: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub add-worker: {error}"))?;
        drop(connection);
        self.get_team_run(team_run_id)?
            .ok_or_else(|| "team run vanished after add-worker".to_string())
    }

    /// Append a task to one worker's durable FIFO queue. Nothing executes
    /// until the queue pump claims it, so it stays viewable, editable, and
    /// cancellable.
    pub fn enqueue_worker_task(
        &self,
        team_run_id: &str,
        worker_id: &str,
        scope: &str,
        claims: &[ResourceClaim],
    ) -> Result<WorkerTask, String> {
        let scope = scope.trim();
        if scope.is_empty() {
            return Err("a worker task needs a non-empty scope".to_string());
        }
        for claim in claims {
            claim.validate()?;
        }
        let claims_json = serde_json::to_string(claims)
            .map_err(|error| format!("serialize Hub task claims: {error}"))?;
        let now = now_unix_ms();
        let task_id = uuid::Uuid::new_v4().to_string();
        let connection = self.lock()?;
        let belongs: Option<String> = connection
            .query_row(
                "SELECT worker_id FROM team_workers
                  WHERE team_run_id = ?1 AND worker_id = ?2",
                params![team_run_id, worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub team worker: {error}"))?;
        if belongs.is_none() {
            return Err(format!(
                "worker '{worker_id}' does not belong to team run '{team_run_id}'"
            ));
        }
        connection
            .execute(
                "INSERT INTO worker_tasks
                   (task_id, team_run_id, worker_id, scope, status, claims_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'queued', ?5, ?6)",
                params![task_id, team_run_id, worker_id, scope, claims_json, now],
            )
            .map_err(|error| format!("insert Hub worker task: {error}"))?;
        drop(connection);
        self.worker_task(&task_id)?
            .ok_or_else(|| "worker task vanished after creation".to_string())
    }

    /// Atomically claim the oldest queued task of one worker for execution,
    /// but only if its ResourceClaims are compatible with everything already
    /// running in the same workspace. Cancelled tasks are never claimed;
    /// each worker's order is FIFO, and a blocked head-of-queue waits rather
    /// than letting a later task jump it — that is what makes the ordering
    /// deterministic.
    pub fn claim_next_worker_task(&self, worker_id: &str) -> Result<TaskClaim, String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub task claim transaction: {error}"))?;
        let head: Option<(String, String, i64, i64)> = transaction
            .query_row(
                "SELECT task_id, claims_json, created_at, rowid FROM worker_tasks
                  WHERE worker_id = ?1 AND status = 'queued'
                  ORDER BY created_at ASC, rowid ASC LIMIT 1",
                [worker_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
            )
            .optional()
            .map_err(|error| format!("query Hub worker queue: {error}"))?;
        let Some((task_id, claims_json, head_created_at, head_rowid)) = head else {
            return Ok(TaskClaim::Empty);
        };
        let wanted: Vec<ResourceClaim> = serde_json::from_str(&claims_json)
            .map_err(|error| format!("decode Hub task claims: {error}"))?;

        // Everything currently running in this workspace holds its claims.
        let workspace: String = transaction
            .query_row(
                "SELECT hs.workspace FROM team_workers tw
                   JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                  WHERE tw.worker_id = ?1",
                [worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub worker workspace: {error}"))?
            .ok_or_else(|| format!("worker '{worker_id}' has no session"))?;
        let mut holder_statement = transaction
            .prepare(
                "SELECT wt.task_id, wt.worker_id, wt.claims_json FROM worker_tasks wt
                   JOIN team_workers tw ON tw.worker_id = wt.worker_id
                   JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                  WHERE wt.status = ?1 AND hs.workspace = ?2 AND wt.task_id <> ?3",
            )
            .map_err(|error| format!("prepare Hub claim holders: {error}"))?;
        let holders = holder_statement
            .query_map(params![STATUS_RUNNING, workspace, task_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(|error| format!("query Hub claim holders: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub claim holders: {error}"))?;
        drop(holder_statement);
        let mut blocked_by: Vec<BlockedResource> = Vec::new();
        for (holder_task_id, holder_worker_id, holder_claims_json) in holders {
            let held: Vec<ResourceClaim> = serde_json::from_str(&holder_claims_json)
                .map_err(|error| format!("decode Hub holder claims: {error}"))?;
            for conflict in crate::claims::conflicting_claims(&wanted, &held) {
                blocked_by.push(BlockedResource {
                    resource: conflict.describe(),
                    holder_task_id: holder_task_id.clone(),
                    holder_worker_id: holder_worker_id.clone(),
                });
            }
        }
        // Fairness: an older head-of-queue waiter in this workspace reserves
        // its resources, so a stream of short conflicting tasks cannot starve
        // it. Holders never wait, so the globally oldest waiter is always
        // eventually grantable — reservations cannot deadlock.
        if blocked_by.is_empty() {
            let mut waiter_statement = transaction
                .prepare(
                    "SELECT wt.task_id, wt.worker_id, wt.claims_json FROM worker_tasks wt
                       JOIN team_workers tw ON tw.worker_id = wt.worker_id
                       JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                      WHERE wt.status = 'queued' AND hs.workspace = ?1
                        AND wt.worker_id <> ?2
                        AND (wt.created_at < ?3
                             OR (wt.created_at = ?3 AND wt.rowid < ?4))
                        AND wt.task_id = (
                          SELECT head.task_id FROM worker_tasks head
                           WHERE head.worker_id = wt.worker_id AND head.status = 'queued'
                           ORDER BY head.created_at ASC, head.rowid ASC LIMIT 1
                        )",
                )
                .map_err(|error| format!("prepare Hub claim waiters: {error}"))?;
            let waiters = waiter_statement
                .query_map(
                    params![workspace, worker_id, head_created_at, head_rowid],
                    |row| {
                        Ok((
                            row.get::<_, String>(0)?,
                            row.get::<_, String>(1)?,
                            row.get::<_, String>(2)?,
                        ))
                    },
                )
                .map_err(|error| format!("query Hub claim waiters: {error}"))?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(|error| format!("decode Hub claim waiters: {error}"))?;
            drop(waiter_statement);
            for (waiter_task_id, waiter_worker_id, waiter_claims_json) in waiters {
                let reserved: Vec<ResourceClaim> = serde_json::from_str(&waiter_claims_json)
                    .map_err(|error| format!("decode Hub waiter claims: {error}"))?;
                for conflict in crate::claims::conflicting_claims(&wanted, &reserved) {
                    blocked_by.push(BlockedResource {
                        resource: conflict.describe(),
                        holder_task_id: waiter_task_id.clone(),
                        holder_worker_id: waiter_worker_id.clone(),
                    });
                }
            }
        }
        if !blocked_by.is_empty() {
            return Ok(TaskClaim::Blocked {
                task_id,
                blocked_by,
            });
        }

        let changed = transaction
            .execute(
                "UPDATE worker_tasks SET status = ?2, started_at = ?3
                  WHERE task_id = ?1 AND status = 'queued'",
                params![task_id, STATUS_RUNNING, now],
            )
            .map_err(|error| format!("claim Hub worker task: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub task claim: {error}"))?;
        drop(connection);
        if changed != 1 {
            return Ok(TaskClaim::Empty);
        }
        match self.worker_task(&task_id)? {
            Some(task) => Ok(TaskClaim::Claimed(Box::new(task))),
            None => Ok(TaskClaim::Empty),
        }
    }

    /// Test convenience: claim, flattening "blocked" and "empty" to None.
    #[cfg(test)]
    pub(crate) fn claim_for_test(&self, worker_id: &str) -> Result<Option<WorkerTask>, String> {
        Ok(match self.claim_next_worker_task(worker_id)? {
            TaskClaim::Claimed(task) => Some(*task),
            _ => None,
        })
    }

    /// Record that a task is waiting on resources another task holds, so the
    /// lead timeline shows the wait and its holder.
    pub fn record_resource_wait(
        &self,
        team_run_id: &str,
        worker_id: &str,
        blocked_by: &[BlockedResource],
    ) -> Result<(), String> {
        if blocked_by.is_empty() {
            return Ok(());
        }
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub resource wait transaction: {error}"))?;
        let lead_session_id = transaction
            .query_row(
                "SELECT lead_session_id FROM team_runs WHERE team_run_id = ?1",
                [team_run_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| format!("query Hub team lead: {error}"))?;
        let lead = query_session(&transaction, &lead_session_id)?
            .ok_or_else(|| format!("Hub session '{lead_session_id}' was not found"))?;
        let worker_engine: String = transaction
            .query_row(
                "SELECT eb.engine_id FROM team_workers tw
                   JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                   JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
                  WHERE tw.worker_id = ?1",
                [worker_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("query Hub worker engine: {error}"))?;
        let summary = blocked_by
            .iter()
            .map(|blocked| {
                format!(
                    "{} (held by worker {})",
                    blocked.resource, blocked.holder_worker_id
                )
            })
            .collect::<Vec<_>>()
            .join("; ");
        insert_switch_timeline(
            &transaction,
            &lead_session_id,
            "resource_wait",
            &lead.active_branch_id,
            None,
            &lead.engine_id,
            &worker_engine,
            &format!("Waiting on {summary}"),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub resource wait: {error}"))?;
        Ok(())
    }

    /// Workers with tasks still queued — the app restarts their pumps on
    /// launch so a durable queue survives a restart.
    pub fn workers_with_queued_tasks(&self) -> Result<Vec<String>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare("SELECT DISTINCT worker_id FROM worker_tasks WHERE status = 'queued'")
            .map_err(|error| format!("prepare Hub queued workers: {error}"))?;
        let workers = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(|error| format!("query Hub queued workers: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub queued workers: {error}"))?;
        Ok(workers)
    }

    /// Whether the worker has anything left in its queue.
    pub fn has_queued_task(&self, worker_id: &str) -> Result<bool, String> {
        let connection = self.lock()?;
        let queued: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM worker_tasks
                  WHERE worker_id = ?1 AND status = 'queued' LIMIT 1",
                [worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub worker queue depth: {error}"))?;
        Ok(queued.is_some())
    }

    /// Edit a still-queued task's scope; consumed tasks are immutable.
    pub fn update_worker_task_scope(
        &self,
        task_id: &str,
        scope: &str,
    ) -> Result<WorkerTask, String> {
        let scope = scope.trim();
        if scope.is_empty() {
            return Err("a worker task needs a non-empty scope".to_string());
        }
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE worker_tasks SET scope = ?2
                  WHERE task_id = ?1 AND status = 'queued'",
                params![task_id, scope],
            )
            .map_err(|error| format!("edit Hub worker task: {error}"))?;
        drop(connection);
        if changed != 1 {
            return Err("only queued tasks can be edited".to_string());
        }
        self.worker_task(task_id)?
            .ok_or_else(|| "worker task vanished after edit".to_string())
    }

    /// Cancel a still-queued task; running tasks are interrupted through the
    /// worker's live attempt instead.
    pub fn cancel_worker_task(&self, task_id: &str) -> Result<WorkerTask, String> {
        let now = now_unix_ms();
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE worker_tasks SET status = 'cancelled', completed_at = ?2
                  WHERE task_id = ?1 AND status = 'queued'",
                params![task_id, now],
            )
            .map_err(|error| format!("cancel Hub worker task: {error}"))?;
        drop(connection);
        if changed != 1 {
            return Err("only queued tasks can be cancelled".to_string());
        }
        self.worker_task(task_id)?
            .ok_or_else(|| "worker task vanished after cancel".to_string())
    }

    /// Close a WorkerTask with the worker's terminal outcome; evidence and
    /// changed files come from the worker session's latest attempt, and the
    /// structured outcome lands on the LEAD timeline as a `worker_outcome`
    /// fact — the execution channel back to the lead.
    pub fn finish_worker_task(
        &self,
        task_id: &str,
        success: bool,
        result: Option<&str>,
    ) -> Result<WorkerTask, String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub worker task transaction: {error}"))?;
        // Evidence window opens when this task was CLAIMED — enqueue time
        // would sweep in the attempts of tasks queued ahead of it.
        let (team_run_id, worker_id, task_started_at) = transaction
            .query_row(
                "SELECT team_run_id, worker_id, COALESCE(started_at, created_at)
                   FROM worker_tasks WHERE task_id = ?1",
                [task_id],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
            .map_err(|error| format!("query Hub worker task: {error}"))?
            .ok_or_else(|| format!("worker task '{task_id}' was not found"))?;
        let (worker_session_id,) = transaction
            .query_row(
                "SELECT worker_session_id FROM team_workers WHERE worker_id = ?1",
                [&worker_id],
                |row| Ok((row.get::<_, String>(0)?,)),
            )
            .map_err(|error| format!("query Hub team worker session: {error}"))?;
        // Every attempt this task's run produced (fallback can create
        // several), scoped so another task's attempts never leak in.
        let mut evidence_statement = transaction
            .prepare(
                "SELECT side_effects_json FROM engine_attempts
                  WHERE product_session_id = ?1 AND started_at >= ?2
                  ORDER BY started_at ASC, rowid ASC",
            )
            .map_err(|error| format!("prepare Hub worker evidence: {error}"))?;
        let evidence_rows = evidence_statement
            .query_map(params![worker_session_id, task_started_at], |row| {
                row.get::<_, String>(0)
            })
            .map_err(|error| format!("query Hub worker evidence: {error}"))?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode Hub worker evidence: {error}"))?;
        drop(evidence_statement);
        let mut verification: Vec<SideEffectEvidence> = Vec::new();
        for encoded in evidence_rows {
            let mut entries: Vec<SideEffectEvidence> = serde_json::from_str(&encoded)
                .map_err(|error| format!("decode Hub worker evidence: {error}"))?;
            verification.append(&mut entries);
        }
        // Only tools that finished successfully count as changed files.
        let mut changed_files: Vec<String> = verification
            .iter()
            .filter(|evidence| evidence.succeeded == Some(true))
            .filter_map(|evidence| evidence.path.clone())
            .collect();
        changed_files.sort();
        changed_files.dedup();
        let status = if success {
            STATUS_COMPLETED
        } else {
            STATUS_FAILED
        };
        let changed = transaction
            .execute(
                "UPDATE worker_tasks
                    SET status = ?2, result = ?3, changed_files_json = ?4,
                        verification_json = ?5, completed_at = ?6
                  WHERE task_id = ?1 AND status = ?7",
                params![
                    task_id,
                    status,
                    result,
                    serde_json::to_string(&changed_files)
                        .map_err(|error| format!("serialize changed files: {error}"))?,
                    serde_json::to_string(&verification)
                        .map_err(|error| format!("serialize verification: {error}"))?,
                    now,
                    STATUS_RUNNING
                ],
            )
            .map_err(|error| format!("complete Hub worker task: {error}"))?;
        if changed != 1 {
            return Err(format!("worker task '{task_id}' was not running"));
        }
        let lead = transaction
            .query_row(
                "SELECT lead_session_id FROM team_runs WHERE team_run_id = ?1",
                [&team_run_id],
                |row| row.get::<_, String>(0),
            )
            .map_err(|error| format!("query Hub team lead: {error}"))?;
        let lead_session = query_session(&transaction, &lead)?
            .ok_or_else(|| format!("Hub session '{lead}' was not found"))?;
        let worker_session = query_session(&transaction, &worker_session_id)?
            .ok_or_else(|| format!("Hub session '{worker_session_id}' was not found"))?;
        // Char-based excerpt (byte slicing panics on multi-byte boundaries)
        // and the same redaction the handoff channel applies — the outcome
        // lands on the lead's durable timeline.
        let summary = crate::redact::redact_text(&format!(
            "Worker ({}) {}{} · {} files · {} evidence entries",
            worker_session.engine_id,
            status,
            result
                .map(|result| {
                    let brief: String = result.chars().take(160).collect();
                    format!(": {brief}")
                })
                .unwrap_or_default(),
            changed_files.len(),
            verification.len(),
        ));
        insert_switch_timeline(
            &transaction,
            &lead,
            "worker_outcome",
            &lead_session.active_branch_id,
            None,
            &lead_session.engine_id,
            &worker_session.engine_id,
            &summary,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub worker task: {error}"))?;
        drop(connection);
        self.worker_task(task_id)?
            .ok_or_else(|| "worker task vanished after completion".to_string())
    }

    pub fn worker_task_by_id(&self, task_id: &str) -> Result<Option<WorkerTask>, String> {
        self.worker_task(task_id)
    }

    fn worker_task(&self, task_id: &str) -> Result<Option<WorkerTask>, String> {
        let connection = self.lock()?;
        query_worker_task(&connection, task_id)
    }

    /// Toggle whether this session's future handoffs may attach the full
    /// (still filtered) transcript. Off by default; never retroactive.
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

    /// Record a user Stop against the running attempt as a timeline fact.
    /// The attempt itself reaches its terminal state through the engine's
    /// cancelled stream, so nothing here mutates attempt rows — which also
    /// makes the action naturally idempotent across app restarts.
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

    /// A handoff counts as delivered once its target engine actually starts
    /// executing with it; until then it stays pending for recovery.
    pub(crate) fn mark_handoff_used(&self, handoff_id: &str) -> Result<(), String> {
        let connection = self.lock()?;
        connection
            .execute(
                "UPDATE handoff_packages SET used_at = COALESCE(used_at, ?2)
                  WHERE handoff_id = ?1",
                params![handoff_id, now_unix_ms()],
            )
            .map_err(|error| format!("mark Hub handoff used: {error}"))?;
        Ok(())
    }

    /// First policy-chain target authorized for automatic use that has not
    /// been considered in this attempt cycle (compared as the full
    /// engine/model/provider triple, so same-engine alternate providers each
    /// get their one try). Read-only selection over the session's policy
    /// snapshot; never loops back to a considered target.
    pub(crate) fn next_authorized_fallback_target(
        &self,
        product_session_id: &str,
        considered: &[EngineTarget],
    ) -> Result<Option<EngineTarget>, String> {
        let connection = self.lock()?;
        let policy = query_fallback_policy(&connection, Some(product_session_id))?;
        Ok(policy.targets.into_iter().find(|target| {
            target.automatic_use_authorized
                && !considered.iter().any(|tried| {
                    tried.engine_id == target.engine_id
                        && tried.model_id == target.model_id
                        && tried.provider_id == target.provider_id
                })
        }))
    }

    /// Record that a fallback candidate was skipped (unhealthy or capability
    /// mismatch) without ever executing, so the reason survives in the
    /// timeline.
    pub(crate) fn record_fallback_skip(
        &self,
        product_session_id: &str,
        source_branch_id: &str,
        source_engine_id: &str,
        target: &EngineTarget,
        reason: &str,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub fallback skip transaction: {error}"))?;
        insert_switch_timeline(
            &transaction,
            product_session_id,
            "engine_fallback_skipped",
            source_branch_id,
            None,
            source_engine_id,
            &target.engine_id,
            reason,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub fallback skip: {error}"))?;
        Ok(())
    }

    /// Record that the fallback chain is exhausted; the reason carries every
    /// attempt's terminal state so the card can show the whole cycle.
    pub(crate) fn record_fallback_exhaustion(
        &self,
        product_session_id: &str,
        branch_id: &str,
        engine_id: &str,
        summary: &str,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub fallback exhaustion transaction: {error}"))?;
        insert_switch_timeline(
            &transaction,
            product_session_id,
            "fallback_exhausted",
            branch_id,
            None,
            engine_id,
            engine_id,
            summary,
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub fallback exhaustion: {error}"))?;
        Ok(())
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
            // ToolEnd updates carry no input; keep the path ToolStart saw.
            let path = evidence.path.clone().or_else(|| existing.path.take());
            *existing = evidence;
            existing.path = path;
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

fn validate_engine_target(target: &EngineTarget) -> Result<(), String> {
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
    if target
        .required_capabilities
        .iter()
        .any(|capability| capability.trim().is_empty())
    {
        return Err(format!(
            "engine target '{}' has an empty required capability",
            target.engine_id
        ));
    }
    Ok(())
}

fn engine_target_from_branch(
    engine_id: String,
    model_id: String,
    provider_id: String,
) -> EngineTarget {
    let mut target = EngineTarget::for_engine(&engine_id)
        .expect("Hub engine branches are created only from supported engine targets");
    target.model_id = model_id;
    target.provider_id = provider_id;
    target
}

fn validate_fallback_targets(targets: &[EngineTarget]) -> Result<(), String> {
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

fn query_fallback_policy(
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

fn persist_fallback_policy(
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

fn upsert_queued_switch(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    source_branch_id: &str,
    target: &EngineTarget,
    reason: &str,
    now: i64,
) -> Result<String, String> {
    let existing = transaction
        .query_row(
            "SELECT switch_id FROM engine_switches
              WHERE product_session_id = ?1 AND status = 'queued'",
            [product_session_id],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("read queued Hub engine switch: {error}"))?;
    if let Some(switch_id) = existing {
        transaction
            .execute(
                "UPDATE engine_switches
                    SET source_branch_id = ?2, target_engine_id = ?3,
                        target_model_id = ?4, target_provider_id = ?5,
                        reason = ?6, requested_at = ?7
                  WHERE switch_id = ?1",
                params![
                    switch_id,
                    source_branch_id,
                    target.engine_id,
                    target.model_id,
                    target.provider_id,
                    reason,
                    now
                ],
            )
            .map_err(|error| format!("update queued Hub engine switch: {error}"))?;
        return Ok(switch_id);
    }

    let switch_id = uuid::Uuid::new_v4().to_string();
    transaction
        .execute(
            "INSERT INTO engine_switches
               (switch_id, product_session_id, source_branch_id, target_branch_id,
                target_engine_id, target_model_id, target_provider_id, reason,
                status, handoff_id, requested_at, applied_at)
             VALUES (?1, ?2, ?3, NULL, ?4, ?5, ?6, ?7, 'queued', NULL, ?8, NULL)",
            params![
                switch_id,
                product_session_id,
                source_branch_id,
                target.engine_id,
                target.model_id,
                target.provider_id,
                reason,
                now
            ],
        )
        .map_err(|error| format!("queue Hub engine switch: {error}"))?;
    Ok(switch_id)
}

fn apply_switch_in_transaction(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    switch_id: &str,
    source_session: &HubSessionSummary,
    target: &EngineTarget,
    reason: &str,
    now: i64,
) -> Result<SwitchOutcome, String> {
    let existing_target = transaction
        .query_row(
            "SELECT branch_id FROM engine_branches
              WHERE product_session_id = ?1 AND engine_id = ?2
                AND model_id = ?3 AND provider_id = ?4",
            params![
                product_session_id,
                target.engine_id,
                target.model_id,
                target.provider_id
            ],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|error| format!("find target Hub engine branch: {error}"))?;
    let target_branch_id = existing_target.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    if transaction
        .query_row(
            "SELECT 1 FROM engine_branches WHERE branch_id = ?1",
            [&target_branch_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("check target Hub engine branch: {error}"))?
        .is_none()
    {
        transaction
            .execute(
                "INSERT INTO engine_branches
                   (branch_id, product_session_id, engine_id, model_id, provider_id,
                    engine_session_id, status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'parked', ?6, ?6)",
                params![
                    target_branch_id,
                    product_session_id,
                    target.engine_id,
                    target.model_id,
                    target.provider_id,
                    now
                ],
            )
            .map_err(|error| format!("create target Hub engine branch: {error}"))?;
    }

    transaction
        .execute(
            "UPDATE engine_branches
                SET status = 'parked', updated_at = ?2,
                    watermark_turn_rowid = (
                      SELECT MAX(t.rowid) FROM turns t
                       WHERE t.product_session_id = ?1
                    )
              WHERE branch_id = ?3",
            params![product_session_id, now, source_session.active_branch_id],
        )
        .map_err(|error| format!("record Hub source branch watermark: {error}"))?;
    transaction
        .execute(
            "UPDATE engine_branches SET status = 'parked', updated_at = ?2
              WHERE product_session_id = ?1 AND branch_id <> ?3",
            params![product_session_id, now, source_session.active_branch_id],
        )
        .map_err(|error| format!("park inactive Hub engine branches: {error}"))?;
    transaction
        .execute(
            "UPDATE engine_branches SET status = 'active', updated_at = ?2
              WHERE branch_id = ?1",
            params![target_branch_id, now],
        )
        .map_err(|error| format!("activate target Hub engine branch: {error}"))?;
    transaction
        .execute(
            "UPDATE hub_sessions SET active_branch_id = ?2, updated_at = ?3
              WHERE product_session_id = ?1",
            params![product_session_id, target_branch_id, now],
        )
        .map_err(|error| format!("activate Hub session engine branch: {error}"))?;

    let mut source = EngineTarget::for_engine(&source_session.engine_id)?;
    source.model_id = source_session.model_id.clone();
    source.provider_id = source_session.provider_id.clone();
    let package = build_handoff_package(
        transaction,
        product_session_id,
        &source_session.active_branch_id,
        &target_branch_id,
        source,
        target.clone(),
        reason,
        now,
        HandoffKind::Full,
        None,
    )?;
    persist_handoff_package(transaction, &package)?;
    transaction
        .execute(
            "UPDATE engine_switches
                SET target_branch_id = ?2, status = 'applied', handoff_id = ?3,
                    applied_at = ?4
              WHERE switch_id = ?1 AND status = 'queued'",
            params![switch_id, target_branch_id, package.handoff_id, now],
        )
        .map_err(|error| format!("complete Hub engine switch: {error}"))?;
    insert_switch_timeline(
        transaction,
        product_session_id,
        "engine_switched",
        &source_session.active_branch_id,
        Some(&target_branch_id),
        &source_session.engine_id,
        &target.engine_id,
        reason,
        now,
    )?;

    Ok(SwitchOutcome {
        disposition: SwitchDisposition::Applied,
        active_branch_id: target_branch_id,
        active_engine_id: target.engine_id.clone(),
        target_engine_id: target.engine_id.clone(),
    })
}

#[allow(clippy::too_many_arguments)]
fn insert_switch_timeline(
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

#[allow(clippy::too_many_arguments)]
fn build_handoff_package(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    source_branch_id: &str,
    target_branch_id: &str,
    source: EngineTarget,
    target: EngineTarget,
    reason: &str,
    now: i64,
    kind: HandoffKind,
    watermark_turn_rowid: Option<i64>,
) -> Result<HandoffPackage, String> {
    let (workspace, include_full_transcript) = transaction
        .query_row(
            "SELECT workspace, include_full_transcript
               FROM hub_sessions WHERE product_session_id = ?1",
            [product_session_id],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, bool>(1)?)),
        )
        .map_err(|error| format!("read workspace for Hub handoff: {error}"))?;
    let mut statement = transaction
        .prepare(
            "SELECT rowid, prompt, final_result, error_message, status
               FROM turns WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare turn history for Hub handoff: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, Option<String>>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
            ))
        })
        .map_err(|error| format!("query turn history for Hub handoff: {error}"))?;
    let history = rows
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode turn history for Hub handoff: {error}"))?;
    drop(statement);
    let user_goal = history.first().map(|row| bounded_text(&row.1));
    let latest_instruction = history.last().map(|row| bounded_text(&row.1));
    let completed_work = history
        .iter()
        .filter(|(rowid, _, _, _, _)| {
            watermark_turn_rowid.is_none_or(|watermark| *rowid > watermark)
        })
        .filter_map(|(_, prompt, result, _, status)| {
            (status == STATUS_COMPLETED).then(|| {
                result.as_ref().map(|result| HandoffCompletedWork {
                    prompt: bounded_text(prompt),
                    result: bounded_text(result),
                })
            })?
        })
        .collect();
    let pending_work = history
        .iter()
        .filter(|(rowid, _, _, _, _)| {
            watermark_turn_rowid.is_none_or(|watermark| *rowid > watermark)
        })
        .filter(|(_, _, _, _, status)| {
            matches!(status.as_str(), STATUS_FAILED | STATUS_INTERRUPTED)
        })
        .map(|(_, prompt, _, error, status)| HandoffPendingWork {
            prompt: bounded_text(prompt),
            reason: bounded_text(error.as_deref().unwrap_or(status)),
        })
        .collect();
    let mut side_effect_statement = transaction
        .prepare(
            "SELECT ea.side_effects_json
               FROM engine_attempts ea
               JOIN turns t ON t.turn_id = ea.turn_id
              WHERE ea.product_session_id = ?1
                AND (?2 IS NULL OR t.rowid > ?2)
              ORDER BY ea.started_at ASC, ea.rowid ASC",
        )
        .map_err(|error| format!("prepare side effects for Hub handoff: {error}"))?;
    let encoded = side_effect_statement
        .query_map(params![product_session_id, watermark_turn_rowid], |row| {
            row.get::<_, String>(0)
        })
        .map_err(|error| format!("query side effects for Hub handoff: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode side effects for Hub handoff: {error}"))?;
    let mut known_side_effects = Vec::new();
    for json in encoded {
        let mut evidence: Vec<SideEffectEvidence> = serde_json::from_str(&json)
            .map_err(|error| format!("decode side effects for Hub handoff: {error}"))?;
        known_side_effects.append(&mut evidence);
    }
    // Full transcript is opt-in per session; when enabled it passes the same
    // filter as every other field (redact_handoff below).
    let transcript = if include_full_transcript {
        history
            .iter()
            .map(|(_, prompt, result, error, _)| TranscriptTurn {
                prompt: prompt.clone(),
                result: result.clone().or_else(|| error.clone()),
            })
            .collect()
    } else {
        Vec::new()
    };

    let mut package = HandoffPackage {
        handoff_id: uuid::Uuid::new_v4().to_string(),
        product_session_id: product_session_id.to_string(),
        source_branch_id: source_branch_id.to_string(),
        target_branch_id: target_branch_id.to_string(),
        source,
        target,
        kind,
        reason: reason.to_string(),
        user_goal,
        latest_instruction,
        accepted_decisions: Vec::new(),
        completed_work,
        pending_work,
        changed_files: Vec::new(),
        recent_commands: Vec::new(),
        verification_results: Vec::new(),
        workspace_summary: format!(
            "Workspace (authoritative; inspect it before trusting transcript evidence): {workspace}"
        ),
        known_side_effects,
        team_snapshot: None,
        transcript,
        created_at: now,
        used_at: None,
    };
    // The one filter every cross-engine handoff passes, BEFORE persistence —
    // stored packages equal what the target adapter receives.
    crate::redact::redact_handoff(&mut package);
    Ok(package)
}

fn persist_handoff_package(
    transaction: &Transaction<'_>,
    package: &HandoffPackage,
) -> Result<(), String> {
    let package_json = serde_json::to_string(package)
        .map_err(|error| format!("serialize {:?} Hub handoff: {error}", package.kind))?;
    transaction
        .execute(
            "INSERT INTO handoff_packages
               (handoff_id, product_session_id, source_branch_id, target_branch_id,
                kind, reason, package_json, created_at, used_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL)",
            params![
                package.handoff_id,
                package.product_session_id,
                package.source_branch_id,
                package.target_branch_id,
                handoff_kind_name(package.kind),
                package.reason,
                package_json,
                package.created_at
            ],
        )
        .map_err(|error| format!("persist Hub handoff: {error}"))?;
    Ok(())
}

fn bounded_text(text: &str) -> String {
    const MAX_CHARS: usize = 2_000;
    let mut chars = text.chars();
    let bounded: String = chars.by_ref().take(MAX_CHARS).collect();
    if chars.next().is_some() {
        format!("{bounded}…")
    } else {
        bounded
    }
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
            "SELECT branch_id, engine_id, model_id, provider_id, engine_session_id,
                    watermark_turn_rowid, status, created_at, updated_at
               FROM engine_branches WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub engine branches: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok(StoredBranch {
                branch_id: row.get(0)?,
                engine_id: row.get(1)?,
                model_id: row.get(2)?,
                provider_id: row.get(3)?,
                engine_session_id: row.get(4)?,
                watermark_turn_rowid: row.get(5)?,
                status: row.get(6)?,
                created_at: row.get(7)?,
                updated_at: row.get(8)?,
            })
        })
        .map_err(|error| format!("query Hub engine branches: {error}"))?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub engine branches: {error}"))
}

fn query_handoffs(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Vec<HandoffPackage>, String> {
    let mut statement = connection
        .prepare(
            "SELECT package_json, used_at FROM handoff_packages
              WHERE product_session_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub handoffs: {error}"))?;
    let rows = statement
        .query_map([product_session_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<i64>>(1)?))
        })
        .map_err(|error| format!("query Hub handoffs: {error}"))?;
    rows.map(|row| {
        let (json, used_at) = row.map_err(|error| format!("decode Hub handoff row: {error}"))?;
        let mut package: HandoffPackage = serde_json::from_str(&json)
            .map_err(|error| format!("decode Hub handoff package: {error}"))?;
        package.used_at = used_at;
        Ok(package)
    })
    .collect()
}

fn query_latest_handoff(
    connection: &Connection,
    target_branch_id: &str,
    kind: Option<HandoffKind>,
) -> Result<Option<HandoffPackage>, String> {
    let sql = if kind.is_some() {
        "SELECT package_json FROM handoff_packages
           WHERE target_branch_id = ?1 AND kind = ?2
           ORDER BY created_at DESC, rowid DESC LIMIT 1"
    } else {
        "SELECT package_json FROM handoff_packages
           WHERE target_branch_id = ?1
           ORDER BY created_at DESC, rowid DESC LIMIT 1"
    };
    let json = match kind {
        Some(kind) => connection.query_row(
            sql,
            params![target_branch_id, handoff_kind_name(kind)],
            |row| row.get::<_, String>(0),
        ),
        None => connection.query_row(sql, [target_branch_id], |row| row.get::<_, String>(0)),
    }
    .optional()
    .map_err(|error| format!("query latest Hub handoff: {error}"))?;
    json.map(|json| {
        serde_json::from_str(&json).map_err(|error| format!("decode latest Hub handoff: {error}"))
    })
    .transpose()
}

fn handoff_kind_name(kind: HandoffKind) -> &'static str {
    match kind {
        HandoffKind::Full => "full",
        HandoffKind::Delta => "delta",
    }
}

fn source_branch_for_handoff(prior: &Option<HandoffPackage>, fallback_branch_id: &str) -> String {
    prior
        .as_ref()
        .map(|package| package.source_branch_id.clone())
        .unwrap_or_else(|| fallback_branch_id.to_string())
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

fn query_pending_switch(
    connection: &Connection,
    product_session_id: &str,
) -> Result<Option<PendingSwitch>, String> {
    connection
        .query_row(
            "SELECT switch_id, source_branch_id, target_engine_id,
                    target_model_id, target_provider_id, reason, requested_at
               FROM engine_switches
              WHERE product_session_id = ?1 AND status = 'queued'",
            [product_session_id],
            |row| {
                Ok(PendingSwitch {
                    switch_id: row.get(0)?,
                    source_branch_id: row.get(1)?,
                    target: engine_target_from_branch(row.get(2)?, row.get(3)?, row.get(4)?),
                    reason: row.get(5)?,
                    requested_at: row.get(6)?,
                })
            },
        )
        .optional()
        .map_err(|error| format!("query pending Hub engine switch: {error}"))
}

fn query_session(
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
                    hs.permission_profile, hs.include_full_transcript
               FROM hub_sessions hs
               JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
              WHERE hs.product_session_id = ?1",
            [product_session_id],
            decode_session,
        )
        .optional()
        .map_err(|error| format!("query Hub session '{product_session_id}': {error}"))
}

fn query_team_run(
    connection: &Connection,
    team_run_id: &str,
) -> Result<Option<TeamRunDetail>, String> {
    let run = connection
        .query_row(
            "SELECT team_run_id, lead_session_id, status, created_at
               FROM team_runs WHERE team_run_id = ?1",
            [team_run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("query Hub team run: {error}"))?;
    let Some((team_run_id, lead_session_id, status, created_at)) = run else {
        return Ok(None);
    };
    let mut worker_statement = connection
        .prepare(
            "SELECT tw.worker_id, tw.worker_session_id, eb.engine_id, tw.created_at
               FROM team_workers tw
               JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
               JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
              WHERE tw.team_run_id = ?1
              ORDER BY tw.created_at ASC, tw.rowid ASC",
        )
        .map_err(|error| format!("prepare Hub team workers: {error}"))?;
    let workers = worker_statement
        .query_map([&team_run_id], |row| {
            Ok(TeamWorker {
                worker_id: row.get(0)?,
                worker_session_id: row.get(1)?,
                engine_id: row.get(2)?,
                created_at: row.get(3)?,
            })
        })
        .map_err(|error| format!("query Hub team workers: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub team workers: {error}"))?;
    let mut task_statement = connection
        .prepare(
            "SELECT task_id FROM worker_tasks WHERE team_run_id = ?1
              ORDER BY created_at ASC, rowid ASC",
        )
        .map_err(|error| format!("prepare Hub worker tasks: {error}"))?;
    let task_ids = task_statement
        .query_map([&team_run_id], |row| row.get::<_, String>(0))
        .map_err(|error| format!("query Hub worker tasks: {error}"))?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(|error| format!("decode Hub worker tasks: {error}"))?;
    let mut tasks = Vec::new();
    for task_id in task_ids {
        if let Some(task) = query_worker_task(connection, &task_id)? {
            tasks.push(task);
        }
    }
    Ok(Some(TeamRunDetail {
        team_run_id,
        lead_session_id,
        status,
        workers,
        tasks,
        created_at,
    }))
}

fn query_worker_task(connection: &Connection, task_id: &str) -> Result<Option<WorkerTask>, String> {
    connection
        .query_row(
            "SELECT task_id, team_run_id, worker_id, scope, status, result,
                    changed_files_json, verification_json, claims_json,
                    created_at, completed_at
               FROM worker_tasks WHERE task_id = ?1",
            [task_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, Option<String>>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, String>(7)?,
                    row.get::<_, String>(8)?,
                    row.get::<_, i64>(9)?,
                    row.get::<_, Option<i64>>(10)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("query Hub worker task: {error}"))?
        .map(
            |(
                task_id,
                team_run_id,
                worker_id,
                scope,
                status,
                result,
                changed_files_json,
                verification_json,
                claims_json,
                created_at,
                completed_at,
            )| {
                Ok(WorkerTask {
                    task_id,
                    team_run_id,
                    worker_id,
                    scope,
                    status,
                    result,
                    changed_files: serde_json::from_str(&changed_files_json)
                        .map_err(|error| format!("decode Hub task files: {error}"))?,
                    verification: serde_json::from_str(&verification_json)
                        .map_err(|error| format!("decode Hub task evidence: {error}"))?,
                    claims: serde_json::from_str(&claims_json)
                        .map_err(|error| format!("decode Hub task claims: {error}"))?,
                    created_at,
                    completed_at,
                })
            },
        )
        .transpose()
}

fn decode_session(row: &rusqlite::Row<'_>) -> rusqlite::Result<HubSessionSummary> {
    let profile_text: String = row.get(11)?;
    Ok(HubSessionSummary {
        product_session_id: row.get(0)?,
        workspace: row.get(1)?,
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
    use crate::claims::ClaimMode;

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
            .prepare_new_turn(
                "product-1",
                workspace.path(),
                "hello",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
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
            .prepare_new_turn(
                "product-crash",
                workspace.path(),
                "change the project",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
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
                    path: Some("changed.txt".into()),
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
        // Pre-profile sessions keep their de-facto contract after migration.
        assert_eq!(
            detail.session.permission_profile,
            PermissionProfile::WorkspaceWrite
        );
        let version = store
            .lock()
            .unwrap()
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, SCHEMA_VERSION);
    }

    #[test]
    fn permission_profile_persists_and_survives_branch_switches() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-profile",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::Unrestricted,
            )
            .unwrap();
        assert_eq!(prepared.permission_profile, PermissionProfile::Unrestricted);
        store
            .finish_turn(&prepared, Some("alva-native"), Some("done"), None, None)
            .unwrap();

        // A manual engine switch replaces the active branch but must never
        // touch the session-stable permission profile.
        store
            .request_engine_switch("product-profile", &claude, "user chose claude")
            .unwrap();
        let switched = store.get_session("product-profile").unwrap().unwrap();
        assert_eq!(switched.session.engine_id, "claude");
        assert_eq!(
            switched.session.permission_profile,
            PermissionProfile::Unrestricted
        );

        // Resumed turns carry the stored profile, and it survives reopen.
        let resumed = store
            .prepare_resumed_turn("product-profile", "continue")
            .unwrap();
        assert_eq!(resumed.permission_profile, PermissionProfile::Unrestricted);
        store
            .finish_turn(&resumed, None, Some("ok"), None, None)
            .unwrap();
        drop(store);
        let reopened = HubStore::open(&database).unwrap();
        assert_eq!(
            reopened
                .get_session("product-profile")
                .unwrap()
                .unwrap()
                .session
                .permission_profile,
            PermissionProfile::Unrestricted
        );
    }

    #[test]
    fn manual_switch_to_a_weaken_only_target_is_rejected_with_the_reason() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-gated-switch",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::Interactive,
            )
            .unwrap();
        store
            .finish_turn(&prepared, Some("alva-native"), Some("done"), None, None)
            .unwrap();

        for engine_id in ["claude", "codex"] {
            let target = EngineTarget::for_engine(engine_id).unwrap();
            let error = store
                .request_engine_switch("product-gated-switch", &target, "user insisted")
                .expect_err("a weaken-only manual switch must be rejected");
            assert!(
                error.contains("cannot preserve or tighten") && error.contains("interactive"),
                "{engine_id}: {error}"
            );
        }
        // Re-selecting the current engine stays a permitted no-op.
        let same = store
            .request_engine_switch("product-gated-switch", &alva, "stay on alva")
            .unwrap();
        assert_eq!(same.disposition, SwitchDisposition::Applied);
        let detail = store.get_session("product-gated-switch").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "alva");
        assert!(detail.pending_switch.is_none());
    }

    #[test]
    fn dirty_permission_profile_text_is_rejected_loudly() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-dirty",
                workspace.path(),
                "work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE hub_sessions SET permission_profile = 'yolo'
                  WHERE product_session_id = 'product-dirty'",
                [],
            )
            .unwrap();

        let error = store
            .get_session("product-dirty")
            .expect_err("a corrupted profile must not silently default");
        assert!(
            error.contains("yolo"),
            "error should name the value: {error}"
        );
    }

    #[test]
    fn rejects_a_future_schema_without_downgrading_it() {
        let root = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let connection = Connection::open(&database).unwrap();
        connection.pragma_update(None, "user_version", 11).unwrap();
        drop(connection);

        let error = HubStore::open(&database)
            .err()
            .expect("future schema must be rejected");
        assert!(error.contains("newer than supported"));

        let connection = Connection::open(&database).unwrap();
        let version = connection
            .query_row("PRAGMA user_version", [], |row| row.get::<_, i64>(0))
            .unwrap();
        assert_eq!(version, 11);
    }

    #[test]
    fn manual_switch_parks_source_and_persists_full_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let prepared = store
            .prepare_new_turn(
                "product-switch",
                workspace.path(),
                "build the feature",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&prepared, "alva-native-1")
            .unwrap();
        store
            .finish_turn(
                &prepared,
                Some("alva-native-1"),
                Some("feature complete"),
                None,
                None,
            )
            .unwrap();

        let switched = store
            .request_engine_switch("product-switch", &claude, "User explicitly selected Claude")
            .unwrap();
        assert_eq!(switched.disposition, SwitchDisposition::Applied);
        assert_eq!(switched.active_engine_id, "claude");

        let detail = store.get_session("product-switch").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "claude");
        assert_eq!(detail.branches.len(), 2);
        let source = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert_eq!(source.status, "parked");
        assert_eq!(source.engine_session_id.as_deref(), Some("alva-native-1"));
        let target = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "claude")
            .unwrap();
        assert_eq!(target.status, "active");
        assert_eq!(target.model_id, "engine-default");
        assert_eq!(target.provider_id, "claude-cli");
        assert_eq!(detail.handoffs.len(), 1);
        let handoff = &detail.handoffs[0];
        assert_eq!(handoff.source_branch_id, source.branch_id);
        assert_eq!(handoff.target_branch_id, target.branch_id);
        assert_eq!(handoff.reason, "User explicitly selected Claude");
        assert_eq!(handoff.user_goal.as_deref(), Some("build the feature"));
        assert_eq!(
            handoff.latest_instruction.as_deref(),
            Some("build the feature")
        );
        assert_eq!(handoff.completed_work.len(), 1);
        assert_eq!(handoff.completed_work[0].result, "feature complete");
        assert_eq!(detail.timeline_events.len(), 1);
        assert_eq!(detail.timeline_events[0].kind, "engine_switched");

        drop(store);
        let reopened = HubStore::open(&database).unwrap();
        let recovered = reopened.get_session("product-switch").unwrap().unwrap();
        assert_eq!(recovered.session.engine_id, "claude");
        assert_eq!(recovered.branches.len(), 2);
        assert_eq!(recovered.handoffs.len(), 1);
    }

    #[test]
    fn running_switch_is_queued_until_the_attempt_is_terminal() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let codex = EngineTarget::for_engine("codex").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-queued",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();

        let queued = store
            .request_engine_switch("product-queued", &codex, "User explicitly selected Codex")
            .unwrap();
        assert_eq!(queued.disposition, SwitchDisposition::Queued);
        assert_eq!(queued.active_engine_id, "alva");
        let running = store.get_session("product-queued").unwrap().unwrap();
        assert_eq!(running.session.engine_id, "alva");
        assert_eq!(
            running.pending_switch.as_ref().unwrap().target.engine_id,
            "codex"
        );
        assert_eq!(running.timeline_events[0].kind, "engine_switch_queued");

        let cancelled = store
            .request_engine_switch(
                "product-queued",
                &alva,
                "User returned to the active engine",
            )
            .unwrap();
        assert_eq!(cancelled.disposition, SwitchDisposition::Applied);
        assert!(store
            .get_session("product-queued")
            .unwrap()
            .unwrap()
            .pending_switch
            .is_none());
        store
            .request_engine_switch(
                "product-queued",
                &codex,
                "User explicitly selected Codex again",
            )
            .unwrap();

        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        let applied = store
            .apply_pending_switch("product-queued")
            .unwrap()
            .expect("queued switch should apply at the safe boundary");
        assert_eq!(applied.disposition, SwitchDisposition::Applied);
        assert_eq!(applied.active_engine_id, "codex");

        let detail = store.get_session("product-queued").unwrap().unwrap();
        assert!(detail.pending_switch.is_none());
        assert_eq!(detail.session.engine_id, "codex");
        assert_eq!(detail.timeline_events.len(), 4);
        assert_eq!(detail.timeline_events[1].kind, "engine_switch_cancelled");
        assert_eq!(detail.timeline_events[3].kind, "engine_switched");
    }

    #[test]
    fn reopening_applies_a_switch_queued_before_a_crash() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        store
            .prepare_new_turn(
                "product-crash-switch",
                workspace.path(),
                "work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .request_engine_switch(
                "product-crash-switch",
                &EngineTarget::for_engine("claude").unwrap(),
                "User selected Claude before restart",
            )
            .unwrap();
        drop(store);

        let reopened = HubStore::open(&database).unwrap();
        let detail = reopened
            .get_session("product-crash-switch")
            .unwrap()
            .unwrap();
        assert_eq!(detail.session.status, STATUS_INTERRUPTED);
        assert_eq!(detail.session.engine_id, "claude");
        assert!(detail.pending_switch.is_none());
        assert_eq!(detail.handoffs.len(), 1);
    }

    #[test]
    fn parked_branch_records_watermark_and_builds_a_delta_handoff_on_return() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let first = store
            .prepare_new_turn(
                "product-delta",
                workspace.path(),
                "alva work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.record_engine_session(&first, "alva-native").unwrap();
        store
            .finish_turn(
                &first,
                Some("alva-native"),
                Some("alva complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-delta", &claude, "switch to Claude")
            .unwrap();

        let claude_turn = store
            .prepare_resumed_turn("product-delta", "claude work")
            .unwrap();
        store
            .finish_turn(
                &claude_turn,
                Some("claude-native"),
                Some("claude complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-delta", &alva, "return to Alva")
            .unwrap();

        let detail = store.get_session("product-delta").unwrap().unwrap();
        let alva_branch = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert!(alva_branch.watermark_turn_rowid.is_some());

        let resumed = store
            .prepare_resumed_turn("product-delta", "resume Alva")
            .unwrap();
        assert_eq!(resumed.engine_session_id.as_deref(), Some("alva-native"));
        let handoff = resumed
            .handoff
            .expect("native resume must receive a handoff");
        assert_eq!(handoff.kind, HandoffKind::Delta);
        assert_eq!(handoff.completed_work.len(), 1);
        assert_eq!(handoff.completed_work[0].result, "claude complete");
        assert!(!handoff
            .completed_work
            .iter()
            .any(|work| work.result == "alva complete"));
    }

    #[test]
    fn native_resume_upgrade_is_crash_recoverable_with_full_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        let first = store
            .prepare_new_turn(
                "product-upgrade",
                workspace.path(),
                "alva work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .record_engine_session(&first, "expired-native")
            .unwrap();
        store
            .finish_turn(
                &first,
                Some("expired-native"),
                Some("alva complete"),
                None,
                None,
            )
            .unwrap();
        store
            .request_engine_switch("product-upgrade", &claude, "switch to Claude")
            .unwrap();
        let claude_turn = store
            .prepare_resumed_turn("product-upgrade", "claude work")
            .unwrap();
        store
            .finish_turn(&claude_turn, None, Some("claude complete"), None, None)
            .unwrap();
        store
            .request_engine_switch("product-upgrade", &alva, "return to Alva")
            .unwrap();
        let resume = store
            .prepare_resumed_turn("product-upgrade", "resume Alva")
            .unwrap();
        assert!(resume.engine_session_id.is_some());

        let fresh = store
            .upgrade_failed_native_resume(&resume, "Session not found: expired-native")
            .unwrap();
        assert!(fresh.engine_session_id.is_none());
        assert_eq!(fresh.handoff.as_ref().unwrap().kind, HandoffKind::Full);
        drop(store);

        let reopened = HubStore::open(&database).unwrap();
        let detail = reopened.get_session("product-upgrade").unwrap().unwrap();
        assert_eq!(detail.session.engine_session_id, None);
        assert_eq!(detail.turns.last().unwrap().status, STATUS_INTERRUPTED);
        assert_eq!(detail.turns.last().unwrap().attempts.len(), 2);
        assert!(detail
            .handoffs
            .iter()
            .any(|handoff| handoff.kind == HandoffKind::Full));
    }

    #[test]
    fn new_session_snapshots_global_fallback_policy_without_later_drift() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        claude.required_capabilities = vec!["streaming".into(), "resume".into()];
        store
            .set_global_fallback_policy(vec![claude.clone()])
            .unwrap();

        store
            .prepare_new_turn(
                "product-policy-snapshot",
                workspace.path(),
                "start work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let first = store
            .get_session("product-policy-snapshot")
            .unwrap()
            .unwrap();
        assert_eq!(first.fallback_policy.targets, vec![claude]);

        let mut codex = EngineTarget::for_engine("codex").unwrap();
        codex.automatic_use_authorized = true;
        store
            .set_global_fallback_policy(vec![codex.clone()])
            .unwrap();
        let unchanged = store
            .get_session("product-policy-snapshot")
            .unwrap()
            .unwrap();
        assert_eq!(unchanged.fallback_policy.targets[0].engine_id, "claude");

        store
            .prepare_new_turn(
                "product-policy-new",
                workspace.path(),
                "start another",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        let later = store.get_session("product-policy-new").unwrap().unwrap();
        assert_eq!(later.fallback_policy.targets, vec![codex]);
    }

    #[test]
    fn automatic_fallback_needs_explicit_session_authorization_but_manual_switch_does_not() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-policy-auth",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store
            .set_session_fallback_policy("product-policy-auth", vec![claude.clone(), alva.clone()])
            .unwrap();
        let policy = store
            .get_session("product-policy-auth")
            .unwrap()
            .unwrap()
            .fallback_policy;
        assert_eq!(
            policy
                .targets
                .iter()
                .map(|target| target.engine_id.as_str())
                .collect::<Vec<_>>(),
            vec!["claude", "alva"]
        );
        assert!(!store
            .is_authorized_for_automatic_fallback("product-policy-auth", &claude)
            .unwrap());

        let manual = store
            .request_engine_switch(
                "product-policy-auth",
                &claude,
                "User explicitly selected Claude",
            )
            .unwrap();
        assert_eq!(manual.active_engine_id, "claude");

        let mut authorized = claude.clone();
        authorized.automatic_use_authorized = true;
        store
            .set_session_fallback_policy("product-policy-auth", vec![authorized.clone()])
            .unwrap();
        assert!(store
            .is_authorized_for_automatic_fallback("product-policy-auth", &authorized)
            .unwrap());
    }

    #[test]
    fn session_fallback_policy_and_target_account_metadata_survive_restart() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-policy-restart",
                workspace.path(),
                "work",
                &EngineTarget::for_engine("alva").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        let mut codex = EngineTarget::for_engine("codex").unwrap();
        codex.account_source = "Personal Codex subscription".into();
        codex.billing_route = "OpenAI account · monthly plan".into();
        codex.automatic_use_authorized = true;
        codex.required_capabilities = vec!["streaming".into()];
        store
            .set_session_fallback_policy("product-policy-restart", vec![codex.clone()])
            .unwrap();
        drop(store);

        let reopened = HubStore::open(&database).unwrap();
        let detail = reopened
            .get_session("product-policy-restart")
            .unwrap()
            .unwrap();
        assert_eq!(detail.fallback_policy.targets, vec![codex]);
    }

    #[test]
    fn automatic_fallback_continues_the_turn_with_serial_attempt_and_persisted_handoff() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        let prepared = store
            .prepare_new_turn(
                "product-fallback",
                workspace.path(),
                "do the work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback", vec![claude.clone()])
            .unwrap();

        let next = store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .unwrap();
        assert_eq!(next.turn_id, prepared.turn_id);
        assert_ne!(next.attempt_id, prepared.attempt_id);
        assert_eq!(next.target.engine_id, "claude");
        assert!(next.handoff.as_ref().is_some_and(|handoff| {
            handoff.kind == HandoffKind::Full && handoff.reason == "engine process died"
        }));
        assert!(!next.native_resume);

        let detail = store.get_session("product-fallback").unwrap().unwrap();
        assert_eq!(detail.session.engine_id, "claude");
        assert_eq!(detail.session.status, "running");
        let source = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "alva")
            .unwrap();
        assert_eq!(source.status, "parked");
        let target = detail
            .branches
            .iter()
            .find(|branch| branch.engine_id == "claude")
            .unwrap();
        assert_eq!(target.status, "active");

        assert_eq!(detail.turns.len(), 1);
        let attempts = &detail.turns[0].attempts;
        assert_eq!(attempts.len(), 2);
        assert_eq!(attempts[0].engine_id, "alva");
        assert_eq!(attempts[0].status, "failed");
        assert_eq!(
            attempts[0].failure_reason.as_deref(),
            Some("engine process died")
        );
        assert_eq!(attempts[1].engine_id, "claude");
        assert_eq!(attempts[1].status, "running");

        assert!(detail.handoffs.iter().any(
            |handoff| handoff.kind == HandoffKind::Full && handoff.target.engine_id == "claude"
        ));
        let fallback_event = detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "engine_fallback")
            .unwrap();
        assert_eq!(fallback_event.source_engine_id, "alva");
        assert_eq!(fallback_event.target_engine_id, "claude");
        assert!(fallback_event.reason.contains("engine process died"));
        assert!(fallback_event.reason.contains("handoff"));
    }

    #[test]
    fn fallback_requires_session_authorization_and_a_different_engine() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-fallback-auth",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback-auth", vec![claude.clone()])
            .unwrap();

        let unauthorized = store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .unwrap_err();
        assert!(unauthorized.contains("not authorized"));

        let same_engine = store
            .fallback_failed_attempt(&prepared, "engine process died", &alva)
            .unwrap_err();
        assert!(same_engine.contains("different engine"));

        // The rejected fallback must leave the source attempt recoverable.
        let detail = store.get_session("product-fallback-auth").unwrap().unwrap();
        assert_eq!(detail.turns[0].attempts.len(), 1);
        assert_eq!(detail.turns[0].attempts[0].status, "running");
        assert_eq!(detail.session.engine_id, "alva");

        claude.automatic_use_authorized = true;
        store
            .set_session_fallback_policy("product-fallback-auth", vec![claude.clone()])
            .unwrap();
        assert!(store
            .fallback_failed_attempt(&prepared, "engine process died", &claude)
            .is_ok());
    }

    #[test]
    fn returning_to_the_fallback_source_branch_receives_the_fallback_turn_in_the_delta() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;

        let first = store
            .prepare_new_turn(
                "product-fallback-delta",
                workspace.path(),
                "first task",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store.record_engine_session(&first, "alva-native").unwrap();
        store
            .finish_turn(
                &first,
                Some("alva-native"),
                Some("alva did the first task"),
                None,
                None,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-fallback-delta", vec![claude.clone()])
            .unwrap();

        let second = store
            .prepare_resumed_turn("product-fallback-delta", "second task")
            .unwrap();
        let fallback = store
            .fallback_failed_attempt(&second, "alva stream died", &claude)
            .unwrap();
        store
            .finish_turn(
                &fallback,
                Some("claude-native"),
                Some("claude completed the second task"),
                None,
                None,
            )
            .unwrap();

        store
            .request_engine_switch(
                "product-fallback-delta",
                &EngineTarget::for_engine("alva").unwrap(),
                "return to Alva",
            )
            .unwrap();
        let resumed = store
            .prepare_resumed_turn("product-fallback-delta", "third task")
            .unwrap();
        assert_eq!(resumed.engine_session_id.as_deref(), Some("alva-native"));
        let handoff = resumed
            .handoff
            .expect("returning source receives a handoff");
        assert_eq!(handoff.kind, HandoffKind::Delta);
        // The turn the fallback engine completed happened after the source
        // engine died — it must be part of the delta.
        assert!(handoff
            .completed_work
            .iter()
            .any(|work| work.result == "claude completed the second task"));
        assert!(!handoff
            .completed_work
            .iter()
            .any(|work| work.result == "alva did the first task"));
    }

    #[test]
    fn next_authorized_fallback_target_follows_chain_order_and_skips_tried_engines() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let codex = EngineTarget::for_engine("codex").unwrap();
        let mut claude = EngineTarget::for_engine("claude").unwrap();
        claude.automatic_use_authorized = true;
        store
            .prepare_new_turn(
                "product-chain",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy("product-chain", vec![codex, claude.clone()])
            .unwrap();

        // codex is first in the chain but unauthorized; claude is next.
        let selected = store
            .next_authorized_fallback_target("product-chain", std::slice::from_ref(&alva))
            .unwrap()
            .unwrap();
        assert_eq!(selected.engine_id, "claude");

        let exhausted = store
            .next_authorized_fallback_target("product-chain", &[alva, claude])
            .unwrap();
        assert!(exhausted.is_none());
    }

    #[test]
    fn same_engine_alternate_provider_targets_each_get_their_own_try() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let mut claude_a = EngineTarget::for_engine("claude").unwrap();
        claude_a.automatic_use_authorized = true;
        let mut claude_b = claude_a.clone();
        claude_b.provider_id = "claude-alt-provider".into();
        store
            .prepare_new_turn(
                "product-alt-provider",
                workspace.path(),
                "work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .set_session_fallback_policy(
                "product-alt-provider",
                vec![claude_a.clone(), claude_b.clone()],
            )
            .unwrap();

        let first = store
            .next_authorized_fallback_target("product-alt-provider", std::slice::from_ref(&alva))
            .unwrap()
            .unwrap();
        assert_eq!(first.provider_id, "claude-cli");

        let second = store
            .next_authorized_fallback_target(
                "product-alt-provider",
                &[alva.clone(), claude_a.clone()],
            )
            .unwrap()
            .unwrap();
        assert_eq!(second.provider_id, "claude-alt-provider");

        let exhausted = store
            .next_authorized_fallback_target("product-alt-provider", &[alva, claude_a, claude_b])
            .unwrap();
        assert!(exhausted.is_none());
    }

    #[test]
    fn transcript_is_opt_in_and_passes_the_same_filter() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();
        let prepared = store
            .prepare_new_turn(
                "product-transcript",
                workspace.path(),
                "set things up",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(
                &prepared,
                None,
                Some("done, used GITHUB_TOKEN=ghp_canarytoken99x"),
                None,
                None,
            )
            .unwrap();

        // Default: no transcript crosses engines.
        store
            .request_engine_switch("product-transcript", &claude, "try Claude")
            .unwrap();
        let detail = store.get_session("product-transcript").unwrap().unwrap();
        assert!(!detail.session.include_full_transcript);
        assert!(detail.handoffs.last().unwrap().transcript.is_empty());

        // Opt in: transcript attaches, but through the same filter.
        store
            .set_transcript_sharing("product-transcript", true)
            .unwrap();
        store
            .request_engine_switch(
                "product-transcript",
                &EngineTarget::for_engine("codex").unwrap(),
                "try Codex",
            )
            .unwrap();
        let detail = store.get_session("product-transcript").unwrap().unwrap();
        assert!(detail.session.include_full_transcript);
        let handoff = detail
            .handoffs
            .iter()
            .find(|handoff| handoff.target.engine_id == "codex")
            .unwrap();
        assert_eq!(handoff.transcript.len(), 1);
        let result = handoff.transcript[0].result.as_deref().unwrap();
        assert!(result.contains("[redacted]"));
        assert!(!result.contains("ghp_canarytoken99x"));
    }

    #[test]
    fn team_run_walking_skeleton_survives_reopen_with_all_associations() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let database = root.path().join("alva-hub.db");
        let store = HubStore::open(&database).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let claude = EngineTarget::for_engine("claude").unwrap();

        // No lead session — refused.
        assert!(store.create_team_run("missing-lead", &claude).is_err());

        let lead = store
            .prepare_new_turn(
                "lead-session",
                workspace.path(),
                "plan the work",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();

        let run = store.create_team_run("lead-session", &claude).unwrap();
        assert_eq!(run.lead_session_id, "lead-session");
        assert_eq!(run.workers.len(), 1);
        assert_eq!(run.workers[0].engine_id, "claude");
        // The worker is a Hub-owned durable session with its own branch and
        // the lead's profile — not an engine-private subagent.
        let worker_session = store
            .get_session(&run.workers[0].worker_session_id)
            .unwrap()
            .unwrap();
        assert_eq!(worker_session.session.engine_id, "claude");
        assert_eq!(
            worker_session.session.permission_profile,
            PermissionProfile::WorkspaceWrite
        );

        let queued = store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "implement the parser",
                &[],
            )
            .unwrap();
        assert_eq!(queued.status, "queued");
        let task = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        assert_eq!(task.status, "running");

        // Worker executes a turn with evidence, then the task closes.
        let worker_turn = store
            .prepare_resumed_turn(&run.workers[0].worker_session_id, "implement the parser")
            .unwrap();
        store
            .record_side_effect(
                &worker_turn,
                SideEffectEvidence {
                    tool_call_id: "t1".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(10),
                    path: Some("src/parser.rs".into()),
                },
            )
            .unwrap();
        store
            .finish_turn(&worker_turn, None, Some("parser implemented"), None, None)
            .unwrap();
        let finished = store
            .finish_worker_task(&task.task_id, true, Some("parser implemented"))
            .unwrap();
        assert_eq!(finished.status, "completed");
        assert_eq!(finished.changed_files, vec!["src/parser.rs".to_string()]);
        assert_eq!(finished.verification.len(), 1);

        // The outcome reached the lead's timeline.
        let lead_detail = store.get_session("lead-session").unwrap().unwrap();
        let outcome = lead_detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "worker_outcome")
            .expect("worker outcome fact on lead timeline");
        assert!(outcome.reason.contains("parser implemented"));
        assert_eq!(outcome.target_engine_id, "claude");

        // Multi-byte results must excerpt safely (chars, not bytes) — this
        // used to poison the store mutex via a byte-boundary panic.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &run.workers[0].worker_id,
                "写中文文档",
                &[],
            )
            .unwrap();
        let second_task = store
            .claim_for_test(&run.workers[0].worker_id)
            .unwrap()
            .unwrap();
        let long_chinese =
            "完成了所有的中文文档编写工作，包括安装指南、配置说明和常见问题解答".repeat(8);
        let finished = store
            .finish_worker_task(&second_task.task_id, true, Some(&long_chinese))
            .unwrap();
        assert_eq!(finished.status, "completed");

        // Everything survives a database reopen, fully associated.
        drop(store);
        let reopened = HubStore::open(&database).unwrap();
        let run = reopened.team_run_for_lead("lead-session").unwrap().unwrap();
        assert_eq!(run.workers.len(), 1);
        assert_eq!(run.tasks.len(), 2);
        assert_eq!(run.tasks[0].status, "completed");
        assert_eq!(
            run.tasks[0].changed_files,
            vec!["src/parser.rs".to_string()]
        );
        assert_eq!(run.tasks[1].status, "completed");
        assert!(reopened
            .get_session(&run.workers[0].worker_session_id)
            .unwrap()
            .is_some());
    }

    #[test]
    fn worker_queues_are_fifo_editable_and_cancellable_until_claimed() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "queue-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("queue-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        // A second worker on a different engine — the batch's parallel intent.
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
            )
            .unwrap();
        assert_eq!(run.workers.len(), 2);
        let claude_worker = run.workers[0].worker_id.clone();
        let codex_worker = run.workers[1].worker_id.clone();

        let first = store
            .enqueue_worker_task(&run.team_run_id, &claude_worker, "first", &[])
            .unwrap();
        let second = store
            .enqueue_worker_task(&run.team_run_id, &claude_worker, "second", &[])
            .unwrap();
        let other = store
            .enqueue_worker_task(&run.team_run_id, &codex_worker, "codex work", &[])
            .unwrap();

        // Pending tasks are editable and cancellable before consumption.
        let edited = store
            .update_worker_task_scope(&second.task_id, "second, refined")
            .unwrap();
        assert_eq!(edited.scope, "second, refined");
        store.cancel_worker_task(&first.task_id).unwrap();

        // FIFO per worker; cancelled tasks are never claimed; queues are
        // independent between workers.
        let claimed = store.claim_for_test(&claude_worker).unwrap().unwrap();
        assert_eq!(claimed.task_id, edited.task_id);
        assert_eq!(claimed.status, "running");
        assert!(store.claim_for_test(&claude_worker).unwrap().is_none());
        let codex_claimed = store.claim_for_test(&codex_worker).unwrap().unwrap();
        assert_eq!(codex_claimed.task_id, other.task_id);

        // Consumed tasks are immutable and uncancellable.
        assert!(store
            .update_worker_task_scope(&claimed.task_id, "too late")
            .is_err());
        assert!(store.cancel_worker_task(&claimed.task_id).is_err());
        // Enqueue refuses foreign workers.
        assert!(store
            .enqueue_worker_task(&run.team_run_id, "not-a-worker", "x", &[])
            .is_err());
        assert!(!store.has_queued_task(&claude_worker).unwrap());
    }

    #[test]
    fn queued_task_evidence_is_scoped_to_its_own_claim_not_the_queue() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "scope-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("scope-lead", &EngineTarget::for_engine("claude").unwrap())
            .unwrap();
        let worker = run.workers[0].clone();

        // Both tasks are queued up front — the second's created_at predates
        // the first's execution, which used to sweep in the first's evidence.
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "first", &[])
            .unwrap();
        store
            .enqueue_worker_task(&run.team_run_id, &worker.worker_id, "second", &[])
            .unwrap();

        let first = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        let first_turn = store
            .prepare_resumed_turn(&worker.worker_session_id, "first")
            .unwrap();
        store
            .record_side_effect(
                &first_turn,
                SideEffectEvidence {
                    tool_call_id: "t-first".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(5),
                    path: Some("first.rs".into()),
                },
            )
            .unwrap();
        store
            .finish_turn(&first_turn, None, Some("first done"), None, None)
            .unwrap();
        let first = store
            .finish_worker_task(&first.task_id, true, Some("first done"))
            .unwrap();
        assert_eq!(first.changed_files, vec!["first.rs".to_string()]);

        let second = store.claim_for_test(&worker.worker_id).unwrap().unwrap();
        let second_turn = store
            .prepare_resumed_turn(&worker.worker_session_id, "second")
            .unwrap();
        store
            .record_side_effect(
                &second_turn,
                SideEffectEvidence {
                    tool_call_id: "t-second".into(),
                    tool_name: "write_file".into(),
                    succeeded: Some(true),
                    duration_ms: Some(5),
                    path: Some("second.rs".into()),
                },
            )
            .unwrap();
        store
            .finish_turn(&second_turn, None, Some("second done"), None, None)
            .unwrap();
        let second = store
            .finish_worker_task(&second.task_id, true, Some("second done"))
            .unwrap();
        // Only its own work — the first task's file must not appear here.
        assert_eq!(second.changed_files, vec!["second.rs".to_string()]);
        assert_eq!(second.verification.len(), 1);
    }

    #[test]
    fn resource_claims_let_compatible_tasks_run_and_queue_conflicting_ones() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "claims-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        // Three workers sharing ONE workspace: the in-product Alva worker and
        // two external engines all pass the same product-level gate.
        let run = store
            .create_team_run("claims-lead", &EngineTarget::for_engine("alva").unwrap())
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("claude").unwrap(),
            )
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("codex").unwrap(),
            )
            .unwrap();
        let (alva_worker, claude_worker, codex_worker) = (
            run.workers[0].clone(),
            run.workers[1].clone(),
            run.workers[2].clone(),
        );

        let write_lib = ResourceClaim::Path {
            path: "src/lib.rs".into(),
            mode: ClaimMode::Write,
        };
        let write_docs = ResourceClaim::Path {
            path: "docs/guide.md".into(),
            mode: ClaimMode::Write,
        };
        let write_src_tree = ResourceClaim::Path {
            path: "src".into(),
            mode: ClaimMode::Write,
        };

        store
            .enqueue_worker_task(
                &run.team_run_id,
                &alva_worker.worker_id,
                "edit lib",
                std::slice::from_ref(&write_lib),
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &claude_worker.worker_id,
                "edit docs",
                std::slice::from_ref(&write_docs),
            )
            .unwrap();
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &codex_worker.worker_id,
                "refactor the whole src tree",
                std::slice::from_ref(&write_src_tree),
            )
            .unwrap();

        // The Alva worker takes src/lib.rs.
        let alva_task = match store
            .claim_next_worker_task(&alva_worker.worker_id)
            .unwrap()
        {
            TaskClaim::Claimed(task) => *task,
            other => panic!("expected a claim, got {other:?}"),
        };
        // Disjoint docs work runs concurrently — no waiting.
        assert!(matches!(
            store
                .claim_next_worker_task(&claude_worker.worker_id)
                .unwrap(),
            TaskClaim::Claimed(_)
        ));
        // The src-tree refactor overlaps src/lib.rs, so it waits and names
        // both the resource and its holder.
        let blocked = store
            .claim_next_worker_task(&codex_worker.worker_id)
            .unwrap();
        match blocked {
            TaskClaim::Blocked { blocked_by, .. } => {
                assert_eq!(blocked_by.len(), 1);
                assert_eq!(blocked_by[0].resource, "write src");
                assert_eq!(blocked_by[0].holder_worker_id, alva_worker.worker_id);
                assert_eq!(blocked_by[0].holder_task_id, alva_task.task_id);
                store
                    .record_resource_wait(&run.team_run_id, &codex_worker.worker_id, &blocked_by)
                    .unwrap();
            }
            other => panic!("expected a block, got {other:?}"),
        }

        // The wait and its holder are visible on the lead timeline.
        let lead_detail = store.get_session("claims-lead").unwrap().unwrap();
        let wait = lead_detail
            .timeline_events
            .iter()
            .find(|event| event.kind == "resource_wait")
            .expect("resource wait recorded");
        assert!(wait.reason.contains("write src"));
        assert!(wait.reason.contains(&alva_worker.worker_id));

        // Once the holder finishes, the blocked task claims deterministically.
        store
            .finish_worker_task(&alva_task.task_id, true, Some("done"))
            .unwrap();
        let unblocked = store
            .claim_next_worker_task(&codex_worker.worker_id)
            .unwrap();
        assert!(matches!(unblocked, TaskClaim::Claimed(_)));
    }

    #[test]
    fn an_older_waiter_reserves_its_resources_against_a_stream_of_short_tasks() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let lead = store
            .prepare_new_turn(
                "fair-lead",
                workspace.path(),
                "plan",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&lead, None, Some("planned"), None, None)
            .unwrap();
        let run = store
            .create_team_run("fair-lead", &EngineTarget::for_engine("alva").unwrap())
            .unwrap();
        let run = store
            .add_team_worker(
                &run.team_run_id,
                &EngineTarget::for_engine("claude").unwrap(),
            )
            .unwrap();
        let (holder, waiter) = (run.workers[0].clone(), run.workers[1].clone());

        let narrow = ResourceClaim::Path {
            path: "src/a.rs".into(),
            mode: ClaimMode::Write,
        };
        let broad = ResourceClaim::Path {
            path: "src".into(),
            mode: ClaimMode::Write,
        };
        // The holder is already running something narrow.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &holder.worker_id,
                "first narrow edit",
                std::slice::from_ref(&narrow),
            )
            .unwrap();
        let first = match store.claim_next_worker_task(&holder.worker_id).unwrap() {
            TaskClaim::Claimed(task) => *task,
            other => panic!("expected a claim, got {other:?}"),
        };
        // The broad waiter arrives and is blocked.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &waiter.worker_id,
                "refactor the tree",
                std::slice::from_ref(&broad),
            )
            .unwrap();
        assert!(matches!(
            store.claim_next_worker_task(&waiter.worker_id).unwrap(),
            TaskClaim::Blocked { .. }
        ));
        // A newer narrow task queues behind on the holder.
        store
            .enqueue_worker_task(
                &run.team_run_id,
                &holder.worker_id,
                "second narrow edit",
                std::slice::from_ref(&narrow),
            )
            .unwrap();
        store
            .finish_worker_task(&first.task_id, true, Some("done"))
            .unwrap();

        // Without a reservation the newer narrow task would grab src/a.rs
        // again and starve the waiter. The older waiter wins instead.
        assert!(matches!(
            store.claim_next_worker_task(&holder.worker_id).unwrap(),
            TaskClaim::Blocked { .. }
        ));
        assert!(matches!(
            store.claim_next_worker_task(&waiter.worker_id).unwrap(),
            TaskClaim::Claimed(_)
        ));
    }

    #[test]
    fn tasks_in_different_workspaces_never_contend() {
        let root = tempfile::tempdir().unwrap();
        let first_workspace = tempfile::tempdir().unwrap();
        let second_workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        let same_file = ResourceClaim::Path {
            path: "src/lib.rs".into(),
            mode: ClaimMode::Write,
        };
        let mut workers = Vec::new();
        for (index, workspace) in [first_workspace.path(), second_workspace.path()]
            .into_iter()
            .enumerate()
        {
            let lead_id = format!("ws-lead-{index}");
            let lead = store
                .prepare_new_turn(
                    &lead_id,
                    workspace,
                    "plan",
                    &alva,
                    PermissionProfile::WorkspaceWrite,
                )
                .unwrap();
            store
                .finish_turn(&lead, None, Some("planned"), None, None)
                .unwrap();
            let run = store
                .create_team_run(&lead_id, &EngineTarget::for_engine("claude").unwrap())
                .unwrap();
            store
                .enqueue_worker_task(
                    &run.team_run_id,
                    &run.workers[0].worker_id,
                    "edit lib",
                    std::slice::from_ref(&same_file),
                )
                .unwrap();
            workers.push(run.workers[0].worker_id.clone());
        }
        // Identical claims, different workspaces — both run.
        for worker_id in workers {
            assert!(matches!(
                store.claim_next_worker_task(&worker_id).unwrap(),
                TaskClaim::Claimed(_)
            ));
        }
    }

    #[test]
    fn latest_attempt_outcome_reports_terminal_evidence_per_engine() {
        let root = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(root.path().join("alva-hub.db")).unwrap();
        let alva = EngineTarget::for_engine("alva").unwrap();
        assert_eq!(store.latest_attempt_outcome("alva").unwrap(), None);

        let failed = store
            .prepare_new_turn(
                "product-outcome",
                workspace.path(),
                "first",
                &alva,
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&failed, None, None, Some("stream ended early"), None)
            .unwrap();
        assert_eq!(
            store.latest_attempt_outcome("alva").unwrap(),
            Some(("failed".to_string(), Some("stream ended early".to_string())))
        );

        let completed = store
            .prepare_resumed_turn("product-outcome", "second")
            .unwrap();
        store
            .finish_turn(&completed, None, Some("done"), None, None)
            .unwrap();
        assert_eq!(
            store.latest_attempt_outcome("alva").unwrap(),
            Some(("completed".to_string(), None))
        );
        assert_eq!(store.latest_attempt_outcome("claude").unwrap(), None);
    }
}
