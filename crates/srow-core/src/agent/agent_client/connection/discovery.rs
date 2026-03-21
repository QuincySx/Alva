// INPUT:  std::path, crate::agent::agent_client::AcpError, dirs
// OUTPUT: ExternalAgentKind, AgentCliCommand, AgentDiscovery
// POS:    Discovers external Agent CLI executables (Claude Code, Qwen, Codex, Gemini) by searching PATH and built-in locations.
use std::path::PathBuf;

use crate::agent::agent_client::AcpError;

/// Supported external Agent types
#[derive(Debug, Clone, PartialEq)]
pub enum ExternalAgentKind {
    /// Claude Code (claude-code-acp)
    ClaudeCode,
    /// Qwen Code
    QwenCode,
    /// Codex CLI (Zed Industries)
    CodexCli,
    /// Gemini CLI
    GeminiCli,
    /// Generic ACP (any CLI implementing the protocol, user-defined command)
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

pub struct AgentDiscovery;

impl AgentDiscovery {
    /// Discover the CLI command for the specified Agent
    pub fn discover(kind: &ExternalAgentKind) -> Result<AgentCliCommand, AcpError> {
        match kind {
            ExternalAgentKind::ClaudeCode => Self::discover_claude_code(),
            ExternalAgentKind::QwenCode => Self::discover_qwen_code(),
            ExternalAgentKind::CodexCli => Self::discover_codex_cli(),
            ExternalAgentKind::GeminiCli => Self::discover_gemini_cli(),
            ExternalAgentKind::Generic { command } => Self::discover_generic(command),
        }
    }

    /// Claude Code: PATH lookup for `claude-code-acp`
    fn discover_claude_code() -> Result<AgentCliCommand, AcpError> {
        let exe = which("claude-code-acp").ok_or_else(|| AcpError::AgentNotFound {
            kind: "claude-code-acp".to_string(),
            hint: "Install Claude Code and ensure `claude-code-acp` is in $PATH".to_string(),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::ClaudeCode,
            executable: exe,
            args: vec![],
        })
    }

    /// Qwen Code:
    ///   1. PATH lookup for `qwen`
    ///   2. Built-in path $APP_DATA/packages/qwen/node_modules/.bin/qwen
    fn discover_qwen_code() -> Result<AgentCliCommand, AcpError> {
        if let Some(exe) = which("qwen") {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::QwenCode,
                executable: exe,
                args: vec![],
            });
        }
        let builtin = builtin_packages_dir()
            .join("qwen")
            .join("node_modules")
            .join(".bin")
            .join("qwen");
        if builtin.exists() {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::QwenCode,
                executable: builtin,
                args: vec![],
            });
        }
        Err(AcpError::AgentNotFound {
            kind: "qwen".to_string(),
            hint: "Install Qwen Code CLI or place the package in the built-in packages directory"
                .to_string(),
        })
    }

    /// Codex CLI:
    ///   1. PATH lookup for `codex-acp`
    ///   2. `npx @zed-industries/codex-acp` fallback
    fn discover_codex_cli() -> Result<AgentCliCommand, AcpError> {
        if let Some(exe) = which("codex-acp") {
            return Ok(AgentCliCommand {
                kind: ExternalAgentKind::CodexCli,
                executable: exe,
                args: vec![],
            });
        }
        let npx = which("npx").ok_or_else(|| AcpError::AgentNotFound {
            kind: "codex-acp".to_string(),
            hint: "Install Node.js/npx or `codex-acp` binary in $PATH".to_string(),
        })?;
        Ok(AgentCliCommand {
            kind: ExternalAgentKind::CodexCli,
            executable: npx,
            args: vec!["@zed-industries/codex-acp".to_string()],
        })
    }

    /// Gemini CLI: PATH lookup for `gemini` or `gemini-cli`
    fn discover_gemini_cli() -> Result<AgentCliCommand, AcpError> {
        for name in &["gemini", "gemini-cli"] {
            if let Some(exe) = which(name) {
                return Ok(AgentCliCommand {
                    kind: ExternalAgentKind::GeminiCli,
                    executable: exe,
                    args: vec![],
                });
            }
        }
        Err(AcpError::AgentNotFound {
            kind: "gemini-cli".to_string(),
            hint: "Install Gemini CLI and ensure it is in $PATH".to_string(),
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
fn builtin_packages_dir() -> PathBuf {
    #[cfg(target_os = "macos")]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("srow-agent")
            .join("packages")
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("C:\\Temp"))
            .join("srow-agent")
            .join("packages")
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::data_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("srow-agent")
            .join("packages")
    }
}
