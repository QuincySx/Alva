// INPUT:  serde, std::path, std::time, std::collections::hash_map, std::hash
// OUTPUT: TaskType, TaskStatus, TaskState, generate_task_id, create_task_state
// POS:    Task types for background/async task management — mirrors Claude Code's task model.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The kind of task being executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskType {
    LocalBash,
    LocalAgent,
    RemoteAgent,
    InProcessTeammate,
    LocalWorkflow,
    MonitorMcp,
    Dream,
}

/// Lifecycle status of a task.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TaskStatus {
    Pending,
    Running,
    Completed,
    Failed,
    Killed,
}

impl TaskStatus {
    /// Whether this status represents a terminal (finished) state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::Completed | Self::Failed | Self::Killed)
    }
}

/// Full state of a tracked task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskState {
    pub id: String,
    pub task_type: TaskType,
    pub status: TaskStatus,
    pub description: String,
    pub tool_use_id: Option<String>,
    pub start_time: u64,
    pub end_time: Option<u64>,
    pub total_paused_ms: Option<u64>,
    pub output_file: PathBuf,
    pub output_offset: usize,
    pub notified: bool,
}

impl TaskType {
    /// Single-character prefix for task IDs, allowing quick identification of task kind.
    pub fn prefix(&self) -> char {
        match self {
            Self::LocalBash => 'b',
            Self::LocalAgent => 'a',
            Self::RemoteAgent => 'r',
            Self::InProcessTeammate => 't',
            Self::LocalWorkflow => 'w',
            Self::MonitorMcp => 'm',
            Self::Dream => 'd',
        }
    }
}

/// Generate a unique task ID with a type prefix followed by 8 base-36 characters.
///
/// The ID combines a timestamp with random bits to ensure uniqueness without
/// requiring external dependencies like `uuid`.
pub fn generate_task_id(task_type: &TaskType) -> String {
    let prefix = task_type.prefix();
    let id_num = timestamp_nanos() ^ rand_u64();
    format!("{}{}", prefix, base36_encode(id_num))
}

/// Create a new `TaskState` in the `Pending` status.
pub fn create_task_state(
    task_type: TaskType,
    description: String,
    tool_use_id: Option<String>,
    output_file: PathBuf,
) -> TaskState {
    let id = generate_task_id(&task_type);
    let start_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    TaskState {
        id,
        task_type,
        status: TaskStatus::Pending,
        description,
        tool_use_id,
        start_time,
        end_time: None,
        total_paused_ms: None,
        output_file,
        output_offset: 0,
        notified: false,
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

fn timestamp_nanos() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos() as u64
}

fn rand_u64() -> u64 {
    use std::collections::hash_map::RandomState;
    use std::hash::{BuildHasher, Hasher};
    let s = RandomState::new();
    let mut hasher = s.build_hasher();
    hasher.write_u64(timestamp_nanos());
    hasher.finish()
}

fn base36_encode(mut n: u64) -> String {
    const CHARS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if n == 0 {
        return "00000000".to_string();
    }
    let mut result = Vec::new();
    while n > 0 {
        result.push(CHARS[(n % 36) as usize]);
        n /= 36;
    }
    result.reverse();
    // Pad to at least 8 chars
    while result.len() < 8 {
        result.insert(0, b'0');
    }
    String::from_utf8(result).unwrap()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_status_terminal() {
        assert!(!TaskStatus::Pending.is_terminal());
        assert!(!TaskStatus::Running.is_terminal());
        assert!(TaskStatus::Completed.is_terminal());
        assert!(TaskStatus::Failed.is_terminal());
        assert!(TaskStatus::Killed.is_terminal());
    }

    #[test]
    fn generate_task_id_format() {
        let id = generate_task_id(&TaskType::LocalBash);
        // Must start with the prefix character
        assert!(id.starts_with('b'), "LocalBash id should start with 'b': {}", id);
        // Must be at least 9 chars: 1 prefix + 8 base36
        assert!(id.len() >= 9, "id too short: {}", id);
        // All chars after prefix must be base36 (lowercase alphanumeric)
        for ch in id[1..].chars() {
            assert!(
                ch.is_ascii_lowercase() || ch.is_ascii_digit(),
                "non-base36 char in id: {}",
                ch
            );
        }
    }

    #[test]
    fn generate_task_id_uniqueness() {
        let ids: Vec<String> = (0..100)
            .map(|_| generate_task_id(&TaskType::LocalAgent))
            .collect();
        let unique: std::collections::HashSet<&str> =
            ids.iter().map(|s| s.as_str()).collect();
        // Should be all unique (probabilistically)
        assert_eq!(unique.len(), ids.len(), "generated duplicate task IDs");
    }

    #[test]
    fn task_type_prefix_distinct() {
        let all = [
            TaskType::LocalBash,
            TaskType::LocalAgent,
            TaskType::RemoteAgent,
            TaskType::InProcessTeammate,
            TaskType::LocalWorkflow,
            TaskType::MonitorMcp,
            TaskType::Dream,
        ];
        let prefixes: Vec<char> = all.iter().map(|t| t.prefix()).collect();
        let unique: std::collections::HashSet<char> = prefixes.iter().cloned().collect();
        assert_eq!(unique.len(), prefixes.len(), "duplicate task type prefixes");
    }

    #[test]
    fn base36_encode_zero() {
        let result = base36_encode(0);
        assert_eq!(result, "00000000");
    }

    #[test]
    fn base36_encode_small() {
        let result = base36_encode(35);
        // 35 in base36 is 'z', padded to 8 chars
        assert_eq!(result, "0000000z");
    }

    #[test]
    fn base36_encode_large() {
        let result = base36_encode(u64::MAX);
        // Should produce a valid base36 string > 8 chars
        assert!(result.len() >= 8);
        for ch in result.chars() {
            assert!(ch.is_ascii_lowercase() || ch.is_ascii_digit());
        }
    }

    #[test]
    fn create_task_state_defaults() {
        let state = create_task_state(
            TaskType::Dream,
            "test task".into(),
            Some("tool_123".into()),
            PathBuf::from("/tmp/output"),
        );
        assert!(state.id.starts_with('d'));
        assert_eq!(state.status, TaskStatus::Pending);
        assert_eq!(state.description, "test task");
        assert_eq!(state.tool_use_id, Some("tool_123".into()));
        assert!(state.end_time.is_none());
        assert!(!state.notified);
    }

    #[test]
    fn task_status_serde_roundtrip() {
        let statuses = [
            TaskStatus::Pending,
            TaskStatus::Running,
            TaskStatus::Completed,
            TaskStatus::Failed,
            TaskStatus::Killed,
        ];
        for status in &statuses {
            let json = serde_json::to_string(status).unwrap();
            let rt: TaskStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(*status, rt);
        }
    }

    #[test]
    fn task_type_serde_roundtrip() {
        let types = [
            TaskType::LocalBash,
            TaskType::LocalAgent,
            TaskType::RemoteAgent,
            TaskType::InProcessTeammate,
            TaskType::LocalWorkflow,
            TaskType::MonitorMcp,
            TaskType::Dream,
        ];
        for tt in &types {
            let json = serde_json::to_string(tt).unwrap();
            let rt: TaskType = serde_json::from_str(&json).unwrap();
            assert_eq!(*tt, rt);
        }
    }
}
