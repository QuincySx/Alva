// INPUT:  std::process, tokio::process, tracing
// OUTPUT: SROW_PARENT_PID_ENV, parent_pid_env_value, cleanup_orphan_processes
// POS:    Orphan ACP process cleanup on startup — scans for processes left over from a previous crash.

/// Environment variable name injected into child ACP processes.
/// Value is the parent process's PID.
pub const SROW_PARENT_PID_ENV: &str = "SROW_PROCESS_MANAGER_PID";

/// Return the current process PID as a string for injection into child env.
pub fn parent_pid_env_value() -> String {
    std::process::id().to_string()
}

/// Scan for orphan ACP processes and terminate them.
///
/// On AcpProcessManager startup, we look for processes that have the
/// `SROW_PARENT_PID_ENV` marker in their environment but whose parent
/// PID no longer matches the current process (leftover from a crash).
///
/// Implementation uses `ps` on Unix. Windows is a no-op for now.
pub async fn cleanup_orphan_processes() {
    tracing::info!("scanning for orphan ACP processes...");

    #[cfg(unix)]
    {
        if let Err(e) = cleanup_unix().await {
            tracing::warn!(error = %e, "orphan cleanup failed (non-fatal)");
        }
    }

    #[cfg(not(unix))]
    {
        tracing::debug!("orphan cleanup not implemented on this platform");
    }
}

/// Unix implementation: use `ps` to find processes with our env var marker,
/// then check if their parent PID matches ours. Kill orphans with SIGTERM.
#[cfg(unix)]
async fn cleanup_unix() -> Result<(), String> {
    use tokio::process::Command;

    let my_pid = std::process::id();

    // Use `ps -e -o pid,ppid,command` to list all processes
    let output = Command::new("ps")
        .args(["-e", "-o", "pid,ppid,command"])
        .output()
        .await
        .map_err(|e| format!("failed to run ps: {}", e))?;

    if !output.status.success() {
        return Err("ps command failed".to_string());
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let mut orphan_count = 0u32;

    for line in stdout.lines().skip(1) {
        // Each line: "  PID  PPID COMMAND"
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 3 {
            continue;
        }

        let pid: u32 = match parts[0].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };
        let ppid: u32 = match parts[1].parse() {
            Ok(p) => p,
            Err(_) => continue,
        };

        // Skip if parent is us (it's our child, not an orphan)
        if ppid == my_pid {
            continue;
        }

        // Check if this process has our env marker by reading /proc/<pid>/environ (Linux)
        // or using the command string heuristic (macOS)
        if is_our_orphan(pid).await {
            tracing::info!(pid, ppid, "killing orphan ACP process");
            // Use `kill` command to send SIGTERM (avoids libc dependency)
            let _ = Command::new("kill")
                .args(["-TERM", &pid.to_string()])
                .output()
                .await;
            orphan_count += 1;
        }
    }

    if orphan_count > 0 {
        tracing::info!(count = orphan_count, "terminated orphan ACP processes");
    } else {
        tracing::debug!("no orphan ACP processes found");
    }

    Ok(())
}

/// Check if a process is one of our orphans by examining its environment.
#[cfg(unix)]
async fn is_our_orphan(pid: u32) -> bool {
    // On Linux, read /proc/<pid>/environ
    #[cfg(target_os = "linux")]
    {
        let environ_path = format!("/proc/{}/environ", pid);
        if let Ok(content) = tokio::fs::read(&environ_path).await {
            let env_str = String::from_utf8_lossy(&content);
            return env_str.contains(SROW_PARENT_PID_ENV);
        }
        false
    }

    // On macOS, /proc doesn't exist. Use `ps -e -ww -o pid,command` to check
    // if the process command contains our marker env var name as a heuristic.
    // This is less reliable but avoids requiring elevated permissions.
    #[cfg(target_os = "macos")]
    {
        use tokio::process::Command;

        let output = Command::new("ps")
            .args(["-p", &pid.to_string(), "-e", "-ww", "-o", "command"])
            .output()
            .await;

        match output {
            Ok(o) => {
                let stdout = String::from_utf8_lossy(&o.stdout);
                stdout.contains(SROW_PARENT_PID_ENV)
            }
            Err(_) => false,
        }
    }

    #[cfg(not(any(target_os = "linux", target_os = "macos")))]
    {
        let _ = pid;
        false
    }
}
