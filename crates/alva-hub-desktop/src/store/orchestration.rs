// INPUT:  crate::claims::conflicting_claims, crate::store::team::{BlockedResource, WorkerLifecycle}, ResourceClaim
// OUTPUT: ClaimInputs, ClaimDecision, decide_claim
// POS:    编排**策略**（ticket 18）：谁可以领取任务、被什么阻塞、休眠/归档
//         领取者如何处理、老任务如何预留资源防饿死。纯函数——没有 SQL、
//         没有事务、没有连接；输入是普通数据，输出是决策。持久化
//         （读写 worker_tasks / team_workers）留在 team.rs 的记账层。

use crate::claims::conflicting_claims;

use super::team::{BlockedResource, WorkerLifecycle};

/// Everything the claim decision needs, gathered by the persistence layer.
pub struct ClaimInputs {
    /// The claiming worker's lifecycle (only Active workers consume work).
    pub lifecycle: WorkerLifecycle,
    /// The head-of-queue task, if any: (task_id, created_at, rowid).
    pub head: Option<HeadTask>,
    /// Claims the candidate task wants to hold.
    pub wanted: Vec<crate::claims::ResourceClaim>,
    /// Everything currently RUNNING in the same workspace:
    /// (task_id, worker_id, held claims). Holders never wait.
    pub holders: Vec<RunningHolder>,
    /// Other queued tasks in the same workspace whose head-of-queue is older
    /// than the candidate: (task_id, worker_id, claims). Their reservation
    /// prevents a stream of short conflicting tasks from starving them.
    pub older_waiters: Vec<RunningHolder>,
}

#[derive(Debug, Clone)]
pub struct HeadTask {
    pub task_id: String,
}

/// (task_id, worker_id, claims) — a running holder or an older waiter.
pub struct RunningHolder {
    pub task_id: String,
    pub worker_id: String,
    pub claims: Vec<crate::claims::ResourceClaim>,
}

/// What the queue pump should do with the candidate task.
pub enum ClaimDecision {
    /// The worker has no queued work.
    Empty,
    /// The worker keeps its queue but consumes nothing (hibernated/archived).
    Dormant,
    /// The task conflicts with holders and/or reserved older waiters.
    Blocked {
        task_id: String,
        blocked_by: Vec<BlockedResource>,
    },
    /// Grant the claim — the persistence layer flips the task to running.
    Grant { task_id: String },
}

/// The claim decision, pure: FIFO head first, lifecycle gate, conflict
/// detection against holders, fairness reservation against older waiters.
pub fn decide_claim(inputs: &ClaimInputs) -> ClaimDecision {
    let Some(head) = &inputs.head else {
        return ClaimDecision::Empty;
    };
    // Hibernated and archived workers keep their queue but consume nothing:
    // that is exactly what "released the runtime" means.
    if inputs.lifecycle != WorkerLifecycle::Active {
        return ClaimDecision::Dormant;
    }

    let mut blocked_by: Vec<BlockedResource> = Vec::new();
    // Everything currently running in this workspace holds its claims.
    for holder in &inputs.holders {
        for conflict in conflicting_claims(&inputs.wanted, &holder.claims) {
            blocked_by.push(BlockedResource {
                resource: conflict.describe(),
                holder_task_id: holder.task_id.clone(),
                holder_worker_id: holder.worker_id.clone(),
            });
        }
    }
    // Fairness: an older head-of-queue waiter in this workspace reserves its
    // resources, so a stream of short conflicting tasks cannot starve it.
    // Holders never wait, so the globally oldest waiter is always eventually
    // grantable — reservations cannot deadlock.
    if blocked_by.is_empty() {
        for waiter in &inputs.older_waiters {
            for conflict in conflicting_claims(&inputs.wanted, &waiter.claims) {
                blocked_by.push(BlockedResource {
                    resource: conflict.describe(),
                    holder_task_id: waiter.task_id.clone(),
                    holder_worker_id: waiter.worker_id.clone(),
                });
            }
        }
    }
    if !blocked_by.is_empty() {
        return ClaimDecision::Blocked {
            task_id: head.task_id.clone(),
            blocked_by,
        };
    }
    ClaimDecision::Grant {
        task_id: head.task_id.clone(),
    }
}
