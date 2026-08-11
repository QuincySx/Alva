// INPUT:  std::path::PathBuf
// OUTPUT: CodexAdapterConfig, CodexSandboxMode
// POS:    Configuration for spawning `codex exec --json` as a Hub engine.

use std::collections::HashMap;
use std::path::PathBuf;

/// Sandbox policy forwarded to `codex exec -s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CodexSandboxMode {
    /// Default for Hub one-shot: write inside workspace only.
    #[default]
    WorkspaceWrite,
    ReadOnly,
    DangerFullAccess,
}

impl CodexSandboxMode {
    pub fn as_cli_str(self) -> &'static str {
        match self {
            Self::WorkspaceWrite => "workspace-write",
            Self::ReadOnly => "read-only",
            Self::DangerFullAccess => "danger-full-access",
        }
    }
}

/// How the adapter launches Codex.
#[derive(Debug, Clone)]
pub struct CodexAdapterConfig {
    /// Path to the `codex` binary (default: `codex` on PATH).
    pub codex_path: String,
    /// Optional model override (`-m`).
    pub model: Option<String>,
    /// Sandbox mode for `codex exec -s`.
    pub sandbox: CodexSandboxMode,
    /// When true, pass `--dangerously-bypass-approvals-and-sandbox`.
    /// Only for already-externally-sandboxed automation.
    pub bypass_approvals_and_sandbox: bool,
    /// Extra `-c key=value` overrides (raw strings after `=`).
    pub config_overrides: Vec<(String, String)>,
    /// Default working directory when the request omits one.
    pub default_cwd: Option<PathBuf>,
    /// Additional child environment, primarily provider API-key indirection.
    pub env: HashMap<String, String>,
}

impl Default for CodexAdapterConfig {
    fn default() -> Self {
        Self {
            codex_path: "codex".into(),
            model: None,
            sandbox: CodexSandboxMode::WorkspaceWrite,
            bypass_approvals_and_sandbox: false,
            // Non-interactive Hub runs must not hang on approval prompts.
            config_overrides: vec![("approval_policy".into(), "\"never\"".into())],
            default_cwd: None,
            env: HashMap::new(),
        }
    }
}
