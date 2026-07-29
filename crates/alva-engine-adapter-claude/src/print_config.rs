// INPUT:  std::path::PathBuf
// OUTPUT: ClaudeRuntimeMode, ClaudePrintConfig, ClaudePrintPermissionMode
// POS:    Light-mode (`claude -p`) configuration and full/light mode selector.

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
    /// Bypass all permission checks (Hub one-shot default — no TTY).
    #[default]
    BypassPermissions,
    /// Default CLI ask mode (will hang without a TTY / auto-deny path).
    Default,
}

impl ClaudePrintPermissionMode {
    pub fn as_cli_str(self) -> &'static str {
        match self {
            Self::AcceptEdits => "acceptEdits",
            Self::BypassPermissions => "bypassPermissions",
            Self::Default => "default",
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
    /// `--permission-mode` for non-interactive runs.
    pub permission_mode: ClaudePrintPermissionMode,
    /// Pass `--bare` (skip hooks / CLAUDE.md auto-discovery).
    ///
    /// Default **false**: `--bare` also disables OAuth/keychain auth and only
    /// accepts `ANTHROPIC_API_KEY`, which breaks "use my Claude Code login".
    /// Hosts that inject API keys and want minimal context may set this true.
    pub bare: bool,
    /// Default working directory when the request omits one.
    pub default_cwd: Option<PathBuf>,
}

impl Default for ClaudePrintConfig {
    fn default() -> Self {
        Self {
            claude_path: "claude".into(),
            model: None,
            permission_mode: ClaudePrintPermissionMode::BypassPermissions,
            bare: false,
            default_cwd: None,
        }
    }
}
