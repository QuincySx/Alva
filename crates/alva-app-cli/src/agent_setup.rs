// INPUT:  alva_app_core, alva_agent_runtime, alva_provider, checkpoint
// OUTPUT: CliCheckpointCallback, load_project_context, build_agent
// POS:    Agent construction — config loading, provider wiring, checkpoint callback, and project context discovery

use std::path::Path;
use std::sync::Arc;

use alva_app_core::{AlvaPaths, BaseAgent, BaseAgentBuilder};
use alva_agent_runtime::middleware::checkpoint::CheckpointCallback;
use alva_agent_runtime::middleware::security::ApprovalRequest;
use alva_provider::{OpenAIProvider, ProviderConfig};
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

    let model = Arc::new(OpenAIProvider::new(config.clone()));
    let mut builder = BaseAgentBuilder::new()
        .workspace(workspace)
        .system_prompt(&system_prompt)
        .skill_dir(paths.project_skills_dir())
        .skill_dir(paths.global_skills_dir())
        .without_browser()
        .with_sub_agents()
        .sub_agent_max_depth(3);
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
