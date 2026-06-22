//! HooksPlugin — runs shell-script hooks as phase handlers at PreToolUse,
//! PostToolUse, SessionStart, and SessionEnd events.

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;

use alva_agent_core::extension::{PhaseContribution, PhaseHandler, PhaseOrder};
use alva_kernel_abi::tool::execution::ToolOutput;
use alva_kernel_abi::ToolCall;
use alva_kernel_abi::{Phase, PhaseEffect};
use alva_kernel_core::shared::MiddlewareError;
use alva_kernel_core::state::AgentState;

use crate::extension::hooks::{HookEvent, HookExecutor, HookInput};
use crate::extension::{Plugin, Registrar};
use crate::settings::HooksSettings;

/// Lifecycle hooks as middleware — runs shell scripts at PreToolUse, PostToolUse,
/// SessionStart, and SessionEnd events.
pub struct HooksPlugin {
    settings: HooksSettings,
}

impl HooksPlugin {
    pub fn new(settings: HooksSettings) -> Self {
        Self { settings }
    }
}

#[async_trait]
impl Plugin for HooksPlugin {
    fn name(&self) -> &str {
        "hooks"
    }
    fn description(&self) -> &str {
        "Lifecycle hooks (shell scripts at tool/session events)"
    }

    async fn register(&self, r: &Registrar) {
        let runtime = Arc::new(HooksRuntime {
            settings: self.settings.clone(),
            workspace: r.workspace().to_path_buf(),
        });
        for contribution in [
            PhaseContribution::new(
                "hooks-session-start",
                Phase::RunStart,
                PhaseEffect::Decide,
                PhaseOrder::Hooks,
            ),
            PhaseContribution::new(
                "hooks-session-end",
                Phase::RunEnd,
                PhaseEffect::Observe,
                PhaseOrder::Hooks,
            ),
            PhaseContribution::new(
                "hooks-pre-tool-use",
                Phase::BeforeToolCall,
                PhaseEffect::Decide,
                PhaseOrder::Hooks,
            ),
            PhaseContribution::new(
                "hooks-post-tool-use",
                Phase::AfterToolCall,
                PhaseEffect::Observe,
                PhaseOrder::Hooks,
            ),
        ] {
            r.phase_handler(Arc::new(HooksPhaseHandler {
                contribution,
                runtime: runtime.clone(),
            }));
        }
    }
}

struct HooksRuntime {
    settings: HooksSettings,
    workspace: PathBuf,
}

impl HooksRuntime {
    async fn session_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        let session_id = state.session.session_id();
        let executor = HookExecutor::new(&self.workspace, session_id);
        let input = HookInput::lifecycle(HookEvent::SessionStart, session_id, &self.workspace);
        let result = executor
            .run(&self.settings, HookEvent::SessionStart, None, input)
            .await;
        if result.is_blocked() {
            return Err(MiddlewareError::Blocked {
                reason: result.blocking_messages().join("; "),
            });
        }
        Ok(())
    }

    async fn session_end(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        let session_id = state.session.session_id();
        let executor = HookExecutor::new(&self.workspace, session_id);
        let input = HookInput::lifecycle(HookEvent::SessionEnd, session_id, &self.workspace);
        let _ = executor
            .run(&self.settings, HookEvent::SessionEnd, None, input)
            .await;
        Ok(())
    }

    async fn pre_tool_use(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        let session_id = state.session.session_id();
        let executor = HookExecutor::new(&self.workspace, session_id);
        let input = HookInput::pre_tool_use(
            &tool_call.name,
            tool_call.arguments.clone(),
            session_id,
            &self.workspace,
        );
        let result = executor
            .run(
                &self.settings,
                HookEvent::PreToolUse,
                Some(&tool_call.name),
                input,
            )
            .await;
        if result.is_blocked() {
            return Err(MiddlewareError::Blocked {
                reason: result.blocking_messages().join("; "),
            });
        }
        Ok(())
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        tool_output: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        let session_id = state.session.session_id();
        let executor = HookExecutor::new(&self.workspace, session_id);
        let response_text = tool_output.model_text();
        let input = HookInput::post_tool_use(
            &tool_call.name,
            tool_call.arguments.clone(),
            &response_text,
            session_id,
            &self.workspace,
        );
        let _ = executor
            .run(
                &self.settings,
                HookEvent::PostToolUse,
                Some(&tool_call.name),
                input,
            )
            .await;
        Ok(())
    }
}

struct HooksPhaseHandler {
    contribution: PhaseContribution,
    runtime: Arc<HooksRuntime>,
}

#[async_trait]
impl PhaseHandler for HooksPhaseHandler {
    fn contribution(&self) -> PhaseContribution {
        self.contribution.clone()
    }

    async fn run_start(&self, state: &mut AgentState) -> Result<(), MiddlewareError> {
        self.runtime.session_start(state).await
    }

    async fn run_end(
        &self,
        state: &mut AgentState,
        _error: Option<&str>,
    ) -> Result<(), MiddlewareError> {
        self.runtime.session_end(state).await
    }

    async fn before_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        self.runtime.pre_tool_use(state, tool_call).await
    }

    async fn after_tool_call(
        &self,
        state: &mut AgentState,
        tool_call: &ToolCall,
        result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        self.runtime.after_tool_call(state, tool_call, result).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn plugin_registers_lifecycle_hooks_as_phase_handlers() {
        let dir = tempfile::tempdir().expect("tempdir");
        let model = Arc::new(alva_test::mock_provider::MockLanguageModel::new());

        let agent = alva_agent_core::Agent::builder()
            .workspace(dir.path())
            .model(model)
            .plugin(Box::new(HooksPlugin::new(HooksSettings::default())))
            .build()
            .await
            .expect("agent should build");

        let snapshot = agent.assembly_snapshot();
        let plugin = snapshot
            .plugins
            .iter()
            .find(|plugin| plugin.name == "hooks")
            .expect("hooks plugin snapshot");

        assert_eq!(
            plugin.phase_contribution_names,
            vec![
                "hooks-session-start",
                "hooks-session-end",
                "hooks-pre-tool-use",
                "hooks-post-tool-use",
            ]
        );
        assert!(
            !plugin.middleware_names.iter().any(|name| name == "hooks"),
            "hooks should register semantic phase handlers, not a raw middleware"
        );
        for name in [
            "phase:hooks-session-start",
            "phase:hooks-session-end",
            "phase:hooks-pre-tool-use",
            "phase:hooks-post-tool-use",
        ] {
            assert!(
                snapshot
                    .middleware_names
                    .iter()
                    .any(|middleware| middleware == name),
                "missing compiled phase handler {name}: {:?}",
                snapshot.middleware_names
            );
        }
    }
}
