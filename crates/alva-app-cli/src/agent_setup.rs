// INPUT:  alva_app_core, alva_host_native, alva_llm_provider, alva_app_extension_loader, checkpoint, optional job_log middleware
// OUTPUT: bounded workspace-only project context, fallible AgentBundle construction
// POS:    Agent construction — config loading, provider wiring, checkpoint callback, secure project context discovery, subprocess plugin loader wiring

use std::io::Read as _;
use std::path::Path;
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, BaseAgentBuilder, EngineError};
use alva_host_native::middleware::ApprovalRequest;
use alva_kernel_abi::LanguageModel;
use alva_llm_provider::ProviderConfig;
use tokio::sync::mpsc;

use crate::checkpoint;
use crate::job_log::{JobToolLogMiddleware, JobToolLogger};

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

const MAX_PROJECT_CONTEXT_FILE_BYTES: u64 = 1024 * 1024;

/// Load project context from well-known files (AGENTS.md chain, CLAUDE.md,
/// .alva/context.md).
///
/// Context files must resolve inside the canonical workspace. This prevents a
/// repository-controlled symlink from copying an arbitrary host file into the
/// model prompt. Each file is also bounded so a malformed repository cannot
/// exhaust memory or consume the entire context window during startup.
///
/// `AGENTS.md` is collected from the workspace root **and** nested paths when
/// the process cwd sits under the workspace (outer → inner, nearest last).
pub(crate) fn load_project_context(workspace: &Path) -> String {
    let canonical_workspace = match workspace.canonicalize() {
        Ok(path) => path,
        Err(error) => {
            tracing::warn!(path = ?workspace, %error, "cannot resolve workspace for project context");
            return String::new();
        }
    };
    let mut context = String::new();
    let mut seen = std::collections::HashSet::new();

    // AGENTS.md chain: prefer cwd when it is inside the workspace (monorepo
    // nearest-wins by concatenating outer → inner). Fall back to workspace root.
    let agents_start = std::env::current_dir()
        .ok()
        .and_then(|cwd| cwd.canonicalize().ok())
        .filter(|cwd| cwd.starts_with(&canonical_workspace))
        .unwrap_or_else(|| canonical_workspace.clone());
    for path in alva_app_core::paths::collect_agents_md_chain(&agents_start, &canonical_workspace) {
        append_context_file(
            &mut context,
            &mut seen,
            &canonical_workspace,
            &path,
            "AGENTS.md",
        );
    }

    for name in &["CLAUDE.md", ".alva/context.md"] {
        let path = workspace.join(name);
        let Ok(canonical_path) = path.canonicalize() else {
            continue;
        };
        append_context_file(
            &mut context,
            &mut seen,
            &canonical_workspace,
            &canonical_path,
            name,
        );
    }
    context
}

fn append_context_file(
    context: &mut String,
    seen: &mut std::collections::HashSet<std::path::PathBuf>,
    canonical_workspace: &Path,
    canonical_path: &Path,
    label: &str,
) {
    if !canonical_path.starts_with(canonical_workspace) || !canonical_path.is_file() {
        tracing::warn!(
            path = ?canonical_path,
            "skipping project context outside workspace"
        );
        return;
    }
    if !seen.insert(canonical_path.to_path_buf()) {
        return;
    }

    let Ok(file) = std::fs::File::open(canonical_path) else {
        return;
    };
    let mut bytes = Vec::new();
    if file
        .take(MAX_PROJECT_CONTEXT_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .is_err()
    {
        return;
    }
    let truncated = bytes.len() as u64 > MAX_PROJECT_CONTEXT_FILE_BYTES;
    bytes.truncate(MAX_PROJECT_CONTEXT_FILE_BYTES as usize);
    let content = String::from_utf8_lossy(&bytes);
    let display = canonical_path
        .strip_prefix(canonical_workspace)
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| label.to_string());
    context.push_str(&format!(
        "\n\n# Project Context (from {})\n\n{}{}",
        display,
        content,
        if truncated {
            "\n\n[project context truncated at 1 MiB]"
        } else {
            ""
        }
    ));
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
    // `--system-prompt` (headless v2): replaces the default persona so an
    // orchestrator fully owns the worker's role. Project context (AGENTS.md
    // etc.) is still appended — the worker runs in this workspace either way.
    system_prompt_override: Option<&str>,
    // `--max-turns` (recursion-gate wave): per-invocation turn budget so an
    // orchestrator can bound a cheap-model worker. None -> the default 20.
    max_turns: Option<u32>,
) -> Result<AgentBundle, EngineError> {
    let project_context = load_project_context(workspace);
    let base_prompt = system_prompt_override.unwrap_or(
        "You are a helpful coding assistant. You have access to tools for \
         running shell commands, reading/writing files, and searching code. \
         Use tools when needed to accomplish the user's task. \
         Be concise in your responses.",
    );
    let system_prompt = format!("{base_prompt}{project_context}");

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
    let shared_cfg = alva_app_core::config::load();
    let toggles: alva_app_core::components::ComponentToggles = shared_cfg
        .as_ref()
        .map(|c| c.components.clone())
        .unwrap_or_default();
    let ctx = alva_app_core::components::ComponentContext {
        workspace: workspace.to_path_buf(),
        provider_registry: Some(provider_registry),
        skills: Some(alva_app_core::components::SkillsDiscovery::from_alva_paths(
            paths,
            bundled_skill_dir(),
        )),
        mcp_config_paths: paths.mcp_discovery_paths(),
        // ONE knob for both recursion forms — the same config value also
        // drives main.rs's cross-process ALVA_AGENT_DEPTH gate.
        subagent_depth: shared_cfg
            .as_ref()
            .and_then(|c| c.subagent_depth)
            .unwrap_or(alva_app_core::components::DEFAULT_SUBAGENT_DEPTH),
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
    let mut builder = alva_app_core::components::apply_components(
        BaseAgentBuilder::new()
            .workspace(workspace)
            .system_prompt(&system_prompt)
            .max_iterations(max_turns.unwrap_or(20))
            .plugin(Box::new(approval_ext)), // substrate: added manually before components
        &toggles,
        &ctx,
    );
    if let Some(logger) = JobToolLogger::from_env() {
        builder = builder.middleware(Arc::new(JobToolLogMiddleware::new(logger)));
    }
    let agent = builder.build(model).await?;

    // Register checkpoint callback
    let checkpoint_mgr = checkpoint::CheckpointManager::new(workspace);
    agent.set_checkpoint_callback(Arc::new(
        alva_app_core::checkpoint::ManagerCheckpointCallback::new(
            checkpoint::CheckpointManager::new(workspace),
        ),
    ));

    Ok(AgentBundle {
        agent,
        approval_rx,
        checkpoint_mgr,
    })
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

    #[cfg(unix)]
    #[test]
    fn load_project_context_rejects_symlink_outside_workspace() {
        use std::os::unix::fs::symlink;

        let tmp = tempfile::TempDir::new().unwrap();
        let outside = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(outside.path(), "host-secret-that-must-not-be-loaded").unwrap();
        symlink(outside.path(), tmp.path().join("AGENTS.md")).unwrap();

        let out = load_project_context(tmp.path());
        assert!(!out.contains("host-secret-that-must-not-be-loaded"));
        assert!(!out.contains("(from AGENTS.md)"));
    }

    #[test]
    fn load_project_context_bounds_oversized_files() {
        let tmp = tempfile::TempDir::new().unwrap();
        let oversized = "x".repeat(MAX_PROJECT_CONTEXT_FILE_BYTES as usize + 32);
        std::fs::write(tmp.path().join("AGENTS.md"), oversized).unwrap();

        let out = load_project_context(tmp.path());
        assert!(out.contains("[project context truncated at 1 MiB]"));
        assert!(
            out.len() < MAX_PROJECT_CONTEXT_FILE_BYTES as usize + 256,
            "bounded context unexpectedly large: {} bytes",
            out.len()
        );
    }
}
