// INPUT: Private SQLite automation rows and session-pinned engine contracts
// OUTPUT: Paused-first interval schedules, optimistic CRUD, atomic turn claims and terminal settlement
// POS: L7 local scheduling state; never a kernel capability or an OS background service

use super::{
    now_unix_ms, query_branch_target, query_pending_switch, query_session, EngineTarget,
    HubSessionSummary, HubStore,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct Automation {
    pub automation_id: String,
    pub product_session_id: String,
    pub title: String,
    pub prompt: String,
    pub interval_minutes: i64,
    pub enabled: bool,
    pub revision: i64,
    pub branch_id: String,
    pub target: EngineTarget,
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    pub last_status: String,
    pub last_error: Option<String>,
    pub running_turn_id: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum AutomationAction {
    Create {
        product_session_id: String,
        title: String,
        prompt: String,
        interval_minutes: i64,
    },
    Update {
        automation_id: String,
        revision: i64,
        title: String,
        prompt: String,
        interval_minutes: i64,
    },
    SetEnabled {
        automation_id: String,
        revision: i64,
        enabled: bool,
    },
    Delete {
        automation_id: String,
        revision: i64,
    },
}

const COLUMNS: &str = "automation_id, product_session_id, title, prompt, interval_minutes, enabled, revision, branch_id, target_json, next_run_at, last_run_at, last_status, last_error, running_turn_id";

fn decode(row: &rusqlite::Row<'_>) -> rusqlite::Result<Automation> {
    let json: String = row.get(8)?;
    let target = serde_json::from_str(&json).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, Box::new(error))
    })?;
    Ok(Automation {
        automation_id: row.get(0)?,
        product_session_id: row.get(1)?,
        title: row.get(2)?,
        prompt: row.get(3)?,
        interval_minutes: row.get(4)?,
        enabled: row.get(5)?,
        revision: row.get(6)?,
        branch_id: row.get(7)?,
        target,
        next_run_at: row.get(9)?,
        last_run_at: row.get(10)?,
        last_status: row.get(11)?,
        last_error: row.get(12)?,
        running_turn_id: row.get(13)?,
    })
}

fn query(connection: &Connection, id: &str) -> Result<Automation, String> {
    connection
        .query_row(
            &format!("SELECT {COLUMNS} FROM automations WHERE automation_id = ?1"),
            [id],
            decode,
        )
        .optional()
        .map_err(|error| format!("read automation: {error}"))?
        .ok_or("Automation not found".into())
}

fn validate(title: &str, prompt: &str, minutes: i64) -> Result<(), String> {
    if title.trim().is_empty() || title.chars().count() > 120 {
        return Err("Title must contain 1–120 characters".into());
    }
    if prompt.trim().is_empty() || prompt.len() > 16 * 1024 {
        return Err("Prompt must contain 1–16384 UTF-8 bytes".into());
    }
    if !(5..=10080).contains(&minutes) {
        return Err("Interval must be 5–10080 minutes".into());
    }
    Ok(())
}

fn ordinary_session(connection: &Connection, session: &HubSessionSummary) -> Result<(), String> {
    let team: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM team_runs WHERE lead_session_id = ?1)
             OR EXISTS(SELECT 1 FROM team_workers WHERE worker_session_id = ?1)",
            [&session.product_session_id],
            |row| row.get(0),
        )
        .map_err(|error| format!("check automation session scope: {error}"))?;
    if team {
        return Err("Automations require an ordinary session, not a team lead or worker".into());
    }
    if query_pending_switch(connection, &session.product_session_id)?.is_some() {
        return Err("Finish the pending engine switch before configuring an automation".into());
    }
    Ok(())
}

impl HubStore {
    pub fn list_automations(&self) -> Result<Vec<Automation>, String> {
        let connection = self.lock()?;
        let mut statement = connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM automations ORDER BY rowid DESC"
            ))
            .map_err(|error| format!("list automations: {error}"))?;
        let rows = statement
            .query_map([], decode)
            .map_err(|error| format!("query automations: {error}"))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| format!("decode automations: {error}"))
    }

    pub fn get_automation(&self, id: &str) -> Result<Automation, String> {
        let connection = self.lock()?;
        query(&connection, id)
    }

    pub fn automation_action(
        &self,
        action: AutomationAction,
    ) -> Result<Option<Automation>, String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .map_err(|error| format!("begin automation edit: {error}"))?;
        let now = now_unix_ms();
        let id = match action {
            AutomationAction::Create {
                product_session_id,
                title,
                prompt,
                interval_minutes,
            } => {
                validate(&title, &prompt, interval_minutes)?;
                let count: i64 = transaction
                    .query_row("SELECT COUNT(*) FROM automations", [], |row| row.get(0))
                    .map_err(|error| error.to_string())?;
                if count >= 100 {
                    return Err("At most 100 automations are supported".into());
                }
                let session =
                    query_session(&transaction, &product_session_id)?.ok_or("Session not found")?;
                ordinary_session(&transaction, &session)?;
                let target = query_branch_target(&transaction, &session.active_branch_id)?;
                let id = uuid::Uuid::new_v4().to_string();
                transaction.execute(
                    "INSERT INTO automations (automation_id, product_session_id, title, prompt, interval_minutes, branch_id, target_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                    params![id, product_session_id, title.trim(), prompt, interval_minutes, session.active_branch_id, serde_json::to_string(&target).map_err(|error| error.to_string())?]
                ).map_err(|error| format!("create paused automation: {error}"))?;
                id
            }
            action => {
                let (id, revision) = match &action {
                    AutomationAction::Update {
                        automation_id,
                        revision,
                        ..
                    }
                    | AutomationAction::SetEnabled {
                        automation_id,
                        revision,
                        ..
                    }
                    | AutomationAction::Delete {
                        automation_id,
                        revision,
                    } => (automation_id, *revision),
                    _ => unreachable!(),
                };
                let current = query(&transaction, id)?;
                if current.revision != revision {
                    return Err("Automation changed; refresh and review again".into());
                }
                match action {
                    AutomationAction::Update {
                        title,
                        prompt,
                        interval_minutes,
                        ..
                    } => {
                        validate(&title, &prompt, interval_minutes)?;
                        if current.running_turn_id.is_some() {
                            return Err("Wait for the current run to finish before editing".into());
                        }
                        let session = query_session(&transaction, &current.product_session_id)?
                            .ok_or("Session not found")?;
                        ordinary_session(&transaction, &session)?;
                        let target = query_branch_target(&transaction, &session.active_branch_id)?;
                        transaction.execute(
                            "UPDATE automations SET title = ?2, prompt = ?3, interval_minutes = ?4, branch_id = ?5, target_json = ?6, enabled = 0, next_run_at = NULL, revision = revision + 1 WHERE automation_id = ?1",
                            params![current.automation_id, title.trim(), prompt, interval_minutes, session.active_branch_id, serde_json::to_string(&target).map_err(|error| error.to_string())?]
                        ).map_err(|error| format!("edit paused automation: {error}"))?;
                    }
                    AutomationAction::SetEnabled { enabled, .. } => {
                        if enabled {
                            if matches!(
                                current.target.provider_id.as_str(),
                                "workspace-config" | "pi-unconfigured"
                            ) {
                                return Err("Select an explicit configured model/account route before enabling an automation".into());
                            }
                            if current.running_turn_id.is_some() {
                                return Err(
                                    "Wait for the current run to finish before enabling".into()
                                );
                            }
                            let session = query_session(&transaction, &current.product_session_id)?
                                .ok_or("Session not found")?;
                            ordinary_session(&transaction, &session)?;
                            if current.branch_id != session.active_branch_id
                                || current.target
                                    != query_branch_target(&transaction, &session.active_branch_id)?
                            {
                                return Err("Session route changed; edit and review the new account before enabling".into());
                            }
                        }
                        transaction.execute(
                            "UPDATE automations SET enabled = ?2, next_run_at = ?3, revision = revision + 1 WHERE automation_id = ?1",
                            params![current.automation_id, enabled, enabled.then_some(now.saturating_add(current.interval_minutes * 60000))]
                        ).map_err(|error| format!("change automation state: {error}"))?;
                    }
                    AutomationAction::Delete { .. } => {
                        if current.running_turn_id.is_some() {
                            return Err("Wait for the current run to finish before deleting".into());
                        }
                        transaction
                            .execute(
                                "DELETE FROM automations WHERE automation_id = ?1",
                                [&current.automation_id],
                            )
                            .map_err(|error| format!("delete automation configuration: {error}"))?;
                        transaction.commit().map_err(|error| error.to_string())?;
                        return Ok(None);
                    }
                    _ => unreachable!(),
                }
                current.automation_id
            }
        };
        let next = query(&transaction, &id)?;
        transaction
            .commit()
            .map_err(|error| format!("commit automation edit: {error}"))?;
        Ok(Some(next))
    }

    /// Read-only candidates; the execution seam atomically rechecks and claims
    /// the schedule together with the Turn. Reading a due id grants nothing.
    pub fn due_automation_ids(&self) -> Result<Vec<String>, String> {
        let connection = self.lock()?;
        let mut statement = connection.prepare("SELECT automation_id FROM automations WHERE enabled = 1 AND running_turn_id IS NULL AND next_run_at <= ?1 ORDER BY next_run_at, rowid LIMIT 100").map_err(|error| error.to_string())?;
        let rows = statement
            .query_map([now_unix_ms()], |row| row.get(0))
            .map_err(|error| error.to_string())?;
        rows.collect::<rusqlite::Result<Vec<_>>>()
            .map_err(|error| error.to_string())
    }

    pub fn finish_automation(&self, id: &str, error: Option<&str>) -> Result<(), String> {
        let mut connection = self.lock()?;
        let transaction = connection
            .transaction()
            .map_err(|error| error.to_string())?;
        let current = query(&transaction, id)?;
        let Some(turn) = current.running_turn_id else {
            return Ok(());
        };
        let (status, stored_error): (String, Option<String>) = transaction
            .query_row(
                "SELECT status, error_message FROM turns WHERE turn_id = ?1",
                [&turn],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(|error| error.to_string())?;
        let cancelled: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM engine_attempts ea JOIN timeline_events te ON te.product_session_id = ea.product_session_id WHERE ea.turn_id = ?1 AND te.kind = 'attempt_cancelled' AND te.created_at >= ea.started_at AND te.created_at <= COALESCE(ea.completed_at, ?2))",
            params![turn, now_unix_ms()], |row| row.get(0),
        ).map_err(|error| error.to_string())?;
        let completed = status == "completed" && error.is_none() && !cancelled;
        let status = if completed {
            "completed"
        } else if status == "interrupted" || cancelled {
            "interrupted"
        } else {
            "failed"
        };
        let reason = if completed {
            None
        } else {
            Some(crate::redact_text(&super::bounded_text(if cancelled {
                "Stopped by user; review and enable again"
            } else {
                error
                    .or(stored_error.as_deref())
                    .unwrap_or("Execution did not complete; review and enable again")
            })))
        };
        transaction.execute(
            "UPDATE automations SET running_turn_id = NULL, enabled = ?2, next_run_at = ?3, last_status = ?4, last_error = ?5, revision = revision + 1 WHERE automation_id = ?1",
            params![id, completed && current.enabled, (completed && current.enabled).then_some(now_unix_ms().saturating_add(current.interval_minutes * 60000)), status, reason]
        ).map_err(|error| format!("settle automation: {error}"))?;
        transaction.commit().map_err(|error| error.to_string())
    }

    /// A pre-claim validation/build failure pauses only the exact config the
    /// scheduler read. Never overwrite a concurrent edit, pause or busy skip.
    pub fn fail_unclaimed_automation(
        &self,
        id: &str,
        revision: i64,
        error: &str,
    ) -> Result<(), String> {
        self.lock()?.execute(
            "UPDATE automations SET enabled = 0, next_run_at = NULL, last_status = 'failed', last_error = ?3, revision = revision + 1 WHERE automation_id = ?1 AND revision = ?2 AND running_turn_id IS NULL AND enabled = 1",
            params![id, revision, crate::redact_text(&super::bounded_text(error))],
        ).map_err(|error| format!("pause invalid automation: {error}"))?;
        Ok(())
    }
}

pub(super) fn authorize_claim(
    transaction: &Transaction<'_>,
    id: &str,
    session: &HubSessionSummary,
    prompt: &str,
    now: i64,
) -> Result<(), String> {
    let current = query(transaction, id)?;
    if !current.enabled
        || current.running_turn_id.is_some()
        || current.next_run_at.is_none_or(|due| due > now)
    {
        return Err("Automation is paused, claimed, or not due".into());
    }
    if current.product_session_id != session.product_session_id || current.prompt != prompt {
        return Err("Automation request does not match its stored session and prompt".into());
    }
    let valid = current.branch_id == session.active_branch_id
        && current.target == query_branch_target(transaction, &session.active_branch_id)?
        && ordinary_session(transaction, session).is_ok();
    if !valid {
        transaction.execute("UPDATE automations SET enabled = 0, next_run_at = NULL, last_status = 'route_changed', last_error = 'Session route or collaboration scope changed; edit and review again.', revision = revision + 1 WHERE automation_id = ?1", [id]).map_err(|error| error.to_string())?;
        return Err(
            "Automation paused because its authorized route or session scope changed".into(),
        );
    }
    if session.status == "running" {
        transaction.execute("UPDATE automations SET next_run_at = ?2, last_status = 'skipped_busy', revision = revision + 1 WHERE automation_id = ?1", params![id, now.saturating_add(current.interval_minutes * 60000)]).map_err(|error| error.to_string())?;
        return Err("Automation tick skipped because the session is busy".into());
    }
    if now.saturating_sub(current.next_run_at.unwrap_or(now)) >= current.interval_minutes * 60000 {
        transaction.execute("UPDATE automations SET next_run_at = ?2, last_status = 'skipped_missed', revision = revision + 1 WHERE automation_id = ?1", params![id, now.saturating_add(current.interval_minutes * 60000)]).map_err(|error| error.to_string())?;
        return Err("Automation missed tick skipped; no catch-up run is started".into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::PermissionProfile;

    fn fixture() -> (tempfile::TempDir, tempfile::TempDir, HubStore) {
        let data = tempfile::tempdir().unwrap();
        let workspace = tempfile::tempdir().unwrap();
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        let prepared = store
            .prepare_new_turn(
                "session",
                workspace.path(),
                "first",
                &EngineTarget::for_engine("codex").unwrap(),
                PermissionProfile::WorkspaceWrite,
            )
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        (data, workspace, store)
    }
    fn create(store: &HubStore) -> Automation {
        store
            .automation_action(AutomationAction::Create {
                product_session_id: "session".into(),
                title: "Review".into(),
                prompt: "inspect".into(),
                interval_minutes: 5,
            })
            .unwrap()
            .unwrap()
    }
    fn enable_due(store: &HubStore, row: &Automation) -> Automation {
        let enabled = store
            .automation_action(AutomationAction::SetEnabled {
                automation_id: row.automation_id.clone(),
                revision: row.revision,
                enabled: true,
            })
            .unwrap()
            .unwrap();
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE automations SET next_run_at = ?2 WHERE automation_id = ?1",
                params![row.automation_id, now_unix_ms() - 1],
            )
            .unwrap();
        enabled
    }
    fn claim(store: &HubStore, row: &Automation) -> Result<super::super::PreparedTurn, String> {
        store.prepare_resumed_turn_for_automation("session", &row.prompt, Some(&row.automation_id))
    }

    #[test]
    fn automation_crud_is_paused_first_bounded_and_optimistic() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        assert!(!row.enabled);
        assert_eq!(row.next_run_at, None);
        assert!(store.due_automation_ids().unwrap().is_empty());
        let enabled = enable_due(&store, &row);
        assert!(store
            .automation_action(AutomationAction::Delete {
                automation_id: row.automation_id.clone(),
                revision: row.revision
            })
            .is_err());
        let edited = store
            .automation_action(AutomationAction::Update {
                automation_id: row.automation_id.clone(),
                revision: enabled.revision,
                title: "Edited".into(),
                prompt: "new".into(),
                interval_minutes: 10,
            })
            .unwrap()
            .unwrap();
        assert!(!edited.enabled);
        assert_eq!(edited.next_run_at, None);
        assert_eq!(edited.prompt, "new");
        assert!(validate("", "prompt", 5).is_err());
        assert!(validate("title", &"x".repeat(16385), 5).is_err());
        assert!(validate("title", "prompt", 4).is_err());
        assert!(validate("title", "prompt", 10081).is_err());
        store
            .automation_action(AutomationAction::Delete {
                automation_id: row.automation_id,
                revision: edited.revision,
            })
            .unwrap();
        assert!(store.list_automations().unwrap().is_empty());
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns.len(),
            1
        );
    }

    #[test]
    fn automation_claim_is_atomic_exact_and_cannot_be_replayed() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        assert!(claim(&store, &row).is_err());
        enable_due(&store, &row);
        assert!(store
            .prepare_resumed_turn_for_automation("session", "forged", Some(&row.automation_id))
            .is_err());
        let prepared = claim(&store, &row).unwrap();
        let claimed = store.get_automation(&row.automation_id).unwrap();
        assert_eq!(
            claimed.running_turn_id.as_deref(),
            Some(prepared.turn_id.as_str())
        );
        assert_eq!(claimed.next_run_at, None);
        assert!(claim(&store, &row).is_err());
        assert!(store.due_automation_ids().unwrap().is_empty());
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns.len(),
            2
        );
        assert!(store
            .automation_action(AutomationAction::Delete {
                automation_id: row.automation_id,
                revision: claimed.revision
            })
            .is_err());
    }

    #[test]
    fn automation_busy_tick_is_skipped_not_queued() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        enable_due(&store, &row);
        let manual = store.prepare_resumed_turn("session", "manual").unwrap();
        assert!(claim(&store, &row).unwrap_err().contains("busy"));
        let skipped = store.get_automation(&row.automation_id).unwrap();
        assert!(skipped.enabled);
        assert_eq!(skipped.last_status, "skipped_busy");
        assert!(skipped.next_run_at.unwrap() > now_unix_ms());
        assert_eq!(skipped.last_run_at, None);
        store
            .finish_turn(&manual, None, Some("done"), None, None)
            .unwrap();
        assert!(claim(&store, &row).is_err());
    }

    #[test]
    fn automation_suspend_misses_are_skipped_and_stop_pauses_future_runs() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        enable_due(&store, &row);
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE automations SET next_run_at = 0 WHERE automation_id = ?1",
                [&row.automation_id],
            )
            .unwrap();
        assert!(claim(&store, &row).unwrap_err().contains("missed tick"));
        let skipped = store.get_automation(&row.automation_id).unwrap();
        assert_eq!(skipped.last_status, "skipped_missed");
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns.len(),
            1
        );
        store
            .lock()
            .unwrap()
            .execute(
                "UPDATE automations SET next_run_at = ?2 WHERE automation_id = ?1",
                params![row.automation_id, now_unix_ms() - 1],
            )
            .unwrap();
        let prepared = claim(&store, &row).unwrap();
        assert!(store
            .record_attempt_cancelled_bound("session", &prepared.attempt_id, "Stop")
            .unwrap());
        store
            .finish_turn(
                &prepared,
                None,
                Some("natural completion after Stop"),
                None,
                None,
            )
            .unwrap();
        store.finish_automation(&row.automation_id, None).unwrap();
        let stopped = store.get_automation(&row.automation_id).unwrap();
        assert!(!stopped.enabled);
        assert_eq!(stopped.last_status, "interrupted");
        assert_eq!(stopped.next_run_at, None);
    }

    #[test]
    fn automation_pause_during_run_is_not_undone_and_failure_pauses() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        enable_due(&store, &row);
        let prepared = claim(&store, &row).unwrap();
        let running = store.get_automation(&row.automation_id).unwrap();
        store
            .automation_action(AutomationAction::SetEnabled {
                automation_id: row.automation_id.clone(),
                revision: running.revision,
                enabled: false,
            })
            .unwrap();
        store
            .finish_turn(&prepared, None, Some("done"), None, None)
            .unwrap();
        store.finish_automation(&row.automation_id, None).unwrap();
        let paused = store.get_automation(&row.automation_id).unwrap();
        assert!(!paused.enabled);
        assert_eq!(paused.last_status, "completed");
        assert_eq!(paused.next_run_at, None);
        enable_due(&store, &paused);
        let prepared = claim(&store, &paused).unwrap();
        store
            .finish_turn(&prepared, None, None, Some("fixture failure"), None)
            .unwrap();
        store.finish_automation(&row.automation_id, None).unwrap();
        let failed = store.get_automation(&row.automation_id).unwrap();
        assert!(!failed.enabled);
        assert_eq!(failed.last_status, "failed");
        assert_eq!(failed.last_error.as_deref(), Some("fixture failure"));
    }

    #[test]
    fn automation_route_or_team_change_requires_new_review() {
        let (_data, _workspace, store) = fixture();
        let row = create(&store);
        store
            .request_engine_switch(
                "session",
                &EngineTarget::for_engine("claude").unwrap(),
                "manual",
            )
            .unwrap();
        assert!(store
            .automation_action(AutomationAction::SetEnabled {
                automation_id: row.automation_id.clone(),
                revision: row.revision,
                enabled: true
            })
            .unwrap_err()
            .contains("route changed"));
        let updated = store
            .automation_action(AutomationAction::Update {
                automation_id: row.automation_id.clone(),
                revision: row.revision,
                title: row.title,
                prompt: row.prompt,
                interval_minutes: 5,
            })
            .unwrap()
            .unwrap();
        enable_due(&store, &updated);
        store
            .request_engine_switch(
                "session",
                &EngineTarget::for_engine("codex").unwrap(),
                "manual",
            )
            .unwrap();
        assert!(claim(&store, &updated).is_err());
        let veto = store.get_automation(&row.automation_id).unwrap();
        assert!(!veto.enabled);
        assert_eq!(veto.last_status, "route_changed");
        store
            .create_team_run("session", &EngineTarget::for_engine("codex").unwrap())
            .unwrap();
        assert!(store
            .automation_action(AutomationAction::Create {
                product_session_id: "session".into(),
                title: "team".into(),
                prompt: "work".into(),
                interval_minutes: 5
            })
            .is_err());
    }

    #[test]
    fn automation_restart_never_replays_missed_or_interrupted_ticks() {
        let (data, _workspace, store) = fixture();
        let row = create(&store);
        enable_due(&store, &row);
        let running = create(&store);
        enable_due(&store, &running);
        claim(&store, &running).unwrap();
        let path = data.path().join("hub.db");
        drop(store);
        let store = HubStore::open(path).unwrap();
        let idle = store.get_automation(&row.automation_id).unwrap();
        assert!(idle.enabled);
        assert!(idle.next_run_at.unwrap() > now_unix_ms());
        let recovered = store.get_automation(&running.automation_id).unwrap();
        assert!(!recovered.enabled);
        assert_eq!(recovered.running_turn_id, None);
        assert_eq!(recovered.last_status, "interrupted");
        assert!(store.due_automation_ids().unwrap().is_empty());
    }

    #[test]
    fn automation_schema_twenty_migrates_without_changing_session_history() {
        let (data, _workspace, store) = fixture();
        store
            .lock()
            .unwrap()
            .execute_batch("DROP TABLE automations; PRAGMA user_version = 20;")
            .unwrap();
        drop(store);
        let store = HubStore::open(data.path().join("hub.db")).unwrap();
        assert!(store.list_automations().unwrap().is_empty());
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns[0]
                .result
                .as_deref(),
            Some("done")
        );
    }

    #[tokio::test]
    async fn automation_runtime_uses_same_durable_turn_and_never_falls_back() {
        use std::sync::{
            atomic::{AtomicUsize, Ordering},
            Arc,
        };
        let (_data, workspace, store) = fixture();
        let mut fallback = EngineTarget::for_engine("claude").unwrap();
        fallback.automatic_use_authorized = true;
        store
            .set_session_fallback_policy("session", vec![fallback])
            .unwrap();
        let row = create(&store);
        enable_due(&store, &row);
        let calls = Arc::new(AtomicUsize::new(0));
        let called = calls.clone();
        let mut timeline = Vec::new();
        assert!(crate::triggers_fallback(crate::classify_failure(
            "engine crashed (synthetic)"
        )));
        let result = crate::run_persisted_with_automation(
            &store,
            move |target, _, _| {
                assert_eq!(target.engine_id, "codex");
                called.fetch_add(1, Ordering::SeqCst);
                Err("engine crashed (synthetic)".into())
            },
            |_| None,
            crate::RunRequest {
                prompt: row.prompt,
                workspace: workspace.path().to_path_buf(),
                engine_id: "codex".into(),
                product_session_id: Some("session".into()),
                permission_profile: None,
            },
            None,
            None,
            Some(&row.automation_id),
            |event| timeline.push(event),
        )
        .await
        .unwrap();
        assert!(result.failed);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(!timeline
            .iter()
            .any(|event| event.status == crate::TimelineStatus::Fallback));
        store.finish_automation(&row.automation_id, None).unwrap();
        assert_eq!(
            store.get_session("session").unwrap().unwrap().turns.len(),
            2
        );
        assert!(!store.get_automation(&row.automation_id).unwrap().enabled);
    }

    #[tokio::test]
    async fn automation_success_runs_a_real_double_once_and_persists_conversation_results() {
        use alva_engine_runtime::{
            EngineRuntime, PermissionDecision, RuntimeCapabilities, RuntimeError, RuntimeEvent,
            RuntimeRequest,
        };
        use std::{
            pin::Pin,
            sync::{
                atomic::{AtomicUsize, Ordering},
                Arc,
            },
        };
        struct Double(Arc<AtomicUsize>);
        #[async_trait::async_trait]
        impl EngineRuntime for Double {
            fn execute(
                &self,
                request: RuntimeRequest,
            ) -> Result<Pin<Box<dyn futures_core::Stream<Item = RuntimeEvent> + Send>>, RuntimeError>
            {
                self.0.fetch_add(1, Ordering::SeqCst);
                assert_eq!(request.prompt, "inspect");
                Ok(Box::pin(tokio_stream::iter(vec![
                    RuntimeEvent::SessionStarted {
                        session_id: "native-test".into(),
                        model: None,
                        tools: vec![],
                    },
                    RuntimeEvent::Completed {
                        session_id: "native-test".into(),
                        result: Some("scheduled result".into()),
                        usage: None,
                    },
                ])))
            }
            async fn cancel(&self, _: &str) -> Result<(), RuntimeError> {
                Ok(())
            }
            async fn respond_permission(
                &self,
                _: &str,
                _: &str,
                _: PermissionDecision,
            ) -> Result<(), RuntimeError> {
                Ok(())
            }
            fn capabilities(&self) -> RuntimeCapabilities {
                RuntimeCapabilities {
                    streaming: true,
                    tool_control: true,
                    permission_callback: true,
                    resume: true,
                    cancel: true,
                }
            }
        }
        let (_data, workspace, store) = fixture();
        let row = create(&store);
        enable_due(&store, &row);
        let calls = Arc::new(AtomicUsize::new(0));
        let double = Arc::new(Double(calls.clone()));
        let sink = crate::ActiveHubs::default();
        let request = crate::RunRequest {
            prompt: row.prompt.clone(),
            workspace: workspace.path().into(),
            engine_id: "codex".into(),
            product_session_id: Some("session".into()),
            permission_profile: None,
        };
        let outcome = crate::run_persisted_with_automation(
            &store,
            |_, _, profile| {
                assert_eq!(profile, PermissionProfile::WorkspaceWrite);
                Ok(double.clone())
            },
            |_| None,
            request.clone(),
            None,
            Some(&sink),
            Some(&row.automation_id),
            |_| {},
        )
        .await
        .unwrap();
        assert!(!outcome.failed);
        assert_eq!(outcome.result.as_deref(), Some("scheduled result"));
        assert!(sink.lock().unwrap().is_empty());
        assert!(crate::run_persisted_with_automation(
            &store,
            |_, _, _| Ok(double.clone()),
            |_| None,
            request,
            None,
            Some(&sink),
            Some(&row.automation_id),
            |_| {}
        )
        .await
        .is_err());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        store.finish_automation(&row.automation_id, None).unwrap();
        let settled = store.get_automation(&row.automation_id).unwrap();
        assert!(settled.enabled);
        assert!(settled.next_run_at.unwrap() > now_unix_ms());
        let detail = store.get_session("session").unwrap().unwrap();
        assert_eq!(detail.turns[1].result.as_deref(), Some("scheduled result"));
        assert!(!store
            .message_journal_page("session", 0, None)
            .unwrap()
            .records
            .is_empty());
    }
}
