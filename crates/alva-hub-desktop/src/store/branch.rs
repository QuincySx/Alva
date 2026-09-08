// INPUT:  engine_branches/fallback_policies/engine_switches rows
// OUTPUT: EngineTarget/FallbackPolicy/PendingSwitch DTOs, engine target/policy/switch methods on HubStore
// POS:    engine target, fallback policy, and switch persistence cluster of the Hub store.

use rusqlite::{params, OptionalExtension, Transaction, TransactionBehavior};

use serde::{Deserialize, Serialize};

use super::handoff::{build_handoff_package, persist_handoff_package, HandoffKind};
use super::turn::HubSessionSummary;
use super::{
    insert_engine_branch, insert_switch_timeline, now_unix_ms, persist_fallback_policy,
    query_branch_target, query_fallback_policy, query_pending_switch, query_session,
    update_engine_branch_options, validate_engine_target, validate_fallback_targets,
    STATUS_RUNNING,
};

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
    #[serde(default)]
    pub reasoning_effort: Option<String>,
    #[serde(default)]
    pub fast_mode: bool,
}

pub(crate) fn default_account_source() -> String {
    "Unconfigured account".to_string()
}

pub(crate) fn default_billing_route() -> String {
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
            "pi" => (
                "engine-default",
                "pi-unconfigured",
                "Third-party provider required",
                "Provider API usage",
            ),
            other => {
                // Ticket 17: a registered (fifth+) engine gets generic routing
                // defaults; the registry is the authority on what exists.
                if crate::engine_registry::blueprint(other).is_none() {
                    return Err(format!(
                        "unsupported Hub engine '{other}'; expected alva, claude, codex, or pi"
                    ));
                }
                (
                    "engine-default",
                    other,
                    "Registered engine account",
                    "Provider API usage",
                )
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
            reasoning_effort: None,
            fast_mode: false,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FallbackPolicy {
    pub targets: Vec<EngineTarget>,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct StoredBranch {
    pub branch_id: String,
    pub engine_id: String,
    pub model_id: String,
    pub provider_id: String,
    pub account_source: String,
    pub billing_route: String,
    pub automatic_use_authorized: bool,
    pub required_capabilities: Vec<String>,
    pub reasoning_effort: Option<String>,
    pub fast_mode: bool,
    pub engine_session_id: Option<String>,
    pub watermark_turn_rowid: Option<i64>,
    pub status: String,
    pub created_at: i64,
    pub updated_at: i64,
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

impl super::HubStore {
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
            let active_target = query_branch_target(&transaction, &session.active_branch_id)?;
            if active_target.reasoning_effort != target.reasoning_effort
                || active_target.fast_mode != target.fast_mode
            {
                if session.status == STATUS_RUNNING {
                    return Err(
                        "wait for the current attempt to finish before changing model options"
                            .into(),
                    );
                }
                update_engine_branch_options(&transaction, &session.active_branch_id, target, now)?;
            }
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
            }
            transaction
                .commit()
                .map_err(|error| format!("commit Hub model option update: {error}"))?;
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

    pub(crate) fn apply_ready_pending_switches(&self) -> Result<(), String> {
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
}

fn upsert_queued_switch(
    transaction: &Transaction<'_>,
    product_session_id: &str,
    source_branch_id: &str,
    target: &EngineTarget,
    reason: &str,
    now: i64,
) -> Result<String, String> {
    let target_branch_id = transaction
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
        .map_err(|error| format!("find queued-switch Hub engine branch: {error}"))?
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    let branch_exists = transaction
        .query_row(
            "SELECT 1 FROM engine_branches WHERE branch_id = ?1",
            [&target_branch_id],
            |_| Ok(()),
        )
        .optional()
        .map_err(|error| format!("check queued-switch Hub engine branch: {error}"))?
        .is_some();
    if !branch_exists {
        insert_engine_branch(
            transaction,
            &target_branch_id,
            product_session_id,
            target,
            "parked",
            now,
        )?;
    } else {
        update_engine_branch_options(transaction, &target_branch_id, target, now)?;
    }

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
                    SET source_branch_id = ?2, target_branch_id = ?3,
                        target_engine_id = ?4, target_model_id = ?5,
                        target_provider_id = ?6, reason = ?7, requested_at = ?8
                  WHERE switch_id = ?1",
                params![
                    switch_id,
                    source_branch_id,
                    target_branch_id,
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
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'queued', NULL, ?9, NULL)",
            params![
                switch_id,
                product_session_id,
                source_branch_id,
                target_branch_id,
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
        insert_engine_branch(
            transaction,
            &target_branch_id,
            product_session_id,
            target,
            "parked",
            now,
        )?;
    } else {
        update_engine_branch_options(transaction, &target_branch_id, target, now)?;
    }
    let branch_target = query_branch_target(transaction, &target_branch_id)?;

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

    let source = query_branch_target(transaction, &source_session.active_branch_id)?;
    let package = build_handoff_package(
        transaction,
        product_session_id,
        &source_session.active_branch_id,
        &target_branch_id,
        source,
        branch_target.clone(),
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
        &branch_target.engine_id,
        reason,
        now,
    )?;

    Ok(SwitchOutcome {
        disposition: SwitchDisposition::Applied,
        active_branch_id: target_branch_id,
        active_engine_id: branch_target.engine_id.clone(),
        target_engine_id: branch_target.engine_id,
    })
}
