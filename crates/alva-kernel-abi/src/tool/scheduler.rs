// INPUT:  std
// OUTPUT: ResourceKey, LockMode, ExecutionMode, Scheduling, normalize_path_string
// POS:    Pure tool-scheduling vocabulary. Runtime lock map is
//         `alva-agent-core::tool_lock::ToolLockRegistry`（SDK 装配层）.

//! Tool scheduling **types** — concurrency declarations tools make.
//!
//! Runtime acquisition lives in `alva_agent_core::tool_lock` (the SDK
//! assembly layer) — the kernel itself holds no cross-agent facility.

use std::hash::Hash;

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
    /// the filesystem. Relative paths stay relative — resolve against a
    /// workspace root before constructing keys when absolute identity matters
    /// (see `ToolLockRegistry::acquire_within` in `alva-agent-core`).
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
/// - Resolve relative → absolute (needs a workspace root)
/// - Follow symlinks (needs fs access)
/// - Canonicalize Windows-style backslashes (assumes `/` separator,
///   which matches how LLMs emit paths in tool_use JSON)
pub fn normalize_path_string(raw: &str) -> String {
    let is_absolute = raw.starts_with('/');
    let mut segments: Vec<&str> = Vec::new();
    for seg in raw.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                if segments.last().map(|s| *s != "..").unwrap_or(false) {
                    segments.pop();
                } else if !is_absolute {
                    segments.push("..");
                }
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

/// 工具对自己**如何被并发执行**的自述 —— 内核唯一需要向工具打听的额外信息
/// （因为内核负责批量执行）。描述机制，不描述策略：允不允许跑、要不要人审
/// 是 harness 的工具策略层的事，不在这里。
///
/// 原来拆成四个独立 trait 方法（能否并行 / 碰哪些资源 / 执行模式 / 是否自管
/// 超时）的调度问答，永远一起被问、一起被用，现在合成这一份描述符。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Scheduling {
    /// 执行模式。`Parallel` 且 `concurrency_safe` 的调用才可能进并行波；
    /// `SerialGlobal` 独占整批；`Coordinator` 不持锁、内联驱动嵌套工具。
    pub execution_mode: ExecutionMode,
    /// 能否与其它工具调用安全并行。默认 `false` —— 未显式声明的工具是
    /// 顺序执行的硬屏障。
    pub concurrency_safe: bool,
    /// 本次调用会锁住的资源键。多读单写语义：同键多个 `Read` 并发，
    /// `Write` 独占。约定文件类工具用绝对路径，其它资源用 URI 形状
    /// 标识；键在获取前排序，跨调用全序、无死锁。
    pub resource_keys: Vec<ResourceKey>,
    /// 工具是否自管执行超时。`true` 时通用的单工具超时中间件不得再包一层
    /// 额外超时 —— 自超时的子 agent 派生工具、带显式取消的长流工具用。
    /// 默认 `false`（适用中间件的默认超时）。
    pub manages_own_timeout: bool,
}

impl Default for Scheduling {
    /// 默认值与拆分前的四个 trait 默认逐条一致：可并行模式但不声明并发
    /// 安全（即不进并行波）、无资源锁、不自管超时。
    fn default() -> Self {
        Self {
            execution_mode: ExecutionMode::Parallel,
            concurrency_safe: false,
            resource_keys: Vec::new(),
            manages_own_timeout: false,
        }
    }
}

impl Scheduling {
    /// 独占整批 —— 副作用无法精确建模的工具（如 Bash）。
    pub fn serial_global() -> Self {
        Self {
            execution_mode: ExecutionMode::SerialGlobal,
            ..Default::default()
        }
    }

    /// 编排器 —— 不持锁，内联驱动嵌套工具各自拿锁。
    pub fn coordinator() -> Self {
        Self {
            execution_mode: ExecutionMode::Coordinator,
            ..Default::default()
        }
    }

    /// 声明可与其它调用并行。
    pub fn concurrency_safe(mut self) -> Self {
        self.concurrency_safe = true;
        self
    }

    /// 声明资源键。
    pub fn with_resource_keys(mut self, keys: Vec<ResourceKey>) -> Self {
        self.resource_keys = keys;
        self
    }

    /// 声明自管超时。
    pub fn manages_own_timeout(mut self) -> Self {
        self.manages_own_timeout = true;
        self
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_normalization_collapses_dotdot_and_slashes() {
        assert_eq!(
            ResourceKey::write("/src/./foo.rs").key,
            ResourceKey::write("/src/bar/../foo.rs").key
        );
        assert_eq!(ResourceKey::read("/a//b").key, "/a/b");
    }

    #[test]
    fn execution_mode_default_is_parallel() {
        assert_eq!(ExecutionMode::default(), ExecutionMode::Parallel);
    }
}

/// Join a resource key to a workspace root (absolute keys pass through),
/// applying the same string normalization as [`ResourceKey`]. Pure path
/// algebra — no filesystem access.
pub fn resolve_against_workspace(key: &str, workspace: &std::path::Path) -> String {
    if key.starts_with('/') {
        // Already absolute — string-normalize was done at construction.
        key.to_string()
    } else {
        // Relative key joined to absolute workspace → absolute normalized.
        let joined = workspace.join(key);
        normalize_path_string(&joined.to_string_lossy())
    }
}
