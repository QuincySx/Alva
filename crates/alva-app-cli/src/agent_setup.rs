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
#[allow(dead_code)] // MINI MODE:Skills 组件加回时复用
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
    // MINI MODE:暂未用(Skills/Mcp/Loader 组件加回时复用其 skills/mcp/extensions 目录)。
    _paths: &AlvaPaths,
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
    let (approval_ext, approval_rx) =
        alva_app_core::extension::ApprovalPlugin::with_channel();

    // ── MINI MODE (2026-06-15) ──────────────────────────────────────────────
    // 从"裸" BaseAgentBuilder 起步,只注册 CorePlugin(read/create/edit/list/
    // find/grep = 增改查搜)。BaseAgentBuilder 已自动装 memory + security +
    // system_context。
    //
    // ApprovalPlugin + checkpoint 是 harness substrate(REPL 硬接线的
    // approval_rx / checkpoint_mgr),保留,不算"功能组件"。
    //
    // 其余 ~20 个功能组件(Shell/Web/Task/Team/Mcp/SubAgent/Skills/Hooks/
    // 卫生 middleware/...)已移除,按优先级一点点测试加回。
    // 优先级清单见 docs/superpowers/specs/2026-06-15-cli-incremental-components.md
    let builder = BaseAgentBuilder::new()
        .workspace(workspace)
        .system_prompt(&system_prompt)
        .max_iterations(20)
        .plugin(Box::new(approval_ext))
        .plugin(Box::new(alva_app_core::extension::CorePlugin));
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
