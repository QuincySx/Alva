// INPUT:  Automation definitions, local clock, owner-only application support directory
// OUTPUT: Durable schedules, due-run selection, and execution outcome bookkeeping
// POS:    Hub product automation store; the GPUI scheduler consumes this module

use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use chrono::{Datelike as _, Duration, FixedOffset, TimeZone as _, Timelike as _, Weekday};
use serde::{Deserialize, Serialize};

use crate::{EngineTarget, PermissionProfile};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationSchedule {
    Hourly,
    DailyAtNine,
    WeekdaysAtNine,
    MondaysAtNine,
}

impl AutomationSchedule {
    pub const ALL: [Self; 4] = [
        Self::Hourly,
        Self::DailyAtNine,
        Self::WeekdaysAtNine,
        Self::MondaysAtNine,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Hourly => "Every hour",
            Self::DailyAtNine => "Every day at 09:00",
            Self::WeekdaysAtNine => "Weekdays at 09:00",
            Self::MondaysAtNine => "Every Monday at 09:00",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|schedule| schedule.label() == value)
    }

    pub fn next_run_after(self, now_ms: i64, local_offset_seconds: i32) -> i64 {
        let offset = FixedOffset::east_opt(local_offset_seconds)
            .unwrap_or_else(|| FixedOffset::east_opt(0).expect("UTC offset is valid"));
        let now = offset
            .timestamp_millis_opt(now_ms)
            .single()
            .unwrap_or_else(|| offset.timestamp_millis_opt(0).unwrap());
        if self == Self::Hourly {
            let next = now
                .with_minute(0)
                .and_then(|value| value.with_second(0))
                .and_then(|value| value.with_nanosecond(0))
                .unwrap_or(now)
                + Duration::hours(1);
            return next.timestamp_millis();
        }

        for days in 0..=8 {
            let date = now.date_naive() + Duration::days(days);
            let allowed = match self {
                Self::DailyAtNine => true,
                Self::WeekdaysAtNine => !matches!(date.weekday(), Weekday::Sat | Weekday::Sun),
                Self::MondaysAtNine => date.weekday() == Weekday::Mon,
                Self::Hourly => unreachable!(),
            };
            if !allowed {
                continue;
            }
            let candidate = date
                .and_hms_opt(9, 0, 0)
                .and_then(|local| offset.from_local_datetime(&local).single());
            if let Some(candidate) = candidate.filter(|candidate| *candidate > now) {
                return candidate.timestamp_millis();
            }
        }
        (now + Duration::days(7)).timestamp_millis()
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum AutomationRunStatus {
    Idle,
    Running,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AutomationDefinition {
    pub id: String,
    pub name: String,
    pub prompt: String,
    pub schedule: AutomationSchedule,
    pub workspace: PathBuf,
    pub target: EngineTarget,
    pub permission_profile: PermissionProfile,
    pub enabled: bool,
    pub created_at_ms: i64,
    pub updated_at_ms: i64,
    pub next_run_at_ms: i64,
    pub last_started_at_ms: Option<i64>,
    pub last_finished_at_ms: Option<i64>,
    pub last_status: AutomationRunStatus,
    pub last_session_id: Option<String>,
    pub last_error: Option<String>,
}

impl AutomationDefinition {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        name: String,
        prompt: String,
        schedule: AutomationSchedule,
        workspace: PathBuf,
        target: EngineTarget,
        permission_profile: PermissionProfile,
        now_ms: i64,
        local_offset_seconds: i32,
    ) -> Self {
        Self {
            id,
            name,
            prompt,
            schedule,
            workspace,
            target,
            permission_profile,
            enabled: true,
            created_at_ms: now_ms,
            updated_at_ms: now_ms,
            next_run_at_ms: schedule.next_run_after(now_ms, local_offset_seconds),
            last_started_at_ms: None,
            last_finished_at_ms: None,
            last_status: AutomationRunStatus::Idle,
            last_session_id: None,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct AutomationStore {
    path: PathBuf,
    lock: Arc<Mutex<()>>,
}

impl AutomationStore {
    pub fn new(application_support: &Path) -> Self {
        Self {
            path: application_support.join("automations.json"),
            lock: Arc::new(Mutex::new(())),
        }
    }

    pub fn load(&self) -> Result<Vec<AutomationDefinition>, String> {
        let _guard = self.lock()?;
        self.load_unlocked()
    }

    fn load_unlocked(&self) -> Result<Vec<AutomationDefinition>, String> {
        if !self.path.exists() {
            return Ok(Vec::new());
        }
        let bytes = fs::read(&self.path).map_err(|error| format!("read automations: {error}"))?;
        serde_json::from_slice(&bytes).map_err(|error| format!("decode automations: {error}"))
    }

    pub fn save(&self, automations: &[AutomationDefinition]) -> Result<(), String> {
        let _guard = self.lock()?;
        self.save_unlocked(automations)
    }

    fn save_unlocked(&self, automations: &[AutomationDefinition]) -> Result<(), String> {
        let parent = self
            .path
            .parent()
            .ok_or_else(|| "automation store has no parent directory".to_string())?;
        fs::create_dir_all(parent)
            .map_err(|error| format!("create automation settings directory: {error}"))?;
        set_owner_only_directory(parent)?;
        let temporary = self.path.with_extension("json.tmp");
        let payload = serde_json::to_vec_pretty(automations)
            .map_err(|error| format!("encode automations: {error}"))?;
        let mut file = owner_only_file(&temporary)?;
        file.write_all(&payload)
            .and_then(|_| file.sync_all())
            .map_err(|error| format!("write automations: {error}"))?;
        fs::rename(&temporary, &self.path)
            .map_err(|error| format!("replace automations: {error}"))?;
        set_owner_only_file(&self.path)
    }

    pub fn due(&self, now_ms: i64) -> Result<Vec<AutomationDefinition>, String> {
        let _guard = self.lock()?;
        Ok(self
            .load_unlocked()?
            .into_iter()
            .filter(|automation| {
                automation.enabled
                    && automation.last_status != AutomationRunStatus::Running
                    && automation.next_run_at_ms <= now_ms
            })
            .collect())
    }

    pub fn insert(&self, automation: AutomationDefinition) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut automations = self.load_unlocked()?;
        if automations
            .iter()
            .any(|current| current.id == automation.id)
        {
            return Err(format!("automation '{}' already exists", automation.id));
        }
        automations.push(automation);
        self.save_unlocked(&automations)
    }

    pub fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        now_ms: i64,
        local_offset_seconds: i32,
    ) -> Result<AutomationDefinition, String> {
        self.update(id, |automation| {
            if automation.last_status == AutomationRunStatus::Running {
                return Err("A running automation cannot be disabled.".to_string());
            }
            automation.enabled = enabled;
            automation.updated_at_ms = now_ms;
            if enabled {
                automation.next_run_at_ms = automation
                    .schedule
                    .next_run_after(now_ms, local_offset_seconds);
            }
            Ok(())
        })
    }

    pub fn delete(&self, id: &str) -> Result<(), String> {
        let _guard = self.lock()?;
        let mut automations = self.load_unlocked()?;
        let index = automations
            .iter()
            .position(|automation| automation.id == id)
            .ok_or_else(|| "Automation no longer exists.".to_string())?;
        if automations[index].last_status == AutomationRunStatus::Running {
            return Err("A running automation cannot be deleted.".into());
        }
        automations.remove(index);
        self.save_unlocked(&automations)
    }

    pub fn mark_started(&self, id: &str, now_ms: i64) -> Result<AutomationDefinition, String> {
        self.update(id, |automation| {
            automation.last_status = AutomationRunStatus::Running;
            automation.last_started_at_ms = Some(now_ms);
            automation.last_error = None;
            automation.updated_at_ms = now_ms;
            Ok(())
        })
    }

    pub fn mark_finished(
        &self,
        id: &str,
        now_ms: i64,
        local_offset_seconds: i32,
        session_id: Option<String>,
        error: Option<String>,
    ) -> Result<AutomationDefinition, String> {
        self.update(id, |automation| {
            automation.last_status = if error.is_some() {
                AutomationRunStatus::Failed
            } else {
                AutomationRunStatus::Succeeded
            };
            automation.last_finished_at_ms = Some(now_ms);
            automation.last_session_id = session_id;
            automation.last_error = error;
            automation.next_run_at_ms = automation
                .schedule
                .next_run_after(now_ms, local_offset_seconds);
            automation.updated_at_ms = now_ms;
            Ok(())
        })
    }

    fn update(
        &self,
        id: &str,
        update: impl FnOnce(&mut AutomationDefinition) -> Result<(), String>,
    ) -> Result<AutomationDefinition, String> {
        let _guard = self.lock()?;
        let mut automations = self.load_unlocked()?;
        let automation = automations
            .iter_mut()
            .find(|automation| automation.id == id)
            .ok_or_else(|| format!("automation '{id}' was not found"))?;
        update(automation)?;
        let updated = automation.clone();
        self.save_unlocked(&automations)?;
        Ok(updated)
    }

    fn lock(&self) -> Result<std::sync::MutexGuard<'_, ()>, String> {
        self.lock
            .lock()
            .map_err(|_| "automation store lock is poisoned".to_string())
    }
}

fn owner_only_file(path: &Path) -> Result<std::fs::File, String> {
    let mut options = OpenOptions::new();
    options.create(true).truncate(true).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt as _;
        options.mode(0o600);
    }
    options
        .open(path)
        .map_err(|error| format!("open automation store: {error}"))
}

#[cfg(unix)]
fn set_owner_only_directory(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| format!("protect automation settings directory: {error}"))
}

#[cfg(not(unix))]
fn set_owner_only_directory(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(unix)]
fn set_owner_only_file(path: &Path) -> Result<(), String> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| format!("protect automation store: {error}"))
}

#[cfg(not(unix))]
fn set_owner_only_file(_: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> EngineTarget {
        EngineTarget::for_engine("codex").unwrap()
    }

    #[test]
    fn schedules_choose_the_next_allowed_slot() {
        let offset = 8 * 60 * 60;
        let tz = FixedOffset::east_opt(offset).unwrap();
        let friday = tz.with_ymd_and_hms(2026, 8, 14, 10, 30, 0).unwrap();
        let weekday =
            AutomationSchedule::WeekdaysAtNine.next_run_after(friday.timestamp_millis(), offset);
        let monday = tz.with_ymd_and_hms(2026, 8, 17, 9, 0, 0).unwrap();
        assert_eq!(weekday, monday.timestamp_millis());

        let hourly = AutomationSchedule::Hourly.next_run_after(friday.timestamp_millis(), offset);
        assert_eq!(
            hourly,
            tz.with_ymd_and_hms(2026, 8, 14, 11, 0, 0)
                .unwrap()
                .timestamp_millis()
        );
    }

    #[test]
    fn store_tracks_due_and_finished_runs() {
        let root = tempfile::tempdir().unwrap();
        let store = AutomationStore::new(root.path());
        let mut automation = AutomationDefinition::new(
            "nightly".into(),
            "Nightly".into(),
            "Run tests".into(),
            AutomationSchedule::DailyAtNine,
            root.path().to_path_buf(),
            target(),
            PermissionProfile::WorkspaceWrite,
            1_000,
            0,
        );
        automation.next_run_at_ms = 2_000;
        store.save(&[automation]).unwrap();

        assert!(store.due(1_999).unwrap().is_empty());
        assert_eq!(store.due(2_000).unwrap().len(), 1);
        store.mark_started("nightly", 2_000).unwrap();
        assert!(store.due(3_000).unwrap().is_empty());
        let finished = store
            .mark_finished("nightly", 3_000, 0, Some("session".into()), None)
            .unwrap();
        assert_eq!(finished.last_status, AutomationRunStatus::Succeeded);
        assert_eq!(finished.last_session_id.as_deref(), Some("session"));
        assert!(finished.next_run_at_ms > 3_000);
    }

    #[test]
    fn transactional_mutations_preserve_other_automations() {
        let root = tempfile::tempdir().unwrap();
        let store = AutomationStore::new(root.path());
        let first = AutomationDefinition::new(
            "first".into(),
            "First".into(),
            "Run first".into(),
            AutomationSchedule::Hourly,
            root.path().to_path_buf(),
            target(),
            PermissionProfile::WorkspaceWrite,
            1_000,
            0,
        );
        let second = AutomationDefinition::new(
            "second".into(),
            "Second".into(),
            "Run second".into(),
            AutomationSchedule::Hourly,
            root.path().to_path_buf(),
            target(),
            PermissionProfile::WorkspaceWrite,
            1_000,
            0,
        );

        store.insert(first).unwrap();
        store.mark_started("first", 2_000).unwrap();
        store.insert(second).unwrap();
        store
            .mark_finished("first", 3_000, 0, Some("session".into()), None)
            .unwrap();

        let automations = store.load().unwrap();
        assert_eq!(automations.len(), 2);
        assert_eq!(automations[0].last_status, AutomationRunStatus::Succeeded);
        assert_eq!(automations[1].id, "second");
    }
}
