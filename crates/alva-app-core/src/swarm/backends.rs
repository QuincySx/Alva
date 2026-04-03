// INPUT:  std::process::Command
// OUTPUT: SwarmBackend trait, InProcessBackend, TmuxBackend, select_backend
// POS:    Backend abstraction for swarm execution environments with auto-selection.

/// Trait for swarm execution backends.
///
/// A backend represents a way to run agent processes (in-process tokio tasks,
/// OS subprocesses, tmux panes, etc.). The swarm coordinator uses backends
/// to determine how to spawn agents.
pub trait SwarmBackend: Send + Sync {
    /// Human-readable backend name.
    fn name(&self) -> &str;

    /// Whether this backend is currently available on the system.
    fn is_available(&self) -> bool;

    /// Priority for auto-selection (higher = preferred when available).
    fn priority(&self) -> u32;
}

/// In-process backend — always available, runs agents as tokio tasks.
pub struct InProcessBackend;

impl SwarmBackend for InProcessBackend {
    fn name(&self) -> &str {
        "in-process"
    }
    fn is_available(&self) -> bool {
        true
    }
    fn priority(&self) -> u32 {
        1
    }
}

/// Tmux backend — requires an active tmux session, runs agents in split panes.
pub struct TmuxBackend;

impl SwarmBackend for TmuxBackend {
    fn name(&self) -> &str {
        "tmux"
    }
    fn is_available(&self) -> bool {
        std::process::Command::new("tmux")
            .arg("has-session")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    fn priority(&self) -> u32 {
        3
    }
}

/// Select the best available backend based on priority.
///
/// Prefers tmux if a session is active, falls back to in-process.
pub fn select_backend() -> Box<dyn SwarmBackend> {
    let backends: Vec<Box<dyn SwarmBackend>> = vec![
        Box::new(TmuxBackend),
        Box::new(InProcessBackend),
    ];

    backends
        .into_iter()
        .filter(|b| b.is_available())
        .max_by_key(|b| b.priority())
        .unwrap_or_else(|| Box::new(InProcessBackend))
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn in_process_always_available() {
        let backend = InProcessBackend;
        assert!(backend.is_available());
        assert_eq!(backend.name(), "in-process");
    }

    #[test]
    fn select_backend_returns_something() {
        // Should always succeed (InProcessBackend is always available)
        let backend = select_backend();
        assert!(!backend.name().is_empty());
        assert!(backend.is_available());
    }

    #[test]
    fn tmux_backend_name() {
        let backend = TmuxBackend;
        assert_eq!(backend.name(), "tmux");
        // Availability depends on the environment — don't assert it
    }

    #[test]
    fn priority_ordering() {
        assert!(TmuxBackend.priority() > InProcessBackend.priority());
    }
}
