// INPUT:  std::path
// OUTPUT: SandboxMode, SandboxConfig
// POS:    Configures macOS Seatbelt sandbox profiles for restricting shell command execution.
use std::path::Path;

/// Sandbox execution mode, modeled after Wukong's four sandbox profiles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SandboxMode {
    /// Maximum restriction: default deny, only allow reads + writes to target dir.
    RestrictiveOpen,
    /// Like RestrictiveOpen but also blocks network.
    RestrictiveClosed,
    /// Restrictive + proxied network access.
    RestrictiveProxied,
    /// Minimal restrictions — for trusted tools / development mode.
    PermissiveOpen,
}

/// Configuration for sandbox-exec on macOS.
///
/// On macOS, we generate `.sb` (Seatbelt) profile strings and pass them to
/// `sandbox-exec -p <profile> <command>`.  On other platforms this is a no-op
/// placeholder that returns the command unchanged.
#[derive(Debug, Clone)]
pub struct SandboxConfig {
    mode: SandboxMode,
    /// Directories that the sandboxed process is allowed to write to.
    writable_dirs: Vec<std::path::PathBuf>,
    /// Whether network access is allowed. Only consulted by the macOS
    /// Seatbelt profile builder (gated to native), so on wasm32 this
    /// field is constructed but never read — cfg-allow the dead_code
    /// warning instead of cfg-gating the field, since it keeps the
    /// struct shape identical across targets.
    #[cfg_attr(target_family = "wasm", allow(dead_code))]
    allow_network: bool,
}

impl SandboxConfig {
    pub fn new(mode: SandboxMode) -> Self {
        let allow_network = matches!(
            mode,
            SandboxMode::RestrictiveOpen
                | SandboxMode::RestrictiveProxied
                | SandboxMode::PermissiveOpen
        );
        Self {
            mode,
            writable_dirs: Vec::new(),
            allow_network,
        }
    }

    pub fn mode(&self) -> SandboxMode {
        self.mode
    }

    /// Whether a sandbox is actually *applied* to this process's commands.
    ///
    /// **Currently false everywhere, including macOS.** This used to report
    /// `cfg!(target_os = "macos")`, on the theory that Seatbelt is available
    /// there — but availability is not application. [`wrap_command`] is the
    /// only thing that would put a command under `sandbox-exec`, and it has no
    /// production callers: nothing in the binary ever wraps anything.
    ///
    /// The distinction is the whole point. Its one caller gates `Bypass` and
    /// `AcceptShell` — modes that auto-run commands *because* something is
    /// supposed to contain them — and reporting `true` here made that gate
    /// wave those modes through on macOS with no sandbox, no warning, and none
    /// of the explicit acknowledgement Linux requires. Answering honestly costs
    /// macOS users an explicit `--dangerously-allow-unsandboxed`, which is
    /// exactly what their situation warrants.
    ///
    /// Ticket 13 flips this back to a real check when `--sandbox os` actually
    /// confines the worker. Until then it must not claim otherwise.
    ///
    /// [`wrap_command`]: Self::wrap_command
    pub const fn is_enforced() -> bool {
        false
    }

    /// Whether the configured mode is one that promises isolation
    /// (restrictive / proxied). `PermissiveOpen` makes no such promise, so a
    /// no-op on a non-macOS platform is not a broken promise for it.
    pub fn promises_isolation(&self) -> bool {
        !matches!(self.mode, SandboxMode::PermissiveOpen)
    }

    pub fn add_writable_dir(&mut self, dir: std::path::PathBuf) {
        if !self.writable_dirs.contains(&dir) {
            self.writable_dirs.push(dir);
        }
    }

    /// Generate a macOS Seatbelt (.sb) profile string.
    ///
    /// This is the primary output — callers pass it to `sandbox-exec -p`.
    #[cfg(target_os = "macos")]
    pub fn generate_sb_profile(&self) -> String {
        match self.mode {
            SandboxMode::PermissiveOpen => {
                // Minimal restrictions
                "(version 1)\n(allow default)\n".to_string()
            }
            _ => {
                let mut sb = String::new();
                sb.push_str("(version 1)\n");
                sb.push_str("(deny default)\n");

                // Always allow reading
                sb.push_str("(allow file-read*)\n");

                // Allow writes to specified directories
                for dir in &self.writable_dirs {
                    sb.push_str(&format!(
                        "(allow file-write* (subpath \"{}\"))\n",
                        dir.display()
                    ));
                }

                // Process execution
                sb.push_str("(allow process-exec)\n");
                sb.push_str("(allow process-fork)\n");

                // System basics
                sb.push_str("(allow sysctl-read)\n");
                sb.push_str("(allow mach-lookup)\n");

                // Network
                if self.allow_network {
                    sb.push_str("(allow network*)\n");
                }

                sb
            }
        }
    }

    /// Build a command vector that wraps the given command in sandbox-exec.
    ///
    /// On macOS, returns `["sandbox-exec", "-p", "<profile>", ...original_args]`.
    /// On non-macOS, returns the original command unchanged (no-op).
    #[cfg(target_os = "macos")]
    pub fn wrap_command(&self, command: &str, args: &[&str]) -> Vec<String> {
        let profile = self.generate_sb_profile();
        let mut result = vec![
            "sandbox-exec".to_string(),
            "-p".to_string(),
            profile,
            command.to_string(),
        ];
        result.extend(args.iter().map(|a| a.to_string()));
        result
    }

    /// On non-macOS platforms there is no OS-level sandbox, so the command is
    /// returned unchanged. When the configured mode *promised* isolation
    /// (restrictive / proxied) we emit a warning so the missing enforcement is
    /// observable rather than a silent false promise — see [`Self::is_enforced`].
    #[cfg(not(target_os = "macos"))]
    pub fn wrap_command(&self, command: &str, args: &[&str]) -> Vec<String> {
        if self.promises_isolation() {
            tracing::warn!(
                mode = ?self.mode,
                command = %command,
                "sandbox mode requested but this platform has no OS-level \
                 enforcement; the command will run WITHOUT isolation"
            );
        }
        let mut result = vec![command.to_string()];
        result.extend(args.iter().map(|a| a.to_string()));
        result
    }

    /// Create a config from a workspace path with sensible defaults.
    pub fn for_workspace(workspace: &Path, mode: SandboxMode) -> Self {
        let mut config = Self::new(mode);
        config.add_writable_dir(workspace.to_path_buf());

        // Always allow writes to temp
        config.add_writable_dir(std::env::temp_dir());

        config
    }
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self::new(SandboxMode::RestrictiveOpen)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `is_enforced` gates Bypass and AcceptShell — modes that auto-run
    /// commands on the promise that something contains them. It may only say
    /// "yes" when a command actually gets wrapped, and `wrap_command` still has
    /// no production callers, so the honest answer is no. It reported
    /// `cfg!(target_os = "macos")` once, which let macOS auto-run unsandboxed
    /// commands while Linux demanded --dangerously-allow-unsandboxed for the
    /// identical situation.
    ///
    /// Ticket 13 may flip this — but only together with real confinement. If
    /// this test is failing because someone wants `--sandbox os` to work, wire
    /// the sandbox first; do not delete the test.
    #[test]
    fn is_enforced_stays_false_until_a_sandbox_actually_wraps_commands() {
        assert!(
            !SandboxConfig::is_enforced(),
            "nothing calls wrap_command in production, so no platform confines anything yet"
        );
    }

    #[test]
    fn default_mode_is_restrictive_open() {
        let config = SandboxConfig::default();
        assert_eq!(config.mode(), SandboxMode::RestrictiveOpen);
    }

    #[test]
    fn restrictive_closed_denies_network() {
        let config = SandboxConfig::new(SandboxMode::RestrictiveClosed);
        assert!(!config.allow_network);
    }

    #[test]
    fn permissive_allows_network() {
        let config = SandboxConfig::new(SandboxMode::PermissiveOpen);
        assert!(config.allow_network);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sb_profile_contains_deny_default() {
        let config = SandboxConfig::new(SandboxMode::RestrictiveOpen);
        let profile = config.generate_sb_profile();
        assert!(profile.contains("(deny default)"));
        assert!(profile.contains("(allow file-read*)"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn sb_profile_writable_dir() {
        let mut config = SandboxConfig::new(SandboxMode::RestrictiveOpen);
        config.add_writable_dir(std::path::PathBuf::from("/projects/myapp"));
        let profile = config.generate_sb_profile();
        assert!(profile.contains("/projects/myapp"));
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn permissive_profile_allows_default() {
        let config = SandboxConfig::new(SandboxMode::PermissiveOpen);
        let profile = config.generate_sb_profile();
        assert!(profile.contains("(allow default)"));
        assert!(!profile.contains("(deny default)"));
    }

    #[test]
    fn wrap_command_non_empty() {
        let config = SandboxConfig::new(SandboxMode::RestrictiveOpen);
        let cmd = config.wrap_command("ls", &["-la", "/tmp"]);
        assert!(!cmd.is_empty());
        // On macOS: first element is sandbox-exec; on others: first is ls
        #[cfg(target_os = "macos")]
        assert_eq!(cmd[0], "sandbox-exec");
        #[cfg(not(target_os = "macos"))]
        assert_eq!(cmd[0], "ls");
    }
}
