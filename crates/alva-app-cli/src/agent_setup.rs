// INPUT:  alva_app_core, alva_host_native, alva_llm_provider, checkpoint
// OUTPUT: CliCheckpointCallback, load_project_context, build_agent
// POS:    Agent construction — config loading, provider wiring, checkpoint callback, and project context discovery

use std::path::Path;
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, BaseAgentBuilder};
use alva_host_native::middleware::checkpoint::CheckpointCallback;
use alva_host_native::middleware::ApprovalRequest;
use alva_llm_provider::{OpenAIChatProvider, ProviderConfig};
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

    let model = Arc::new(OpenAIChatProvider::new(config.clone()));
    let mut builder = BaseAgentBuilder::new()
        .workspace(workspace)
        .system_prompt(&system_prompt)
        .extension(Box::new(alva_app_core::extension::SkillsExtension::new(vec![
            paths.project_skills_dir(),
            paths.global_skills_dir(),
        ])))
        .extension(Box::new(alva_app_core::extension::CoreExtension))
        .extension(Box::new(alva_app_core::extension::ShellExtension))
        .extension(Box::new(alva_app_core::extension::InteractionExtension))
        .extension(Box::new(alva_app_core::extension::TaskExtension))
        .extension(Box::new(alva_app_core::extension::TeamExtension))
        .extension(Box::new(alva_app_core::extension::PlanningExtension))
        .extension(Box::new(alva_app_core::extension::UtilityExtension))
        .extension(Box::new(alva_app_core::extension::WebExtension))
        .extension(Box::new(alva_app_core::extension::LoopDetectionExtension))
        .extension(Box::new(alva_app_core::extension::DanglingToolCallExtension))
        .extension(Box::new(alva_app_core::extension::ToolTimeoutExtension))
        .extension(Box::new(alva_app_core::extension::CompactionExtension))
        .extension(Box::new(alva_app_core::extension::CheckpointExtension))
        .extension(Box::new(alva_app_core::extension::PlanModeExtension::new()))
        .extension(Box::new(alva_app_core::extension::SubAgentExtension::new(3)))
        .extension(Box::new(alva_app_core::extension::McpExtension::new(vec![
            paths.global_mcp_config(),
            paths.project_mcp_config(),
        ])))
        .extension(Box::new(alva_app_core::extension::HooksExtension::new(
            alva_app_core::settings::HooksSettings::default(),
        )));
    let approval_rx = builder.with_approval_channel();
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
