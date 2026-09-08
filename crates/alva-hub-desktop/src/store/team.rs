// INPUT:  team_runs/team_workers/worker_tasks rows + ResourceClaim algebra
// OUTPUT: TeamRunDetail/TeamWorker/WorkerTask DTOs, team lifecycle methods on HubStore
// POS:    team run / worker / task queue persistence cluster of the Hub store.

use rusqlite::{params, Connection, OptionalExtension, TransactionBehavior};

use serde::{Deserialize, Serialize};

use crate::claims::ResourceClaim;

use super::branch::FallbackPolicy;
use super::turn::SideEffectEvidence;
use super::{
    insert_engine_branch, insert_switch_timeline, now_unix_ms, persist_fallback_policy,
    query_fallback_policy, query_session, validate_engine_target, EngineTarget, STATUS_COMPLETED,
    STATUS_FAILED, STATUS_RUNNING,
};

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TeamRunDetail {
    pub team_run_id: String,
    /// Explicit, persisted user choice: may this run's workers proceed
    /// without per-request approval? Never set implicitly.
    pub autonomous: bool,
    /// Exactly one lead per run — the HubSession the tech lead works in.
    pub lead_session_id: String,
    pub status: String,
    pub workers: Vec<TeamWorker>,
    pub tasks: Vec<WorkerTask>,
    pub created_at: i64,
}

/// Durable worker visibility. Orthogonal to runtime liveness and to task
/// terminal state, and never a deletion: an archived worker keeps every
/// persistent record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerLifecycle {
    /// Participating in the collaboration surface; may run tasks.
    Active,
    /// Runtime released, identity/history/queue kept; a new task resumes it.
    Hibernated,
    /// Hidden from the active surface; needs an explicit restore.
    Archived,
}

impl WorkerLifecycle {
    pub fn as_str(&self) -> &'static str {
        match self {
            WorkerLifecycle::Active => "active",
            WorkerLifecycle::Hibernated => "hibernated",
            WorkerLifecycle::Archived => "archived",
        }
    }

    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "active" => Ok(WorkerLifecycle::Active),
            "hibernated" => Ok(WorkerLifecycle::Hibernated),
            "archived" => Ok(WorkerLifecycle::Archived),
            other => Err(format!("unknown Hub worker lifecycle '{other}'")),
        }
    }
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct TeamWorker {
    pub worker_id: String,
    /// Durable visibility state — independent of whether an engine is live
    /// (that is runtime) and of whether its tasks finished (that is task
    /// terminal state). Never implies deletion.
    pub lifecycle: WorkerLifecycle,
    /// Managed worktree when this worker runs isolated; None means it works
    /// directly in the shared workspace.
    pub worktree_path: Option<String>,
    pub worktree_branch: Option<String>,
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
    /// The worker is hibernated or archived: its queue is intact but must
    /// not be consumed. Distinct from `Empty` so a queue-depth check can
    /// never mistake "paused" for "drained" and respawn a pump forever.
    Dormant,
    /// Nothing queued.
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

impl super::HubStore {
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
        // A worker inherits the lead's profile and must be able to enforce
        // it; creating a worker is never a route to weaker permissions.
        crate::permissions::enforcement_for(&worker_target.engine_id, lead.permission_profile)
            .map_err(|reason| {
                format!("worker engine cannot honour this session's permissions: {reason}")
            })?;
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
        insert_engine_branch(
            &transaction,
            &worker_branch_id,
            &worker_session_id,
            worker_target,
            "active",
            now,
        )?;
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

    pub fn team_run_for_worker(&self, worker_id: &str) -> Result<Option<TeamRunDetail>, String> {
        let connection = self.lock()?;
        let team_run_id: Option<String> = connection
            .query_row(
                "SELECT team_run_id FROM team_workers WHERE worker_id = ?1",
                [worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub run for worker: {error}"))?;
        match team_run_id {
            Some(team_run_id) => query_team_run(&connection, &team_run_id),
            None => Ok(None),
        }
    }

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

    /// to an existing team run — the batch's parallel intent.
    pub fn add_team_worker(
        &self,
        team_run_id: &str,
        worker_target: &EngineTarget,
        worktree: Option<&crate::worktree::WorktreeHandle>,
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
        crate::permissions::enforcement_for(&worker_target.engine_id, lead.permission_profile)
            .map_err(|reason| {
                format!("worker engine cannot honour this session's permissions: {reason}")
            })?;
        let run_status: String = transaction
            .query_row(
                "SELECT status FROM team_runs WHERE team_run_id = ?1",
                [team_run_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("query Hub team run status: {error}"))?;
        if run_status != "active" {
            return Err(format!(
                "team run '{team_run_id}' has ended; start a new run to add workers"
            ));
        }
        transaction
            .execute(
                "INSERT INTO hub_sessions
                       (product_session_id, workspace, active_branch_id, status,
                        permission_profile, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                params![
                    worker_session_id,
                    // An isolated worker works in its managed worktree; a
                    // shared worker in the lead's workspace.
                    worktree
                        .map(|handle| handle.path.clone())
                        .unwrap_or_else(|| lead.workspace.clone()),
                    worker_branch_id,
                    STATUS_COMPLETED,
                    lead.permission_profile.as_str(),
                    now
                ],
            )
            .map_err(|error| format!("insert Hub worker session: {error}"))?;
        insert_engine_branch(
            &transaction,
            &worker_branch_id,
            &worker_session_id,
            worker_target,
            "active",
            now,
        )?;
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
                "INSERT INTO team_workers
                       (worker_id, team_run_id, worker_session_id, created_at,
                        worktree_path, worktree_branch)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    worker_id,
                    team_run_id,
                    worker_session_id,
                    now,
                    worktree.map(|handle| handle.path.clone()),
                    worktree.map(|handle| handle.branch.clone())
                ],
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
                "SELECT lifecycle FROM team_workers
                      WHERE team_run_id = ?1 AND worker_id = ?2",
                params![team_run_id, worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub team worker: {error}"))?;
        let Some(lifecycle) = belongs else {
            return Err(format!(
                "worker '{worker_id}' does not belong to team run '{team_run_id}'"
            ));
        };
        match WorkerLifecycle::parse(&lifecycle)? {
            // New work wakes a hibernated worker.
            WorkerLifecycle::Hibernated => {
                connection
                    .execute(
                        "UPDATE team_workers SET lifecycle = ?2 WHERE worker_id = ?1",
                        params![worker_id, WorkerLifecycle::Active.as_str()],
                    )
                    .map_err(|error| format!("resume Hub worker: {error}"))?;
            }
            // An archived worker must be restored deliberately.
            WorkerLifecycle::Archived => {
                return Err(
                    "this worker is archived; restore it before queueing new work".to_string(),
                )
            }
            WorkerLifecycle::Active => {}
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
        use super::orchestration::{ClaimDecision, ClaimInputs, HeadTask, RunningHolder};

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
        let lifecycle: String = transaction
            .query_row(
                "SELECT lifecycle FROM team_workers WHERE worker_id = ?1",
                [worker_id],
                |row| row.get(0),
            )
            .map_err(|error| format!("query Hub worker lifecycle: {error}"))?;
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

        // Fairness candidates: older head-of-queue waiters in this workspace.
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

        // ── 决策（纯策略，见 orchestration::decide_claim）──────────────
        let to_holder = |(task_id, worker_id, claims_json): (String, String, String)| {
            Ok::<RunningHolder, String>(RunningHolder {
                task_id,
                worker_id,
                claims: serde_json::from_str(&claims_json)
                    .map_err(|error| format!("decode Hub holder claims: {error}"))?,
            })
        };
        let holders = holders
            .into_iter()
            .map(to_holder)
            .collect::<Result<Vec<_>, _>>()?;
        let waiters = waiters
            .into_iter()
            .map(to_holder)
            .collect::<Result<Vec<_>, _>>()?;
        let inputs = ClaimInputs {
            lifecycle: WorkerLifecycle::parse(&lifecycle)?,
            head: Some(HeadTask {
                task_id: task_id.clone(),
            }),
            wanted,
            holders,
            older_waiters: waiters,
        };
        // ── 记账：按决策落库 ────────────────────────────────────────────
        match super::orchestration::decide_claim(&inputs) {
            ClaimDecision::Empty => Ok(TaskClaim::Empty),
            ClaimDecision::Dormant => Ok(TaskClaim::Dormant),
            ClaimDecision::Blocked {
                task_id,
                blocked_by,
            } => Ok(TaskClaim::Blocked {
                task_id,
                blocked_by,
            }),
            ClaimDecision::Grant { task_id } => {
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
        }
    }

    /// Detach an isolated worker's worktree identity after the worktree has
    /// been discarded; the durable session and its history stay.
    pub fn clear_worker_worktree(
        &self,
        worker_id: &str,
        shared_workspace: &str,
    ) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub worktree clear transaction: {error}"))?;
        // Point the durable session back at the shared workspace: leaving it
        // on the deleted worktree would make every later run fail.
        transaction
            .execute(
                "UPDATE hub_sessions SET workspace = ?2, updated_at = ?3
                      WHERE product_session_id = (
                        SELECT worker_session_id FROM team_workers WHERE worker_id = ?1
                      )",
                params![worker_id, shared_workspace, now_unix_ms()],
            )
            .map_err(|error| format!("repoint Hub worker workspace: {error}"))?;
        transaction
            .execute(
                "UPDATE team_workers SET worktree_path = NULL, worktree_branch = NULL
                      WHERE worker_id = ?1",
                [worker_id],
            )
            .map_err(|error| format!("clear Hub worker worktree: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub worktree clear: {error}"))?;
        Ok(())
    }

    /// Attach a managed worktree to an already-registered worker.
    pub fn attach_worker_worktree(
        &self,
        worker_id: &str,
        worktree: &crate::worktree::WorktreeHandle,
    ) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub worktree attach transaction: {error}"))?;
        transaction
            .execute(
                "UPDATE hub_sessions SET workspace = ?2, updated_at = ?3
                      WHERE product_session_id = (
                        SELECT worker_session_id FROM team_workers WHERE worker_id = ?1
                      )",
                params![worker_id, worktree.path, now_unix_ms()],
            )
            .map_err(|error| format!("point Hub worker at worktree: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE team_workers SET worktree_path = ?2, worktree_branch = ?3
                      WHERE worker_id = ?1",
                params![worker_id, worktree.path, worktree.branch],
            )
            .map_err(|error| format!("attach Hub worker worktree: {error}"))?;
        if changed != 1 {
            return Err(format!("worker '{worker_id}' was not found"));
        }
        transaction
            .commit()
            .map_err(|error| format!("commit Hub worktree attach: {error}"))?;
        Ok(())
    }

    /// Human label for a session inside a team run ("worker on codex" /
    /// "lead"), or None for a session that is not part of one.
    pub fn team_worker_label(&self, product_session_id: &str) -> Result<Option<String>, String> {
        if product_session_id.is_empty() {
            return Ok(None);
        }
        let connection = self.lock()?;
        let worker: Option<String> = connection
            .query_row(
                "SELECT eb.engine_id FROM team_workers tw
                       JOIN hub_sessions hs ON hs.product_session_id = tw.worker_session_id
                       JOIN engine_branches eb ON eb.branch_id = hs.active_branch_id
                      WHERE tw.worker_session_id = ?1",
                [product_session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub worker label: {error}"))?;
        if let Some(engine) = worker {
            return Ok(Some(format!("worker on {engine}")));
        }
        let lead: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM team_runs WHERE lead_session_id = ?1 LIMIT 1",
                [product_session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub lead label: {error}"))?;
        Ok(lead.map(|_| "team lead".to_string()))
    }

    /// Collaboration scope of a session: its team run id when it belongs to
    /// one, otherwise the session itself. Approvals only merge inside one.
    pub fn collaboration_scope(&self, product_session_id: &str) -> Result<String, String> {
        if product_session_id.is_empty() {
            return Ok(String::new());
        }
        let connection = self.lock()?;
        let run: Option<String> = connection
            .query_row(
                "SELECT team_run_id FROM team_workers WHERE worker_session_id = ?1
                     UNION ALL
                     SELECT team_run_id FROM team_runs WHERE lead_session_id = ?1
                     LIMIT 1",
                [product_session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub collaboration scope: {error}"))?;
        Ok(run.unwrap_or_else(|| product_session_id.to_string()))
    }

    /// Is this session part of a run the user explicitly made autonomous?
    pub fn is_autonomous_scope(&self, product_session_id: &str) -> Result<bool, String> {
        if product_session_id.is_empty() {
            return Ok(false);
        }
        let connection = self.lock()?;
        let autonomous: Option<bool> = connection
            .query_row(
                "SELECT tr.autonomous FROM team_runs tr
                      WHERE tr.lead_session_id = ?1
                         OR tr.team_run_id = (
                           SELECT team_run_id FROM team_workers WHERE worker_session_id = ?1
                         )
                      LIMIT 1",
                [product_session_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub autonomy: {error}"))?;
        Ok(autonomous.unwrap_or(false))
    }

    /// Audit trail for an approval decision: which sessions it covered,
    /// what was asked, and what the user chose.
    pub fn record_permission_decision(
        &self,
        product_session_id: &str,
        summary: &str,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub approval audit transaction: {error}"))?;
        let session = query_session(&transaction, product_session_id)?
            .ok_or_else(|| format!("Hub session '{product_session_id}' was not found"))?;
        insert_switch_timeline(
            &transaction,
            product_session_id,
            "permission_decision",
            &session.active_branch_id,
            None,
            &session.engine_id,
            &session.engine_id,
            &crate::redact::redact_text(summary),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub approval audit: {error}"))?;
        Ok(())
    }

    /// Turn autonomous execution on or off for a run. Independent of the
    /// PermissionProfile: autonomy decides whether RAISED requests still
    /// need a human, never what the engine is allowed to attempt.
    pub fn set_team_autonomy(
        &self,
        team_run_id: &str,
        autonomous: bool,
    ) -> Result<TeamRunDetail, String> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE team_runs SET autonomous = ?2 WHERE team_run_id = ?1",
                params![team_run_id, autonomous],
            )
            .map_err(|error| format!("persist Hub team autonomy: {error}"))?;
        if changed != 1 {
            return Err(format!("team run '{team_run_id}' was not found"));
        }
        drop(connection);
        self.get_team_run(team_run_id)?
            .ok_or_else(|| "team run vanished after autonomy change".to_string())
    }

    /// Move a worker between lifecycle states. Hibernating and archiving
    /// never touch identity, history, or queue — only visibility and
    /// whether the queue may be consumed.
    pub fn set_worker_lifecycle(
        &self,
        worker_id: &str,
        lifecycle: WorkerLifecycle,
    ) -> Result<TeamWorker, String> {
        let connection = self.lock()?;
        let changed = connection
            .execute(
                "UPDATE team_workers SET lifecycle = ?2 WHERE worker_id = ?1",
                params![worker_id, lifecycle.as_str()],
            )
            .map_err(|error| format!("persist Hub worker lifecycle: {error}"))?;
        if changed != 1 {
            return Err(format!("worker '{worker_id}' was not found"));
        }
        drop(connection);
        self.team_worker(worker_id)?
            .ok_or_else(|| "worker vanished after lifecycle change".to_string())
    }

    pub fn team_worker(&self, worker_id: &str) -> Result<Option<TeamWorker>, String> {
        Ok(self.team_run_for_worker(worker_id)?.and_then(|run| {
            run.workers
                .into_iter()
                .find(|worker| worker.worker_id == worker_id)
        }))
    }

    /// End a TeamRun: workers are ARCHIVED, never deleted, so every
    /// persistent record survives.
    pub fn complete_team_run(&self, team_run_id: &str) -> Result<TeamRunDetail, String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub team completion transaction: {error}"))?;
        let changed = transaction
            .execute(
                "UPDATE team_runs SET status = 'completed'
                      WHERE team_run_id = ?1 AND status = 'active'",
                [team_run_id],
            )
            .map_err(|error| format!("complete Hub team run: {error}"))?;
        if changed != 1 {
            // Re-completing would silently re-archive workers the user has
            // since restored.
            return Err(format!(
                "team run '{team_run_id}' was not found or has already ended"
            ));
        }
        transaction
            .execute(
                "UPDATE team_workers SET lifecycle = ?2 WHERE team_run_id = ?1",
                params![team_run_id, WorkerLifecycle::Archived.as_str()],
            )
            .map_err(|error| format!("archive Hub team workers: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub team completion: {error}"))?;
        drop(connection);
        self.get_team_run(team_run_id)?
            .ok_or_else(|| "team run vanished after completion".to_string())
    }

    /// Permanently remove a worker and its records. Deliberately separate
    /// from archiving, and only reachable for an archived worker — so
    /// "archived" can never be mistaken for "deleted".
    pub fn delete_worker(&self, worker_id: &str) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub worker delete transaction: {error}"))?;
        let lifecycle: Option<String> = transaction
            .query_row(
                "SELECT lifecycle FROM team_workers WHERE worker_id = ?1",
                [worker_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub worker lifecycle: {error}"))?;
        let lifecycle = lifecycle.ok_or_else(|| format!("worker '{worker_id}' was not found"))?;
        if WorkerLifecycle::parse(&lifecycle)? != WorkerLifecycle::Archived {
            return Err("only an archived worker can be deleted".to_string());
        }
        transaction
            .execute("DELETE FROM worker_tasks WHERE worker_id = ?1", [worker_id])
            .map_err(|error| format!("delete Hub worker tasks: {error}"))?;
        transaction
            .execute("DELETE FROM team_workers WHERE worker_id = ?1", [worker_id])
            .map_err(|error| format!("delete Hub team worker: {error}"))?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub worker delete: {error}"))?;
        Ok(())
    }

    /// Is this worker currently executing something?
    pub fn worker_has_active_task(&self, worker_id: &str) -> Result<bool, String> {
        let connection = self.lock()?;
        let active: Option<i64> = connection
            .query_row(
                "SELECT 1 FROM worker_tasks
                      WHERE worker_id = ?1 AND status IN (?2, 'queued') LIMIT 1",
                params![worker_id, STATUS_RUNNING],
                |row| row.get(0),
            )
            .optional()
            .map_err(|error| format!("query Hub worker activity: {error}"))?;
        Ok(active.is_some())
    }

    /// Record an adoption decision on the lead timeline.
    pub fn record_adoption(
        &self,
        team_run_id: &str,
        worker_id: &str,
        adopted: bool,
        summary: &str,
    ) -> Result<(), String> {
        let now = now_unix_ms();
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(|error| format!("begin Hub adoption transaction: {error}"))?;
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
        insert_switch_timeline(
            &transaction,
            &lead_session_id,
            if adopted {
                "worktree_adopted"
            } else {
                "worktree_discarded"
            },
            &lead.active_branch_id,
            None,
            &lead.engine_id,
            &worker_engine,
            &crate::redact::redact_text(summary),
            now,
        )?;
        transaction
            .commit()
            .map_err(|error| format!("commit Hub adoption: {error}"))?;
        Ok(())
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
    pub fn active_workers_with_queued_tasks(&self) -> Result<Vec<String>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT wt.worker_id FROM worker_tasks wt
                       JOIN team_workers tw ON tw.worker_id = wt.worker_id
                      WHERE wt.status = 'queued' AND tw.lifecycle = 'active'",
            )
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
}

fn query_team_run(
    connection: &Connection,
    team_run_id: &str,
) -> Result<Option<TeamRunDetail>, String> {
    let run = connection
        .query_row(
            "SELECT team_run_id, lead_session_id, status, created_at, autonomous
               FROM team_runs WHERE team_run_id = ?1",
            [team_run_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, i64>(3)?,
                    row.get::<_, bool>(4)?,
                ))
            },
        )
        .optional()
        .map_err(|error| format!("query Hub team run: {error}"))?;
    let Some((team_run_id, lead_session_id, status, created_at, autonomous)) = run else {
        return Ok(None);
    };
    let mut worker_statement = connection
        .prepare(
            "SELECT tw.worker_id, tw.worker_session_id, eb.engine_id, tw.created_at,
                    tw.worktree_path, tw.worktree_branch, tw.lifecycle
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
                worktree_path: row.get(4)?,
                worktree_branch: row.get(5)?,
                lifecycle: WorkerLifecycle::parse(&row.get::<_, String>(6)?).map_err(|error| {
                    rusqlite::Error::FromSqlConversionFailure(
                        6,
                        rusqlite::types::Type::Text,
                        error.into(),
                    )
                })?,
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
        autonomous,
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
