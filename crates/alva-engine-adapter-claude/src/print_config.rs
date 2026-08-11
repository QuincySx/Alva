// INPUT:  std::path::PathBuf
// OUTPUT: ClaudeRuntimeMode, ClaudePrintConfig, ClaudePrintPermissionMode
// POS:    Light-mode (`claude -p`) configuration and full/light mode selector.

use std::collections::HashMap;
use std::path::PathBuf;

/// How Hub (or any host) should run the Claude engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudeRuntimeMode {
    /// Full harness: Node bridge + `@anthropic-ai/claude-agent-sdk` (cindy path).
    /// Interactive permissions, richer resume, mid-session controls.
    Full,
    /// Light / exec-style: `claude -p --output-format stream-json` (Codex-exec analogue).
    /// Needs only the `claude` binary on PATH; no npm SDK package.
    #[default]
    Light,
}

impl ClaudeRuntimeMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Light => "light",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "full" | "sdk" => Some(Self::Full),
            "light" | "print" | "cli" | "p" => Some(Self::Light),
            _ => None,
        }
    }
}

/// Permission mode for non-interactive `claude -p` runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ClaudePrintPermissionMode {
    /// Accept file edits without prompting; other tools may still ask.
    AcceptEdits,
    /// Permit tools automatically according to Claude Code's `auto` policy.
    Auto,
    /// Bypass all permission checks. This is an explicit host opt-in only.
    BypassPermissions,
    /// Require manual approval for each permission request.
    Manual,
    /// Reject permission requests rather than prompting without a TTY.
    #[default]
    DontAsk,
    /// Restrict Claude Code to plan-mode operations.
    Plan,
}

impl ClaudePrintPermissionMode {
    pub fn as_cli_str(self) -> &'static str {
        match self {
            Self::AcceptEdits => "acceptEdits",
            Self::Auto => "auto",
            Self::BypassPermissions => "bypassPermissions",
            Self::Manual => "manual",
            Self::DontAsk => "dontAsk",
            Self::Plan => "plan",
        }
    }
}

/// Configuration for light-mode Claude CLI adapter.
#[derive(Debug, Clone)]
pub struct ClaudePrintConfig {
    /// Path to the `claude` binary (default: `claude` on PATH).
    pub claude_path: String,
    /// Optional `--model` override.
    pub model: Option<String>,
    /// Optional `--effort` override (`low`, `medium`, `high`, `xhigh`, `max`).
    pub effort: Option<String>,
    /// `--permission-mode` for non-interactive runs.
    pub permission_mode: ClaudePrintPermissionMode,
    /// Pass `--safe-mode` to disable workspace customizations while preserving
    /// Claude Code authentication, model selection, built-ins, and permissions.
    pub safe_mode: bool,
    /// Default working directory when the request omits one.
    pub default_cwd: Option<PathBuf>,
    /// Additional child environment for custom API-compatible providers.
    pub env: HashMap<String, String>,
}

impl Default for ClaudePrintConfig {
    fn default() -> Self {
        Self {
            claude_path: "claude".into(),
            model: None,
            effort: None,
            permission_mode: ClaudePrintPermissionMode::DontAsk,
            safe_mode: true,
            default_cwd: None,
            env: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ClaudePrintPermissionMode;

    #[test]
    fn permission_modes_match_current_claude_cli_values() {
        assert_eq!(
            [
                ClaudePrintPermissionMode::AcceptEdits.as_cli_str(),
                ClaudePrintPermissionMode::Auto.as_cli_str(),
                ClaudePrintPermissionMode::BypassPermissions.as_cli_str(),
                ClaudePrintPermissionMode::Manual.as_cli_str(),
                ClaudePrintPermissionMode::DontAsk.as_cli_str(),
                ClaudePrintPermissionMode::Plan.as_cli_str(),
            ],
            [
                "acceptEdits",
                "auto",
                "bypassPermissions",
                "manual",
                "dontAsk",
                "plan",
            ]
        );
    }
}
