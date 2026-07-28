// INPUT:  std
// OUTPUT: ResourceKey, LockMode, ExecutionMode, normalize_path_string
// POS:    Pure tool-scheduling vocabulary. Runtime lock map is
//         `alva-kernel-core::tool_lock::ToolLockRegistry`.

//! Tool scheduling **types** — concurrency declarations tools make.
//!
//! Runtime acquisition lives in `alva_kernel_core::tool_lock`.

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
    /// (see `ToolLockRegistry::acquire_within` in `alva-kernel-core`).
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
