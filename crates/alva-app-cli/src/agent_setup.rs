// INPUT:  alva_app_core, alva_host_native, alva_llm_provider, alva_app_extension_loader, checkpoint
// OUTPUT: CliCheckpointCallback, load_project_context, build_agent
// POS:    Agent construction — config loading, provider wiring, checkpoint callback, project context discovery, subprocess plugin loader wiring

use std::path::Path;
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, BaseAgentBuilder};
use alva_host_native::middleware::checkpoint::CheckpointCallback;
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::LanguageModel;
use alva_llm_provider::{
    AnthropicProvider, GeminiProvider, OpenAIChatProvider, OpenAIResponsesProvider, ProviderConfig,
};
use tokio::sync::mpsc;

use crate::checkpoint;

/// CLI checkpoint callback — bridges CheckpointMiddleware to CheckpointManager.
pub(crate) struct CliCheckpointCallback {
    manager: checkpoint::CheckpointManager,
}

impl CheckpointCallback for CliCheckpointCallback {
    fn create_checkpoint(&self, description: &str, file_paths: &[std::path::PathBuf]) {
        match self.manager.create(description, file_paths) {
            Ok(id) => {
                tracing::debug!(id = %id, "auto-checkpoint created");
            }
            Err(e) => {
                tracing::warn!(error = %e, "auto-checkpoint failed");
            }
        }
    }
}

/// Resolve where the App-bundled skills tree was extracted to. Returns
/// `None` (with a logged warning) on extraction failure so the agent
/// continues without bundled skills rather than refusing to start.
pub(crate) fn bundled_skill_dir() -> Option<std::path::PathBuf> {
    match crate::bundled_skills::ensure_extracted() {
        Ok(p) => Some(p),
        Err(e) => {
            tracing::warn!(error = %e, "bundled skills extraction failed; continuing without them");
            None
        }
    }
}

/// Load project context from well-known files (AGENTS.md, CLAUDE.md, .alva/context.md).
pub(crate) fn load_project_context(workspace: &Path) -> String {
    let mut context = String::new();
    for name in &["AGENTS.md", "CLAUDE.md", ".alva/context.md"] {
        let path = workspace.join(name);
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                context.push_str(&format!(
                    "\n\n# Project Context (from {})\n\n{}",
                    name, content
                ));
            }
        }
    }
    context
}

/// Result of building the agent, bundling everything the caller needs.
pub(crate) struct AgentBundle {
    pub agent: BaseAgent,
    pub approval_rx: mpsc::UnboundedReceiver<ApprovalRequest>,
    pub checkpoint_mgr: checkpoint::CheckpointManager,
}

/// Build the agent with all wiring: provider, skills, approval channel, checkpoint callback.
pub(crate) async fn build_agent(
    config: &ProviderConfig,
    workspace: &Path,
    paths: &AlvaPaths,
) -> AgentBundle {
    let project_context = load_project_context(workspace);
    let system_prompt = format!(
        "You are a helpful coding assistant. You have access to tools for \
         running shell commands, reading/writing files, and searching code. \
         Use tools when needed to accomplish the user's task. \
         Be concise in your responses.{}",
        project_context
    );

    let model: Arc<dyn LanguageModel> = match config.kind.as_deref() {
        Some("anthropic") => Arc::new(AnthropicProvider::new(config.clone())),
        Some("openai-responses") => Arc::new(OpenAIResponsesProvider::new(config.clone())),
        Some("gemini") => Arc::new(GeminiProvider::new(config.clone())),
        // None / "openai-chat" / unknown → OpenAI Chat (broadest OpenAI-compat path).
        _ => Arc::new(OpenAIChatProvider::new(config.clone())),
    };
    let provider_registry = alva_llm_provider::build_provider_registry(config);
    let (approval_ext, approval_rx) =
        alva_app_core::extension::ApprovalExtension::with_channel();
    // Extension list is kept in lockstep with `alva-app-tauri::agent::ensure_agent`.
    // Same defaults, same kernel surface, same iteration cap — only storage
    // (JSON vs SQLite) and UI shell legitimately differ between the two apps.
    let builder = BaseAgentBuilder::new()
        .workspace(workspace)
        .system_prompt(&system_prompt)
        .max_iterations(20)
        .plugin(Box::new(
            alva_app_core::extension::ProviderRegistryExtension::new(provider_registry),
        ))
        .plugin(Box::new(
            alva_app_core::extension::ToolLockRegistryExtension::new(),
        ))
        .plugin(Box::new(
            alva_app_core::extension::AnalyticsExtension::new(),
        ))
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(alva_app_core::extension::SkillsExtension::with_bundled(
            paths.project_skills_dir(),
            bundled_skill_dir(),
        )))
        .plugin(Box::new(alva_app_core::extension::CoreExtension))
        .plugin(Box::new(alva_app_core::extension::ShellExtension))
        .plugin(Box::new(alva_app_core::extension::InteractionExtension))
        .plugin(Box::new(alva_app_core::extension::TaskExtension::default()))
        .plugin(Box::new(alva_app_core::extension::TeamExtension::default()))
        .extension(Box::new(alva_app_core::extension::PlanningExtension))
        .plugin(Box::new(alva_app_core::extension::UtilityExtension))
        .plugin(Box::new(alva_app_core::extension::WebExtension))
        .plugin(Box::new(alva_app_core::extension::BrowserExtension))
        .extension(Box::new(alva_app_core::extension::LoopDetectionExtension))
        .extension(Box::new(alva_app_core::extension::DanglingToolCallExtension))
        .extension(Box::new(alva_app_core::extension::ToolTimeoutExtension))
        .extension(Box::new(alva_app_core::extension::CompactionExtension))
        .extension(Box::new(alva_app_core::extension::CheckpointExtension))
        .plugin(Box::new(alva_app_core::extension::PermissionExtension::new()))
        .plugin(Box::new(alva_app_core::extension::SubAgentExtension::new(3)))
        .plugin(Box::new(alva_app_core::extension::McpExtension::new(vec![
            paths.global_mcp_config(),
            paths.project_mcp_config(),
        ])))
        .extension(Box::new(alva_app_core::extension::HooksExtension::new(
            alva_app_core::settings::HooksSettings::default(),
        )))
        // Third-party subprocess plugins (JS / Python / anything).
        // Project dir shadows global on name conflicts — same convention
        // as skills and MCP configs above.
        .extension(Box::new(
            alva_app_extension_loader::loader::SubprocessLoaderExtension::new(vec![
                paths.project_extensions_dir(),
                paths.global_extensions_dir(),
            ]),
        ));
    let agent = builder.build(model).await.expect("failed to build agent");

    // Register checkpoint callback
    let checkpoint_mgr = checkpoint::CheckpointManager::new(workspace);
    agent
        .set_checkpoint_callback(Arc::new(CliCheckpointCallback {
            manager: checkpoint::CheckpointManager::new(workspace),
        }));

    AgentBundle {
        agent,
        approval_rx,
        checkpoint_mgr,
    }
}

#[cfg(test)]
mod tests {
    //! Tests for `load_project_context` — scans 3 well-known files
    //! (AGENTS.md, CLAUDE.md, .alva/context.md) and concatenates any
    //! present with a header. Drives the project-context portion of
    //! the system prompt, so missing or out-of-order content directly
    //! degrades agent response quality.
    use super::*;

    #[test]
    fn load_project_context_empty_dir_returns_empty_string() {
        let tmp = tempfile::TempDir::new().unwrap();
        assert_eq!(load_project_context(tmp.path()), "");
    }

    #[test]
    fn load_project_context_single_file_emitted_with_header() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "agents content").unwrap();
        let out = load_project_context(tmp.path());
        assert!(out.contains("# Project Context (from AGENTS.md)"));
        assert!(out.contains("agents content"));
        assert!(out.starts_with("\n\n"), "leading separator preserved: {:?}", &out[..8]);
    }

    #[test]
    fn load_project_context_all_three_files_concatenated_in_declared_order() {
        // Order matters: code iterates &["AGENTS.md", "CLAUDE.md",
        // ".alva/context.md"], so AGENTS appears before CLAUDE which
        // appears before .alva/context.md regardless of any other
        // ordering (filesystem mtime etc.).
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "A_BODY").unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "C_BODY").unwrap();
        std::fs::create_dir_all(tmp.path().join(".alva")).unwrap();
        std::fs::write(tmp.path().join(".alva/context.md"), "ALVA_BODY").unwrap();

        let out = load_project_context(tmp.path());
        let agents_pos = out.find("A_BODY").expect("AGENTS body present");
        let claude_pos = out.find("C_BODY").expect("CLAUDE body present");
        let alva_pos = out.find("ALVA_BODY").expect(".alva body present");
        assert!(agents_pos < claude_pos, "AGENTS before CLAUDE");
        assert!(claude_pos < alva_pos, "CLAUDE before .alva/context.md");
        // All three headers present
        assert!(out.contains("(from AGENTS.md)"));
        assert!(out.contains("(from CLAUDE.md)"));
        assert!(out.contains("(from .alva/context.md)"));
    }

    #[test]
    fn load_project_context_only_claude_md_skips_missing_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("CLAUDE.md"), "just claude").unwrap();
        let out = load_project_context(tmp.path());
        assert!(out.contains("(from CLAUDE.md)"));
        assert!(out.contains("just claude"));
        assert!(!out.contains("(from AGENTS.md)"));
        assert!(!out.contains("(from .alva/context.md)"));
    }

    #[test]
    fn load_project_context_nested_alva_context_md_resolved_correctly() {
        // The third file lives at `.alva/context.md` — a nested path.
        // Verify it's read via `workspace.join(".alva/context.md")`
        // rather than treating ".alva/context.md" as a single filename.
        let tmp = tempfile::TempDir::new().unwrap();
        // Without the parent dir, write would fail and the file
        // wouldn't load. Create both.
        std::fs::create_dir_all(tmp.path().join(".alva")).unwrap();
        std::fs::write(tmp.path().join(".alva/context.md"), "nested-only").unwrap();
        let out = load_project_context(tmp.path());
        assert!(out.contains("nested-only"));
        assert!(out.contains("(from .alva/context.md)"));
    }

    #[test]
    fn load_project_context_empty_file_still_emits_header() {
        // Edge case: file exists but body is empty. The header still
        // appears (current behavior), so the caller can tell the file
        // was present but empty rather than absent.
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::write(tmp.path().join("AGENTS.md"), "").unwrap();
        let out = load_project_context(tmp.path());
        assert!(out.contains("(from AGENTS.md)"));
        // Body after the header is empty — verify there isn't garbage.
        let after_header = out.split("(from AGENTS.md)").nth(1).unwrap();
        assert!(after_header.trim().is_empty());
    }
}
