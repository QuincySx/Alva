// Re-export from alva-acp (single source of truth for ACP discovery types)
pub use alva_acp::connection::{AgentCliCommand, AgentDiscovery, ExternalAgentKind};

// ---------------------------------------------------------------------------
// Well-known agent constructors (app-specific knowledge)
// ---------------------------------------------------------------------------

pub fn claude_code() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "claude-code".into(),
        executables: vec!["claude-code-acp".into()],
        fallback_npx: None,
    }
}

pub fn qwen_code() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "qwen-code".into(),
        executables: vec!["qwen".into()],
        fallback_npx: None,
    }
}

pub fn codex_cli() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "codex-cli".into(),
        executables: vec!["codex-acp".into()],
        fallback_npx: Some("@zed-industries/codex-acp".into()),
    }
}

pub fn gemini_cli() -> ExternalAgentKind {
    ExternalAgentKind::Named {
        id: "gemini-cli".into(),
        executables: vec!["gemini".into(), "gemini-cli".into()],
        fallback_npx: None,
    }
}
