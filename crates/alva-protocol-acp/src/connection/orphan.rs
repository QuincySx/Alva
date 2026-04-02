// INPUT:  (none)
// OUTPUT: PARENT_PID_ENV, parent_pid_env_value, cleanup_orphan_processes
// POS:    Orphan process cleanup — detect and kill leftover child processes from previous crashes

/// Environment variable name injected into child process (parent PID)
pub const PARENT_PID_ENV: &str = "ACP_PROCESS_MANAGER_PID";

/// Return the current process PID as a string for injection into child env.
pub fn parent_pid_env_value() -> String {
    std::process::id().to_string()
}

/// Orphan cleanup:
/// On AcpProcessManager startup, scan for child processes with PARENT_PID_ENV marker.
/// If their parent PID does not match current PID (leftover from previous crash), kill them.
///
/// Called once in AcpProcessManager::new().
pub async fn cleanup_orphan_processes() {
    tracing::info!("scanning for orphan ACP processes...");
    // Phase 1: no-op placeholder. Phase 2 will implement full platform-specific scanning.
    // TODO: enumerate system processes, find those with PARENT_PID_ENV whose parent PID
    // is not the current process, and send SIGTERM.
}
