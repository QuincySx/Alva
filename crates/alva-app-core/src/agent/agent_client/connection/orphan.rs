// INPUT:  std::process, tracing
// OUTPUT: SROW_PARENT_PID_ENV, parent_pid_env_value, cleanup_orphan_processes
// POS:    Orphan ACP process cleanup on startup (currently a no-op placeholder).
/// Environment variable name injected into child process (parent PID)
pub const SROW_PARENT_PID_ENV: &str = "SROW_PROCESS_MANAGER_PID";

/// Return the current process PID as a string for injection into child env.
pub fn parent_pid_env_value() -> String {
    std::process::id().to_string()
}

/// Srow-side orphan cleanup:
/// On AcpProcessManager startup, scan for child processes with SROW_PARENT_PID_ENV marker.
/// If their parent PID does not match current Srow PID (leftover from previous crash), kill them.
///
/// Platform-dependent implementation:
///   macOS/Linux: parse /proc/{pid}/environ or use sysctl
///   Windows: use CreateToolhelp32Snapshot to enumerate processes
///
/// Called once in AcpProcessManager::new().
pub async fn cleanup_orphan_processes() {
    tracing::info!("scanning for orphan ACP processes...");
    // Phase 1: no-op placeholder. Phase 2 will implement full platform-specific scanning.
    // TODO: enumerate system processes, find those with SROW_PARENT_PID_ENV whose parent PID
    // is not the current process, and send SIGTERM.
}
