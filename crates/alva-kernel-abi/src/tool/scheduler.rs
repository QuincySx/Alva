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
//! `SubAgentPlugin` spawns share the same `ToolLockRegistry` via the bus;
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
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    /// Read lock on `key`. Key is string-normalized (collapse `//`, resolve
    /// `.` and `..` segments) so logically identical paths map to the
    /// same lock entry.
    ///
    /// Note: the normalization is **purely string-based**, doesn't touch
    /// the filesystem. Relative paths stay relative — see
    /// [`ToolLockRegistry::acquire_within`] if you want relative paths
    /// resolved against a workspace root.
    pub fn read(key: impl Into<String>) -> Self {
        Self {
            key: normalize_path_string(&key.into()),
            mode: LockMode::Read,
        }
    }

    /// Write lock on `key`. See [`ResourceKey::read`] for normalization notes.
    pub fn write(key: impl Into<String>) -> Self {
        Self {
            key: normalize_path_string(&key.into()),
            mode: LockMode::Write,
        }
    }
}

/// Pure string path normalization. Safe on every target (no fs calls).
///
/// Handles:
/// - Collapses consecutive slashes: `/a//b` → `/a/b`
/// - Drops empty / `.` segments: `./a/./b` → `a/b`
/// - Resolves `..` segments: `a/b/../c` → `a/c`
/// - Preserves absolute-vs-relative: leading `/` is kept
///
/// Does NOT:
/// - Resolve relative → absolute (needs a workspace root — see
///   [`ToolLockRegistry::acquire_within`])
/// - Follow symlinks (needs fs access)
/// - Canonicalize Windows-style backslashes (assumes `/` separator,
///   which matches how LLMs emit paths in tool_use JSON)
fn normalize_path_string(raw: &str) -> String {
    let is_absolute = raw.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                // Pop only if the top of the stack is a real segment,
                // not a `..` we couldn't resolve earlier (relative paths
                // that escape their base keep their leading `..`s).
                if segments.last().map(|s| *s != "..").unwrap_or(false) {
                    segments.pop();
                } else if !is_absolute {
                    segments.push("..");
                }
                // Absolute paths never keep stray `..` — they're nonsensical.
            }
            s => segments.push(s),
        }
    }
    if is_absolute {
        format!("/{}", segments.join("/"))
    } else if segments.is_empty() {
        ".".to_string()
    } else {
        segments.join("/")
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
    /// Orchestrator — acquires NO lock. The tool performs no side effects of
    /// its own; it drives nested tools that each take their own locks (e.g.
    /// `AgentSpawnTool`, whose inlined sub-agent runs its own `execute_shell`).
    /// Taking any lock here would deadlock: an orchestrator that ran inline
    /// while holding the global read lock would block on the same task's
    /// nested `SerialGlobal` tool requesting the global write lock, which can
    /// never be granted while the read guard is alive.
    Coordinator,
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
/// Default bound for lock acquisition on the tool hot path. Generous on
/// purpose: legitimate queueing behind serial-global tools can take a few
/// tool-timeouts' worth of waiting; anything past this is deadlock-shaped
/// and a bounded error naming the keys beats an indefinite hang.
pub const DEFAULT_ACQUIRE_TIMEOUT: Duration = Duration::from_secs(300);

pub struct ToolLockRegistry {
    /// Bound applied by [`Self::acquire_bounded`] (the tool hot path).
    acquire_timeout: Duration,
    /// Per-key RwLock map. Lazily populated on first access.
    locks: std::sync::Mutex<HashMap<String, Arc<RwLock<()>>>>,
    /// Exclusive global lock held by `SerialGlobal` tools. A `SerialGlobal`
    /// tool takes this in write mode, blocking all other executions until
    /// it releases. `Parallel` tools take it in read mode, so multiple
    /// parallel tools coexist but pause collectively when a SerialGlobal
    /// is running.
    global: Arc<RwLock<()>>,
    /// Active holder snapshot for `inspect()` diagnostics. Tracks active
    /// acquisitions in parallel with the actual lock guards — entries are
    /// inserted on acquire and removed when the matching `ToolLockGuards`
    /// drops. Strictly observational; does not affect lock semantics.
    inspect: Arc<Mutex<InspectState>>,
}

/// Sentinel key used in inspect snapshots when a `SerialGlobal` tool
/// holds the registry-wide exclusive lock.
pub const GLOBAL_SERIAL_KEY: &str = "<global-serial>";

#[derive(Debug, Default)]
struct InspectState {
    /// key → list of active holders (most recent last).
    holders: HashMap<String, Vec<HolderEntry>>,
    /// Monotonic id assigned to each holder for precise removal on drop.
    next_id: u64,
}

#[derive(Debug)]
struct HolderEntry {
    id: u64,
    mode: LockMode,
    holder: Option<String>,
    acquired_at: Instant,
}

/// Snapshot of one active lock acquisition, returned by [`ToolLockRegistry::inspect`].
#[derive(Debug, Clone)]
pub struct LockSnapshot {
    pub key: String,
    pub mode: LockMode,
    pub holder: Option<String>,
    pub age: Duration,
}

/// Returned when [`ToolLockRegistry::acquire_with_timeout`] times out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AcquireError {
    /// Could not obtain all requested locks within the deadline.
    Timeout {
        waited: Duration,
        keys: Vec<ResourceKey>,
    },
}

impl std::fmt::Display for AcquireError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AcquireError::Timeout { waited, keys } => {
                write!(
                    f,
                    "lock acquire timed out after {waited:?} on {} key(s)",
                    keys.len()
                )
            }
        }
    }
}

impl std::error::Error for AcquireError {}

impl ToolLockRegistry {
    pub fn new() -> Self {
        Self {
            acquire_timeout: DEFAULT_ACQUIRE_TIMEOUT,
            locks: std::sync::Mutex::new(HashMap::new()),
            global: Arc::new(RwLock::new(())),
            inspect: Arc::new(Mutex::new(InspectState::default())),
        }
    }

    /// Override the [`Self::acquire_bounded`] deadline (tests use a short
    /// fuse; production keeps [`DEFAULT_ACQUIRE_TIMEOUT`]).
    pub fn with_acquire_timeout(mut self, timeout: Duration) -> Self {
        self.acquire_timeout = timeout;
        self
    }

    /// Snapshot all currently held locks. Diagnostic only — readers see
    /// a consistent point-in-time view across all keys, but new acquires
    /// can complete before the caller acts on the result.
    pub fn inspect(&self) -> Vec<LockSnapshot> {
        let now = Instant::now();
        let state = self.inspect.lock().unwrap_or_else(|e| e.into_inner());
        let mut out = Vec::new();
        for (key, holders) in state.holders.iter() {
            for h in holders {
                out.push(LockSnapshot {
                    key: key.clone(),
                    mode: h.mode,
                    holder: h.holder.clone(),
                    age: now.saturating_duration_since(h.acquired_at),
                });
            }
        }
        // Stable order for predictable test/UI output.
        out.sort_by(|a, b| a.key.cmp(&b.key).then(a.age.cmp(&b.age)));
        out
    }

    /// Internal: register a new active holder, returning its unique id.
    fn track_holder(&self, key: &str, mode: LockMode, holder: Option<String>) -> u64 {
        let mut state = self.inspect.lock().unwrap_or_else(|e| e.into_inner());
        let id = state.next_id;
        state.next_id += 1;
        state
            .holders
            .entry(key.to_string())
            .or_default()
            .push(HolderEntry {
                id,
                mode,
                holder,
                acquired_at: Instant::now(),
            });
        id
    }

    // Untracking happens inline in `HolderTicket::drop` (below) — there was
    // once a `fn untrack_holder(key, id)` helper here, but the Drop impl
    // batches all the holds via `holds.drain(..)` and so it inlined the
    // same logic. The standalone helper became dead and was removed.

    /// Acquire all locks for one tool invocation. Returns a [`ToolLockGuards`]
    /// that releases them on drop.
    ///
    /// `keys` is typically `tool.resource_keys(input)`. They are sorted by
    /// key before acquisition to guarantee a total order and avoid
    /// cross-tool deadlock when two tools contend on the same set of keys.
    ///
    /// If `mode == SerialGlobal`, all `keys` are ignored and a single global
    /// write lock is held instead.
    pub async fn acquire(&self, keys: &[ResourceKey], mode: ExecutionMode) -> ToolLockGuards {
        self.acquire_with_holder(keys, mode, None).await
    }

    /// Like [`Self::acquire`] but tags the acquisition with a `holder`
    /// label (e.g. agent id, tool name) — purely observational, surfaces
    /// in `inspect()` snapshots for diagnostics.
    pub async fn acquire_with_holder(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
        holder: Option<String>,
    ) -> ToolLockGuards {
        match mode {
            ExecutionMode::Coordinator => ToolLockGuards {
                global_write: None,
                global_read: None,
                read: Vec::new(),
                write: Vec::new(),
                ticket: HolderTicket {
                    registry: Arc::downgrade(&self.inspect),
                    holds: Vec::new(),
                },
            },
            ExecutionMode::SerialGlobal => {
                let guard = self.global.clone().write_owned().await;
                let id = self.track_holder(GLOBAL_SERIAL_KEY, LockMode::Write, holder);
                ToolLockGuards {
                    global_write: Some(guard),
                    global_read: None,
                    read: Vec::new(),
                    write: Vec::new(),
                    ticket: HolderTicket {
                        registry: Arc::downgrade(&self.inspect),
                        holds: vec![(GLOBAL_SERIAL_KEY.to_string(), id)],
                    },
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
                let mut holds = Vec::new();
                for rk in dedup {
                    let lock = self.get_or_create(&rk.key);
                    match rk.mode {
                        LockMode::Read => {
                            let g = lock.read_owned().await;
                            let id = self.track_holder(&rk.key, LockMode::Read, holder.clone());
                            holds.push((rk.key.clone(), id));
                            read.push(g);
                        }
                        LockMode::Write => {
                            let g = lock.write_owned().await;
                            let id = self.track_holder(&rk.key, LockMode::Write, holder.clone());
                            holds.push((rk.key.clone(), id));
                            write.push(g);
                        }
                    }
                }
                ToolLockGuards {
                    global_write: None,
                    global_read: Some(global_read),
                    read,
                    write,
                    ticket: HolderTicket {
                        registry: Arc::downgrade(&self.inspect),
                        holds,
                    },
                }
            }
        }
    }

    /// Like [`Self::acquire`] but bounded by a deadline. Returns
    /// [`AcquireError::Timeout`] if not all locks were obtained within
    /// `timeout`.
    ///
    /// Use this on the agent hot path to surface deadlock-shaped issues
    /// as bounded errors instead of indefinite hangs.
    pub async fn acquire_with_timeout(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
        timeout: Duration,
    ) -> Result<ToolLockGuards, AcquireError> {
        self.acquire_with_holder_and_timeout(keys, mode, None, timeout)
            .await
    }

    /// Combined holder-tagged + bounded acquire. See
    /// [`Self::acquire_with_holder`] and [`Self::acquire_with_timeout`].
    pub async fn acquire_with_holder_and_timeout(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
        holder: Option<String>,
        timeout: Duration,
    ) -> Result<ToolLockGuards, AcquireError> {
        let started = Instant::now();
        match tokio::time::timeout(timeout, self.acquire_with_holder(keys, mode, holder)).await {
            Ok(guards) => Ok(guards),
            Err(_elapsed) => Err(AcquireError::Timeout {
                waited: started.elapsed(),
                keys: keys.to_vec(),
            }),
        }
    }

    /// Like [`Self::acquire`] but resolves relative paths in `keys` against
    /// `workspace` before taking locks. This collapses the common collision
    /// where tool A says `"src/foo.rs"` (relative) and tool B says
    /// `"/workspace/src/foo.rs"` (absolute) — both resolve to the same
    /// absolute key and take the same lock.
    ///
    /// Paths already absolute are left alone (still string-normalized).
    /// Paths with no sensible workspace root stay relative.
    pub async fn acquire_within(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
        workspace: &std::path::Path,
    ) -> ToolLockGuards {
        let resolved: Vec<ResourceKey> = keys
            .iter()
            .map(|k| ResourceKey {
                key: resolve_against_workspace(&k.key, workspace),
                mode: k.mode,
            })
            .collect();
        self.acquire(&resolved, mode).await
    }

    /// The tool hot-path acquire: workspace-resolved (when a workspace is
    /// known) AND bounded by this registry's `acquire_timeout`. A wait past
    /// the bound returns [`AcquireError::Timeout`] instead of hanging the
    /// agent loop forever — deadlock-shaped contention becomes a visible,
    /// retryable tool error.
    pub async fn acquire_bounded(
        &self,
        keys: &[ResourceKey],
        mode: ExecutionMode,
        workspace: Option<&std::path::Path>,
    ) -> Result<ToolLockGuards, AcquireError> {
        let resolved: Vec<ResourceKey> = match workspace {
            Some(ws) => keys
                .iter()
                .map(|k| ResourceKey {
                    key: resolve_against_workspace(&k.key, ws),
                    mode: k.mode,
                })
                .collect(),
            None => keys.to_vec(),
        };
        self.acquire_with_holder_and_timeout(&resolved, mode, None, self.acquire_timeout)
            .await
    }

    fn get_or_create(&self, key: &str) -> Arc<RwLock<()>> {
        let mut map = self.locks.lock().unwrap_or_else(|e| e.into_inner());
        map.entry(key.to_string())
            .or_insert_with(|| Arc::new(RwLock::new(())))
            .clone()
    }

    /// For diagnostics: how many distinct resource keys are currently tracked.
    pub fn tracked_keys(&self) -> usize {
        self.locks.lock().unwrap_or_else(|e| e.into_inner()).len()
    }
}

fn resolve_against_workspace(key: &str, workspace: &std::path::Path) -> String {
    if key.starts_with('/') {
        // Already absolute — string-normalize was done at construction.
        key.to_string()
    } else {
        // Relative key joined to absolute workspace → absolute normalized.
        let joined = workspace.join(key);
        normalize_path_string(&joined.to_string_lossy())
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
    /// Inspect-state ticket — declared last so it drops AFTER the actual
    /// lock guards (Rust drops fields in declaration order). Removing the
    /// holder entry from the inspect map after the lock is released keeps
    /// the diagnostic view eventually consistent.
    ticket: HolderTicket,
}

/// Removes a holder's entries from the inspect map on drop.
///
/// Holds a `Weak` to the inspect state so dropping after the registry
/// itself was dropped is harmless (the upgrade fails, no-op).
struct HolderTicket {
    registry: std::sync::Weak<Mutex<InspectState>>,
    holds: Vec<(String, u64)>,
}

impl Drop for HolderTicket {
    fn drop(&mut self) {
        let Some(state) = self.registry.upgrade() else {
            return;
        };
        let mut s = state.lock().unwrap_or_else(|e| e.into_inner());
        for (key, id) in self.holds.drain(..) {
            if let Some(entries) = s.holders.get_mut(&key) {
                entries.retain(|e| e.id != id);
                if entries.is_empty() {
                    s.holders.remove(&key);
                }
            }
        }
    }
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

    #[test]
    fn normalize_collapses_double_slashes_and_dots() {
        assert_eq!(normalize_path_string("/a/b/c"), "/a/b/c");
        assert_eq!(normalize_path_string("/a//b//c"), "/a/b/c");
        assert_eq!(normalize_path_string("/a/./b/./c"), "/a/b/c");
        assert_eq!(normalize_path_string("/a/b/../c"), "/a/c");
        assert_eq!(normalize_path_string("/a/b/../../c"), "/c");
        assert_eq!(normalize_path_string("./a/b"), "a/b");
        assert_eq!(normalize_path_string("a/../b"), "b");
        // Relative path that escapes its base keeps the leading `..`.
        assert_eq!(normalize_path_string("../a"), "../a");
        assert_eq!(normalize_path_string("../../a"), "../../a");
    }

    #[test]
    fn resource_key_normalizes_at_construction() {
        // Two ResourceKeys built from logically-equal paths hash the same.
        let a = ResourceKey::write("/src/./foo.rs");
        let b = ResourceKey::write("/src/bar/../foo.rs");
        let c = ResourceKey::write("/src//foo.rs");
        assert_eq!(a.key, "/src/foo.rs");
        assert_eq!(b.key, "/src/foo.rs");
        assert_eq!(c.key, "/src/foo.rs");
    }

    #[tokio::test]
    async fn acquire_within_collides_relative_and_absolute() {
        // Two tools: one declares "src/foo.rs" (relative), the other
        // "/workspace/src/foo.rs" (absolute). Under acquire_within(workspace=
        // "/workspace"), they must land on the same lock and serialize.
        let reg = Arc::new(ToolLockRegistry::new());
        let workspace = std::path::PathBuf::from("/workspace");
        let order = Arc::new(tokio::sync::Mutex::new(Vec::<u32>::new()));

        let reg1 = reg.clone();
        let ws1 = workspace.clone();
        let order1 = order.clone();
        let t1 = tokio::spawn(async move {
            let _g = reg1
                .acquire_within(
                    &[ResourceKey::write("src/foo.rs")],
                    ExecutionMode::Parallel,
                    &ws1,
                )
                .await;
            order1.lock().await.push(1);
            tokio::time::sleep(Duration::from_millis(50)).await;
            order1.lock().await.push(2);
        });

        tokio::time::sleep(Duration::from_millis(10)).await;

        let reg2 = reg.clone();
        let ws2 = workspace.clone();
        let order2 = order.clone();
        let t2 = tokio::spawn(async move {
            let _g = reg2
                .acquire_within(
                    &[ResourceKey::write("/workspace/src/foo.rs")],
                    ExecutionMode::Parallel,
                    &ws2,
                )
                .await;
            order2.lock().await.push(3);
        });

        t1.await.unwrap();
        t2.await.unwrap();
        assert_eq!(*order.lock().await, vec![1, 2, 3]);
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

    // ---- inspect() snapshot tests --------------------------------------

    #[test]
    fn inspect_empty_registry_is_empty() {
        let reg = ToolLockRegistry::new();
        assert!(reg.inspect().is_empty());
    }

    #[tokio::test]
    async fn inspect_lists_held_locks_with_holder_label() {
        let reg = Arc::new(ToolLockRegistry::new());
        let _g = reg
            .acquire_with_holder(
                &[ResourceKey::write("/x"), ResourceKey::read("/y")],
                ExecutionMode::Parallel,
                Some("editor-tool".into()),
            )
            .await;

        let snap = reg.inspect();
        assert_eq!(snap.len(), 2, "expected two snapshots, got {snap:?}");
        // Sorted by key alphabetically — /x < /y.
        assert_eq!(snap[0].key, "/x");
        assert_eq!(snap[0].mode, LockMode::Write);
        assert_eq!(snap[0].holder.as_deref(), Some("editor-tool"));
        assert_eq!(snap[1].key, "/y");
        assert_eq!(snap[1].mode, LockMode::Read);
    }

    #[tokio::test]
    async fn inspect_clears_after_guard_drops() {
        let reg = Arc::new(ToolLockRegistry::new());
        {
            let _g = reg
                .acquire(&[ResourceKey::write("/transient")], ExecutionMode::Parallel)
                .await;
            assert_eq!(reg.inspect().len(), 1);
        }
        // Give the runtime a tick to ensure the guard's Drop has flushed.
        tokio::task::yield_now().await;
        assert!(
            reg.inspect().is_empty(),
            "snapshot should be empty after guard drop"
        );
    }

    #[tokio::test]
    async fn inspect_serial_global_uses_sentinel_key() {
        let reg = Arc::new(ToolLockRegistry::new());
        let _g = reg
            .acquire_with_holder(&[], ExecutionMode::SerialGlobal, Some("bash-tool".into()))
            .await;
        let snap = reg.inspect();
        assert_eq!(snap.len(), 1);
        assert_eq!(snap[0].key, GLOBAL_SERIAL_KEY);
        assert_eq!(snap[0].mode, LockMode::Write);
        assert_eq!(snap[0].holder.as_deref(), Some("bash-tool"));
    }

    // ---- acquire_with_timeout tests ------------------------------------

    #[tokio::test]
    async fn acquire_with_timeout_succeeds_when_uncontended() {
        let reg = Arc::new(ToolLockRegistry::new());
        let res = reg
            .acquire_with_timeout(
                &[ResourceKey::write("/free")],
                ExecutionMode::Parallel,
                Duration::from_millis(100),
            )
            .await;
        assert!(res.is_ok(), "should acquire immediately when uncontended");
    }

    #[tokio::test]
    async fn acquire_with_timeout_times_out_on_contention() {
        let reg = Arc::new(ToolLockRegistry::new());
        let _holder = reg
            .acquire(&[ResourceKey::write("/blocked")], ExecutionMode::Parallel)
            .await;

        let res = reg
            .acquire_with_timeout(
                &[ResourceKey::write("/blocked")],
                ExecutionMode::Parallel,
                Duration::from_millis(50),
            )
            .await;
        match res {
            Err(AcquireError::Timeout { waited, keys }) => {
                assert!(waited >= Duration::from_millis(40));
                assert_eq!(keys.len(), 1);
                assert_eq!(keys[0].key, "/blocked");
            }
            Ok(_) => panic!("expected Timeout, got Ok"),
        }
    }

    #[tokio::test]
    async fn acquire_with_timeout_succeeds_after_release() {
        let reg = Arc::new(ToolLockRegistry::new());
        let holder = reg
            .acquire(&[ResourceKey::write("/k")], ExecutionMode::Parallel)
            .await;

        // Spawn a waiter with a generous timeout, then release.
        let reg2 = reg.clone();
        let waiter = tokio::spawn(async move {
            reg2.acquire_with_timeout(
                &[ResourceKey::write("/k")],
                ExecutionMode::Parallel,
                Duration::from_secs(2),
            )
            .await
        });

        // Yield then release.
        tokio::time::sleep(Duration::from_millis(20)).await;
        drop(holder);

        let res = waiter.await.unwrap();
        assert!(res.is_ok(), "waiter should acquire after release");
    }

    #[tokio::test]
    async fn inspect_age_grows_with_time() {
        let reg = Arc::new(ToolLockRegistry::new());
        let _g = reg
            .acquire(&[ResourceKey::read("/age-test")], ExecutionMode::Parallel)
            .await;
        let s1 = reg.inspect();
        tokio::time::sleep(Duration::from_millis(20)).await;
        let s2 = reg.inspect();
        assert!(
            s2[0].age >= s1[0].age,
            "age should monotonically grow: s1={:?}, s2={:?}",
            s1[0].age,
            s2[0].age
        );
        assert!(s2[0].age >= Duration::from_millis(15));
    }
}
