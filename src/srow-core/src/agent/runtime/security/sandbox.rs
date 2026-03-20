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
    /// Whether network access is allowed.
    allow_network: bool,
}

impl SandboxConfig {
    pub fn new(mode: SandboxMode) -> Self {
        let allow_network = matches!(
            mode,
            SandboxMode::RestrictiveOpen | SandboxMode::RestrictiveProxied | SandboxMode::PermissiveOpen
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

    /// On non-macOS platforms, return the command unchanged.
    #[cfg(not(target_os = "macos"))]
    pub fn wrap_command(&self, command: &str, args: &[&str]) -> Vec<String> {
        let mut result = vec![command.to_string()];
        result.extend(args.iter().map(|a| a.to_string()));
        result
    }

    /// Create a config from a workspace path with sensible defaults.
    pub fn for_workspace(workspace: &Path, mode: SandboxMode) -> Self {
        let mut config = Self::new(mode);
        config.add_writable_dir(workspace.to_path_buf());

        // Always allow writes to temp
        if let Ok(tmp) = std::env::var("TMPDIR") {
            config.add_writable_dir(std::path::PathBuf::from(tmp));
        } else {
            config.add_writable_dir(std::path::PathBuf::from("/tmp"));
        }

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
