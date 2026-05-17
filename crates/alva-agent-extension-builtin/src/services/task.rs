//! `TaskService` trait + default in-memory backend.
//!
//! Tools (`task_create`/`task_get`/`task_list`/`task_update`/`task_stop`/
//! `task_output`) call into a single shared `dyn TaskService` resolved from
//! the bus. The default `InMemoryTaskStore` keeps everything in process
//! memory — fine for a fresh agent session, gone on restart.
//!
//! # Examples
//!
//! Lifecycle of a single task through the default in-memory backend:
//!
//! ```rust,no_run
//! use alva_agent_extension_builtin::services::{
//!     InMemoryTaskStore, TaskService, TaskUpdate,
//! };
//! use alva_kernel_abi::{create_task_state, TaskStatus, TaskType};
//! use std::path::PathBuf;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let store = InMemoryTaskStore::new();
//!
//! // 1. Create
//! let task = create_task_state(
//!     TaskType::LocalAgent,
//!     "Compile crate".into(),
//!     None,
//!     PathBuf::from("/tmp/out"),
//! );
//! let id = task.id.clone();
//! store.create(task).await?;
//!
//! // 2. Promote to Running + append some live output
//! store.update(&id, TaskUpdate {
//!     status: Some(TaskStatus::Running),
//!     append_output: Some("cargo build started\n".into()),
//!     ..Default::default()
//! }).await?;
//!
//! // 3. List by status — narrow to running tasks only
//! let running = store.list(Some(TaskStatus::Running)).await;
//! assert_eq!(running.len(), 1);
//!
//! // 4. Read accumulated output
//! let log = store.read_output(&id).await?;
//! assert!(log.contains("cargo build started"));
//!
//! // 5. Finalize — terminal state can't be reverted (try Running again
//! //    would return TaskError::AlreadyTerminated)
//! store.stop(&id).await?;
//! # Ok(())
//! # }
//! ```
//!
//! Plugging in a persistent backend (e.g. SQLite, Postgres): implement
//! [`TaskService`] on your own type and register an extension whose
//! `name()` returns `"task"` — the default `TaskExtension` is then
//! skipped by `BaseAgent`'s name-based dedup.

use std::collections::HashMap;
use std::sync::Mutex;

use alva_kernel_abi::{TaskState, TaskStatus};
use async_trait::async_trait;

/// Errors returned by `TaskService` mutations. Reads return `Option`
/// instead — "not found" is not exceptional for `get`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskError {
    /// Operation referenced an id that's not in the store.
    NotFound(String),
    /// Re-opening a terminal-state task is rejected — agents would
    /// otherwise be able to walk a Killed/Completed task back to Running
    /// and lose audit clarity.
    AlreadyTerminated(String),
}

impl std::fmt::Display for TaskError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(id) => write!(f, "task not found: {id}"),
            Self::AlreadyTerminated(id) => {
                write!(f, "task already terminated: {id}")
            }
        }
    }
}

impl std::error::Error for TaskError {}

/// Partial mutation applied by `TaskService::update`. Any field that's
/// `None` is left untouched on the stored `TaskState`.
#[derive(Debug, Clone, Default)]
pub struct TaskUpdate {
    pub status: Option<TaskStatus>,
    pub description: Option<String>,
    /// Free-form output to append to the task's stdout/stderr log. The
    /// in-memory backend stores this as a single concatenated string keyed
    /// by task id; production backends might tail a real log file.
    pub append_output: Option<String>,
}

/// Persistent task tracker shared across the agent loop. Backed by
/// `InMemoryTaskStore` unless an extension has replaced the default.
#[async_trait]
pub trait TaskService: Send + Sync + 'static {
    async fn create(&self, state: TaskState) -> Result<(), TaskError>;
    async fn get(&self, id: &str) -> Option<TaskState>;
    /// List tasks, most-recently-started first. Pass a `status_filter`
    /// to narrow (e.g. `TaskStatus::Running`); `None` returns everything.
    async fn list(&self, status_filter: Option<TaskStatus>) -> Vec<TaskState>;
    async fn update(
        &self,
        id: &str,
        mutation: TaskUpdate,
    ) -> Result<TaskState, TaskError>;
    /// Force-terminate a task (status → Killed). Convenience wrapper —
    /// equivalent to `update(id, TaskUpdate { status: Some(Killed), .. })`.
    async fn stop(&self, id: &str) -> Result<TaskState, TaskError>;
    /// Read accumulated output for a task. Returns empty string if the
    /// task exists but has no output yet.
    async fn read_output(&self, id: &str) -> Result<String, TaskError>;
}

/// Default in-process backend. Two `Mutex`-guarded `HashMap`s: one for
/// task state, one for output buffers. Sync `Mutex` is fine here — every
/// operation is short and there's no `.await` while holding the lock.
pub struct InMemoryTaskStore {
    tasks: Mutex<HashMap<String, TaskState>>,
    output: Mutex<HashMap<String, String>>,
}

impl InMemoryTaskStore {
    pub fn new() -> Self {
        Self {
            tasks: Mutex::new(HashMap::new()),
            output: Mutex::new(HashMap::new()),
        }
    }
}

impl Default for InMemoryTaskStore {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl TaskService for InMemoryTaskStore {
    async fn create(&self, state: TaskState) -> Result<(), TaskError> {
        self.tasks
            .lock()
            .unwrap()
            .insert(state.id.clone(), state);
        Ok(())
    }

    async fn get(&self, id: &str) -> Option<TaskState> {
        self.tasks.lock().unwrap().get(id).cloned()
    }

    async fn list(&self, status_filter: Option<TaskStatus>) -> Vec<TaskState> {
        let tasks = self.tasks.lock().unwrap();
        let mut out: Vec<TaskState> = tasks
            .values()
            .filter(|t| status_filter.map_or(true, |s| t.status == s))
            .cloned()
            .collect();
        // Most recent first — matches what an LLM rendering a task list
        // expects to see at the top.
        out.sort_by(|a, b| b.start_time.cmp(&a.start_time));
        out
    }

    async fn update(
        &self,
        id: &str,
        mutation: TaskUpdate,
    ) -> Result<TaskState, TaskError> {
        let final_state = {
            let mut tasks = self.tasks.lock().unwrap();
            let task = tasks
                .get_mut(id)
                .ok_or_else(|| TaskError::NotFound(id.to_string()))?;
            if let Some(s) = mutation.status {
                if task.status.is_terminal() && !s.is_terminal() {
                    return Err(TaskError::AlreadyTerminated(id.to_string()));
                }
                task.status = s;
                if s.is_terminal() && task.end_time.is_none() {
                    task.end_time =
                        Some(chrono::Utc::now().timestamp() as u64);
                }
            }
            if let Some(d) = mutation.description {
                task.description = d;
            }
            task.clone()
        };
        if let Some(out) = mutation.append_output {
            self.output
                .lock()
                .unwrap()
                .entry(id.to_string())
                .or_default()
                .push_str(&out);
        }
        Ok(final_state)
    }

    async fn stop(&self, id: &str) -> Result<TaskState, TaskError> {
        self.update(
            id,
            TaskUpdate {
                status: Some(TaskStatus::Killed),
                ..Default::default()
            },
        )
        .await
    }

    async fn read_output(&self, id: &str) -> Result<String, TaskError> {
        let exists = self.tasks.lock().unwrap().contains_key(id);
        if !exists {
            return Err(TaskError::NotFound(id.to_string()));
        }
        Ok(self
            .output
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .unwrap_or_default())
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use alva_kernel_abi::{create_task_state, TaskType};
    use std::path::PathBuf;

    fn sample(desc: &str) -> TaskState {
        create_task_state(
            TaskType::LocalAgent,
            desc.to_string(),
            None,
            PathBuf::from("/tmp/out"),
        )
    }

    #[tokio::test]
    async fn create_then_get_roundtrip() {
        let store = InMemoryTaskStore::new();
        let t = sample("alpha");
        let id = t.id.clone();
        store.create(t).await.unwrap();
        let got = store.get(&id).await.expect("exists");
        assert_eq!(got.description, "alpha");
    }

    #[tokio::test]
    async fn list_is_most_recent_first() {
        let store = InMemoryTaskStore::new();
        let mut a = sample("old");
        a.start_time = 100;
        let mut b = sample("new");
        b.start_time = 200;
        store.create(a).await.unwrap();
        store.create(b).await.unwrap();
        let list = store.list(None).await;
        assert_eq!(list[0].description, "new");
        assert_eq!(list[1].description, "old");
    }

    #[tokio::test]
    async fn list_filter_by_status() {
        let store = InMemoryTaskStore::new();
        let mut running = sample("r");
        running.status = TaskStatus::Running;
        let mut done = sample("d");
        done.status = TaskStatus::Completed;
        store.create(running).await.unwrap();
        store.create(done).await.unwrap();
        let r = store.list(Some(TaskStatus::Running)).await;
        assert_eq!(r.len(), 1);
        assert_eq!(r[0].description, "r");
    }

    #[tokio::test]
    async fn update_status_sets_end_time_on_terminal() {
        let store = InMemoryTaskStore::new();
        let t = sample("x");
        let id = t.id.clone();
        store.create(t).await.unwrap();
        let updated = store
            .update(
                &id,
                TaskUpdate {
                    status: Some(TaskStatus::Completed),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.status, TaskStatus::Completed);
        assert!(updated.end_time.is_some());
    }

    #[tokio::test]
    async fn cannot_revive_terminal_task() {
        let store = InMemoryTaskStore::new();
        let mut t = sample("x");
        t.status = TaskStatus::Killed;
        let id = t.id.clone();
        store.create(t).await.unwrap();
        let err = store
            .update(
                &id,
                TaskUpdate {
                    status: Some(TaskStatus::Running),
                    ..Default::default()
                },
            )
            .await
            .unwrap_err();
        assert_eq!(err, TaskError::AlreadyTerminated(id));
    }

    #[tokio::test]
    async fn stop_kills_task() {
        let store = InMemoryTaskStore::new();
        let t = sample("x");
        let id = t.id.clone();
        store.create(t).await.unwrap();
        let stopped = store.stop(&id).await.unwrap();
        assert_eq!(stopped.status, TaskStatus::Killed);
    }

    #[tokio::test]
    async fn append_output_accumulates() {
        let store = InMemoryTaskStore::new();
        let t = sample("x");
        let id = t.id.clone();
        store.create(t).await.unwrap();
        store
            .update(
                &id,
                TaskUpdate {
                    append_output: Some("line1\n".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        store
            .update(
                &id,
                TaskUpdate {
                    append_output: Some("line2\n".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        let out = store.read_output(&id).await.unwrap();
        assert_eq!(out, "line1\nline2\n");
    }

    #[tokio::test]
    async fn read_output_unknown_task() {
        let store = InMemoryTaskStore::new();
        let err = store.read_output("nope").await.unwrap_err();
        assert_eq!(err, TaskError::NotFound("nope".into()));
    }

    /// Concurrency guard: the in-memory backend uses a sync `Mutex`, which
    /// is only safe because no `.await` happens inside the critical section.
    /// If someone refactors `update()` to hold the lock across `.await`, the
    /// runtime can stall under load. This test exercises 50 concurrent
    /// `create` + `update` ops on a multi-threaded runtime and asserts every
    /// task lands without loss or duplicate.
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn concurrent_create_and_update_no_lost_writes() {
        use std::sync::Arc;
        let store = Arc::new(InMemoryTaskStore::new());
        let n = 50;
        let mut handles = Vec::with_capacity(n);
        for i in 0..n {
            let s = store.clone();
            handles.push(tokio::spawn(async move {
                let task = create_task_state(
                    TaskType::LocalAgent,
                    format!("task-{i}"),
                    None,
                    PathBuf::from("/tmp/out"),
                );
                let id = task.id.clone();
                s.create(task).await.expect("create should succeed");
                // Each task immediately gets updated to Running — exercises
                // the read→mutate→write critical section under contention.
                s.update(
                    &id,
                    TaskUpdate {
                        status: Some(TaskStatus::Running),
                        ..Default::default()
                    },
                )
                .await
                .expect("update should succeed");
                id
            }));
        }
        let mut ids = Vec::with_capacity(n);
        for h in handles {
            ids.push(h.await.expect("task should join"));
        }
        // All N must be in the store and all in Running state
        let all = store.list(None).await;
        assert_eq!(all.len(), n, "expected {n} tasks, got {}", all.len());
        let running = store.list(Some(TaskStatus::Running)).await;
        assert_eq!(running.len(), n, "all should have reached Running");
        // No id collisions
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), n, "duplicate ids generated");
    }
}
