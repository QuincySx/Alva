// INPUT:  std collections + sync, futures, crate::bus_cap
// OUTPUT: ResourceKey, LockMode, ExecutionMode, ToolLockRegistry, ToolLockGuards
// POS:    Tool-level resource locking — multi-reader/single-writer semantics per
//         resource key, plus a global-serial mode for tools that can't be
//         precisely modeled (Bash). Lives here so sub-agents running in parallel
//         can share the lock map via the bus and avoid stepping on each other.

//! Resource lock primitives for tool execution scheduling.
//!
//! # Problem
//!
//! "Parallel by default" in the executor prompt is a lie unless the harness
//! actually executes tool calls concurrently. And once concurrent, two tools
//! editing the same file / spawning shells / writing to the same resource
//! will race. A lock map with multi-reader/single-writer semantics fixes
//! this without pushing coordination into each tool.
//!
//! # Current integration scope
//!
//! The single-agent main loop still executes tool calls sequentially (the
//! middleware chain holds `&mut AgentState` between calls). So lock acquisition
//! inside one agent is cheap — no contention with self.
//!
//! Where these locks actually bite: **sub-agent parallel runs**. Multiple
//! `SubAgentExtension` spawns share the same `ToolLockRegistry` via the bus;
//! if two sub-agents both try to edit `/src/foo.rs`, the second one's
//! EditFile call will block until the first finishes. This is the scenario
//! the registry is built for.
//!
//! Future work: flip the single-agent loop to actually spawn tool futures
//! concurrently when all are compatible — at that point the lock semantics
//! kick in there too.
//!
//! # Usage pattern
//!
//! ```ignore
//! let guards = registry.acquire(&tool.resource_keys(input), tool.execution_mode()).await;
//! let result = tool.execute(input, ctx).await;
//! drop(guards); // RAII: locks released automatically
//! ```

use std::collections::HashMap;
use std::sync::Arc;

use tokio::sync::{OwnedRwLockReadGuard, OwnedRwLockWriteGuard, RwLock};

// ---------------------------------------------------------------------------
// ResourceKey / LockMode
// ---------------------------------------------------------------------------

/// One resource a tool invocation wants to hold a lock on.
///
/// The `key` is an opaque identifier — by convention, for file-writing tools
/// it's the absolute file path; for other resources it can be any URI-shaped
/// string that uniquely identifies the contended resource.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ResourceKey {
    pub key: String,
    pub mode: LockMode,
}

impl ResourceKey {
    pub fn read(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            mode: LockMode::Read,
        }
    }

    pub fn write(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            mode: LockMode::Write,
        }
    }
}

/// Multi-reader / single-writer semantics per key:
/// - Read: many concurrent readers allowed, blocks writers.
/// - Write: exclusive; blocks both readers and other writers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LockMode {
    Read,
    Write,
}

// ---------------------------------------------------------------------------
// ExecutionMode
// ---------------------------------------------------------------------------

/// How the scheduler should treat this tool across a concurrent batch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ExecutionMode {
    /// Default: fine-grained lock per `resource_keys()`. Many `Parallel`
    /// tools can run at once if their lock sets don't conflict.
    #[default]
    Parallel,
    /// Global exclusive — regardless of `resource_keys()`, this tool runs
    /// alone. Nothing else runs while it does. Reserved for tools whose
    /// side effects can't be precisely modeled (e.g. Bash).
    SerialGlobal,
}

// ---------------------------------------------------------------------------
// ToolLockRegistry
// ---------------------------------------------------------------------------

/// Bus Capability: shared lock map for tool execution.
///
/// **Provider**: runtime builder (host / app layer) installs a single
/// instance and publishes it on the bus. Sub-agents spawned through
/// `AgentSpawnTool` see the same registry.
/// **Consumers**: tool execution path in `alva-kernel-core::run` acquires
/// locks before `tool.execute()` and releases on completion.
/// **Why bus**: multiple agent instances (main + sub-agents) must share
/// one lock map for cross-agent contention to work; per-agent state would
/// let two sub-agents both think they have the write lock on `/src/foo.rs`.
#[crate::bus_cap]
pub struct ToolLockRegistry {
    /// Per-key RwLock map. Lazily populated on first access.
    locks: std::sync::Mutex<HashMap<String, Arc<RwLock<()>>>>,
    /// Exclusive global lock held by `SerialGlobal` tools. A `SerialGlobal`
    /// tool takes this in write mode, blocking all other executions until
    /// it releases. `Parallel` tools take it in read mode, so multiple
    /// parallel tools coexist but pause collectively when a SerialGlobal
    /// is running.
    global: Arc<RwLock<()>>,
}

impl ToolLockRegistry {
    pub fn new() -> Self {
        Self {
            locks: std::sync::Mutex::new(HashMap::new()),
            global: Arc::new(RwLock::new(())),
        }
    }

    /// Acquire all locks for one tool invocation. Returns a [`ToolLockGuards`]
    /// that releases them on drop.
    ///
    /// `keys` is typically `tool.resource_keys(input)`. They are sorted by
    /// key before acquisition to guarantee a total order and avoid
    /// cross-tool deadlock when two tools contend on the same set of keys.
    ///
    /// If `mode == SerialGlobal`, all `keys` are ignored and a single global
    /// write lock is held instead.
    pub async fn acquire(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
    ) -> ToolLockGuards {
        match mode {
            ExecutionMode::SerialGlobal => {
                let guard = self.global.clone().write_owned().await;
                ToolLockGuards {
                    global_write: Some(guard),
                    global_read: None,
                    read: Vec::new(),
                    write: Vec::new(),
                }
            }
            ExecutionMode::Parallel => {
                // Read the global lock first so SerialGlobal pauses parallels.
                let global_read = self.global.clone().read_owned().await;

                // Sort keys to avoid cross-call deadlock. De-dup by key —
                // if the same key appears as both Read and Write, Write wins.
                let mut sorted: Vec<ResourceKey> = keys.to_vec();
                sorted.sort_by(|a, b| a.key.cmp(&b.key));
                let mut dedup: Vec<ResourceKey> = Vec::with_capacity(sorted.len());
                for rk in sorted {
                    if let Some(existing) = dedup.iter_mut().find(|e| e.key == rk.key) {
                        if rk.mode == LockMode::Write {
                            existing.mode = LockMode::Write;
                        }
                    } else {
                        dedup.push(rk);
                    }
                }

                let mut read = Vec::new();
                let mut write = Vec::new();
                for rk in dedup {
                    let lock = self.get_or_create(&rk.key);
                    match rk.mode {
                        LockMode::Read => read.push(lock.read_owned().await),
                        LockMode::Write => write.push(lock.write_owned().await),
                    }
                }
                ToolLockGuards {
                    global_write: None,
                    global_read: Some(global_read),
                    read,
                    write,
                }
            }
        }
    }

    fn get_or_create(&self, key: &str) -> Arc<RwLock<()>> {
        let mut map = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// For diagnostics: how many distinct resource keys are currently tracked.
    pub fn tracked_keys(&self) -> usize {
        self.locks
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .len()
    }
}

impl Default for ToolLockRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// ToolLockGuards
// ---------------------------------------------------------------------------

/// RAII handle owning the locks for one tool invocation. Drop to release.
///
/// Fields exist only to keep the guards alive (RAII) — they're dropped in
/// declaration order when this struct drops, which releases the underlying
/// locks. The compiler flags them as unused because Rust doesn't know about
/// `Drop`-for-side-effects patterns; `#[allow(dead_code)]` documents intent.
#[allow(dead_code)]
pub struct ToolLockGuards {
    global_write: Option<OwnedRwLockWriteGuard<()>>,
    global_read: Option<OwnedRwLockReadGuard<()>>,
    read: Vec<OwnedRwLockReadGuard<()>>,
    write: Vec<OwnedRwLockWriteGuard<()>>,
}

impl ToolLockGuards {
    pub fn is_global(&self) -> bool {
        self.global_write.is_some()
    }

    pub fn held_reads(&self) -> usize {
        self.read.len()
    }

    pub fn held_writes(&self) -> usize {
        self.write.len()
    }
}

// Drop impl not needed — Vec<Guard> and Option<Guard> release in order.

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    #[tokio::test]
    async fn read_locks_are_concurrent() {
        let reg = Arc::new(ToolLockRegistry::new());
        let start = tokio::time::Instant::now();

        let reg1 = reg.clone();
        let t1 = tokio::spawn(async move {
            let _g = reg1
                .acquire(&[ResourceKey::read("/foo")], ExecutionMode::Parallel)
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });
        let reg2 = reg.clone();
        let t2 = tokio::spawn(async move {
            let _g = reg2
                .acquire(&[ResourceKey::read("/foo")], ExecutionMode::Parallel)
                .await;
            tokio::time::sleep(Duration::from_millis(100)).await;
        });

        t1.await.unwrap();
        t2.await.unwrap();

        // Concurrent reads — should take ~100ms, not ~200ms.
        assert!(start.elapsed() < Duration::from_millis(180));
    }

    #[tokio::test]
    async fn writes_on_same_key_serialize() {
        let reg = Arc::new(ToolLockRegistry::new());
        let order = Arc::new(tokio::sync::Mutex::new(Vec::<u32>::new()));

        let reg1 = reg.clone();
        let order1 = order.clone();
        let t1 = tokio::spawn(async move {
            let _g = reg1
                .acquire(&[ResourceKey::write("/foo")], ExecutionMode::Parallel)
                .await;
            order1.lock().await.push(1);
            tokio::time::sleep(Duration::from_millis(50)).await;
            order1.lock().await.push(2);
        });

        // Give t1 a head start so it takes the lock first.
        tokio::time::sleep(Duration::from_millis(10)).await;

        let reg2 = reg.clone();
        let order2 = order.clone();
        let t2 = tokio::spawn(async move {
            let _g = reg2
                .acquire(&[ResourceKey::write("/foo")], ExecutionMode::Parallel)
                .await;
            order2.lock().await.push(3);
        });

        t1.await.unwrap();
        t2.await.unwrap();
        // 1 and 2 must appear before 3 — t2 must wait for t1.
        let order = order.lock().await;
        assert_eq!(*order, vec![1, 2, 3]);
    }

    #[tokio::test]
    async fn serial_global_blocks_parallels() {
        let reg = Arc::new(ToolLockRegistry::new());
        let parallel_running = Arc::new(AtomicUsize::new(0));
        let max_seen_during_global = Arc::new(AtomicUsize::new(0));

        // A long-running SerialGlobal task.
        let reg1 = reg.clone();
        let max1 = max_seen_during_global.clone();
        let running1 = parallel_running.clone();
        let t_global = tokio::spawn(async move {
            let _g = reg1.acquire(&[], ExecutionMode::SerialGlobal).await;
            for _ in 0..5 {
                tokio::time::sleep(Duration::from_millis(20)).await;
                let seen = running1.load(Ordering::SeqCst);
                if seen > max1.load(Ordering::SeqCst) {
                    max1.store(seen, Ordering::SeqCst);
                }
            }
        });

        // Give global a head start.
        tokio::time::sleep(Duration::from_millis(5)).await;

        // Spawn 3 parallel tasks — they must wait.
        let mut parallels = Vec::new();
        for _ in 0..3 {
            let reg_i = reg.clone();
            let running_i = parallel_running.clone();
            parallels.push(tokio::spawn(async move {
                let _g = reg_i
                    .acquire(
                        &[ResourceKey::read("/different/path")],
                        ExecutionMode::Parallel,
                    )
                    .await;
                running_i.fetch_add(1, Ordering::SeqCst);
                tokio::time::sleep(Duration::from_millis(10)).await;
                running_i.fetch_sub(1, Ordering::SeqCst);
            }));
        }

        t_global.await.unwrap();
        for p in parallels {
            p.await.unwrap();
        }
        // While the SerialGlobal was holding, zero parallels were observed running.
        assert_eq!(max_seen_during_global.load(Ordering::SeqCst), 0);
    }

    #[tokio::test]
    async fn dedup_promotes_to_write_on_mixed_modes() {
        let reg = Arc::new(ToolLockRegistry::new());
        // Ask for /foo as both Read and Write on the same call — should
        // acquire just a Write (not both).
        let guards = reg
            .acquire(
                &[ResourceKey::read("/foo"), ResourceKey::write("/foo")],
                ExecutionMode::Parallel,
            )
            .await;
        assert_eq!(guards.held_reads(), 0);
        assert_eq!(guards.held_writes(), 1);
    }

    #[tokio::test]
    async fn sort_avoids_deadlock_on_multi_key_tools() {
        // Two tools, each wanting /a and /b in opposite orders. If we acquired
        // in declared order they'd deadlock. Sorted order prevents this.
        let reg = Arc::new(ToolLockRegistry::new());

        let reg1 = reg.clone();
        let t1 = tokio::spawn(async move {
            let _g = reg1
                .acquire(
                    &[ResourceKey::write("/a"), ResourceKey::write("/b")],
                    ExecutionMode::Parallel,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(30)).await;
        });

        let reg2 = reg.clone();
        let t2 = tokio::spawn(async move {
            let _g = reg2
                .acquire(
                    // Reversed declared order — still sorts to /a then /b.
                    &[ResourceKey::write("/b"), ResourceKey::write("/a")],
                    ExecutionMode::Parallel,
                )
                .await;
            tokio::time::sleep(Duration::from_millis(30)).await;
        });

        // Must complete (would hang on deadlock).
        tokio::time::timeout(Duration::from_secs(2), async {
            t1.await.unwrap();
            t2.await.unwrap();
        })
        .await
        .expect("deadlock: tasks did not finish within 2s");
    }
}
