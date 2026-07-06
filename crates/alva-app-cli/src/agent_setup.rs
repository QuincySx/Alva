// INPUT:  alva_app_core, alva_host_native, alva_llm_provider, alva_app_extension_loader, checkpoint
// OUTPUT: CliCheckpointCallback, load_project_context, build_agent
// POS:    Agent construction — config loading, provider wiring, checkpoint callback, project context discovery, subprocess plugin loader wiring

use std::path::Path;
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, BaseAgentBuilder};
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::LanguageModel;
use alva_llm_provider::ProviderConfig;
use tokio::sync::mpsc;

use crate::checkpoint;

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
    // P3:SkillsPlugin 用到 paths.project_skills_dir()(Mcp/Loader 组件加回时复用其 mcp/extensions 目录)。
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

    // Single kind→provider switch lives in alva-llm-provider (PR-10).
    let model: Arc<dyn LanguageModel> =
        alva_llm_provider::build_language_model(config.kind.as_deref(), config.clone());
    // Provider registry — lets SubAgent/Task spawn against named providers.
    let provider_registry = alva_llm_provider::build_provider_registry(config);

    // ── Substrate (always wired by the harness, NOT a toggleable component) ──
    // ApprovalPlugin yields `approval_rx` that the REPL hard-wires; the
    // checkpoint *callback* (CheckpointManager) is wired at the end. These are
    // distinct from the toggleable CheckpointMiddleware auto-archiver.
    let (approval_ext, approval_rx) = alva_app_core::extension::ApprovalPlugin::with_channel();

    // ── Component assembly (Stage B) ────────────────────────────────────────
    // The flat catalog + `apply_components` is now the single assembly truth
    // (shared with Tauri in Stage C). Toggles come from `~/.alva/config.json`'s
    // `components` map (absent id → that component's `default_on`).
    let toggles: alva_app_core::components::ComponentToggles = alva_app_core::config::load()
        .map(|c| c.components)
        .unwrap_or_default();
    let ctx = alva_app_core::components::ComponentContext {
        workspace: workspace.to_path_buf(),
        provider_registry: Some(provider_registry),
        skills: Some((paths.project_skills_dir(), bundled_skill_dir())),
        mcp_config_paths: vec![paths.global_mcp_config(), paths.project_mcp_config()],
        subagent_depth: 3,
        subagent_timeout: alva_app_core::components::DEFAULT_SUBAGENT_TIMEOUT,
        subagent_tool_timeout: alva_app_core::components::DEFAULT_SUBAGENT_TOOL_TIMEOUT,
        // Built-in templates (e.g. `video`) + any user/project agents.toml.
        agent_templates: alva_app_core::extension::agent_templates::resolve_agent_templates(&[
            paths.global_agents_config(),
            paths.project_agents_config(),
        ]),
        hooks_settings: alva_app_core::settings::HooksSettings::default(),
        subprocess_ext_dirs: vec![
            paths.project_extensions_dir(),
            paths.global_extensions_dir(),
        ],
    };
    let builder = alva_app_core::components::apply_components(
        BaseAgentBuilder::new()
            .workspace(workspace)
            .system_prompt(&system_prompt)
            .max_iterations(20)
            .plugin(Box::new(approval_ext)), // substrate: added manually before components
        &toggles,
        &ctx,
    );
    let agent = builder.build(model).await.expect("failed to build agent");

    // Register checkpoint callback
    let checkpoint_mgr = checkpoint::CheckpointManager::new(workspace);
    agent.set_checkpoint_callback(Arc::new(
        alva_app_core::checkpoint::ManagerCheckpointCallback::new(
            checkpoint::CheckpointManager::new(workspace),
        ),
    ));

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
        assert!(
            out.starts_with("\n\n"),
            "leading separator preserved: {:?}",
            &out[..8]
        );
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
