// INPUT:  crate::error
// OUTPUT: ExternalAgentKind, AgentCliCommand, AgentDiscovery
// POS:    Agent discovery — resolve external agent CLI commands from PATH, built-in packages, or npx fallback

use std::path::PathBuf;

use crate::error::AcpError;

/// Supported external Agent types
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalAgentKind {
    /// Well-known agent with discovery hints
    Named {
        id: String,
        executables: Vec<String>,
        fallback_npx: Option<String>,
    },
    /// User-specified arbitrary command
    Generic { command: String },
}

/// Discovery result: complete executable command and arguments
#[derive(Debug, Clone)]
pub struct AgentCliCommand {
    pub kind: ExternalAgentKind,
    /// Executable file path (absolute path)
    pub executable: PathBuf,
    /// Additional arguments (e.g. npx package name)
    pub args: Vec<String>,
}

pub struct AgentDiscovery {
    packages_dir: PathBuf,
}

impl AgentDiscovery {
    pub fn new(app_name: &str) -> Self {
        Self {
            packages_dir: builtin_packages_dir(app_name),
        }
    }

    pub fn with_packages_dir(packages_dir: PathBuf) -> Self {
        Self { packages_dir }
    }

    /// Discover the CLI command for the specified Agent
    pub fn discover(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        match kind {
            ExternalAgentKind::Generic { command } => Self::discover_generic(command),
            ExternalAgentKind::Named { .. } => self.discover_named(kind),
        }
    }

    /// Discover a well-known named agent by trying executables in PATH,
    /// then the built-in packages directory, then an npx fallback.
    fn discover_named(&self, kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        let ExternalAgentKind::Named {
            id,
            executables,
            fallback_npx,
        } = kind
        else {
            unreachable!()
        };

        // 1. Try each executable in PATH
        for exe_name in executables {
            if let Some(exe) = which(exe_name) {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: exe,
                    args: vec![],
                });
            }
        }

        // 2. Try builtin packages dir
        for exe_name in executables {
            let builtin = self
                .packages_dir
                .join(id)
                .join("node_modules")
                .join(".bin")
                .join(exe_name);
            if builtin.exists() {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: builtin,
                    args: vec![],
                });
            }
        }

        // 3. Try npx fallback
        if let Some(npx_pkg) = fallback_npx {
            if let Some(npx) = which("npx") {
                return Ok(AgentCliCommand {
                    kind: kind.clone(),
                    executable: npx,
                    args: vec![npx_pkg.clone()],
                });
            }
        }

        Err(AcpError::AgentNotFound {
            kind: id.clone(),
            hint: format!("Ensure one of {:?} is in $PATH", executables),
        })
    }

    /// Generic ACP: directly use user-specified command string
    fn discover_generic(command: &str) -> Result<AgentCliCommand, AcpError> {
        let mut parts = command.split_whitespace();
        let exe_str = parts
            .next()
            .ok_or_else(|| AcpError::InvalidConfig("empty command".to_string()))?;
        let extra_args: Vec<String> = parts.map(str::to_string).collect();
        let exe = which(exe_str).ok_or_else(|| AcpError::AgentNotFound {
            kind: exe_str.to_string(),
            hint: format!("Ensure `{}` is in $PATH", exe_str),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::Generic {
                command: command.to_string(),
            },
            executable: exe,
            args: extra_args,
        })
    }
}

/// Search for executable file in system PATH
fn which(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths).find_map(|dir| {
            let full = dir.join(name);
            if full.is_file() {
                Some(full)
            } else {
                None
            }
        })
    })
}

/// Built-in packages directory (platform-specific)
fn builtin_packages_dir(app_name: &str) -> PathBuf {
    dirs::data_dir()
        .unwrap_or_else(|| {
            #[cfg(target_os = "windows")]
            {
                PathBuf::from("C:\\Temp")
            }
            #[cfg(not(target_os = "windows"))]
            {
                PathBuf::from("/tmp")
            }
        })
        .join(app_name)
        .join("packages")
}
