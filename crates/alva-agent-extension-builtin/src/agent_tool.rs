// INPUT:  alva_kernel_abi, async_trait, schemars, serde, serde_json
// OUTPUT: AgentTool
// POS:    Spawns and manages sub-agents, optionally running them in the background.
//! agent_tool — spawn and manage sub-agents

use alva_kernel_abi::{
    create_task_state, AgentError, TaskType, Tool, ToolExecutionContext, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::path::PathBuf;

/// Operating mode for the spawned sub-agent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AgentMode {
    Code,
    Research,
    Review,
    Plan,
}

/// Isolation level for the spawned sub-agent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
enum AgentIsolation {
    None,
    Worktree,
    Sandbox,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// The prompt / instructions for the sub-agent.
    prompt: String,
    /// Short description of what the agent should do.
    description: String,
    /// Model to use for the sub-agent (defaults to current model).
    #[serde(default)]
    model: Option<String>,
    /// Optional name for the sub-agent.
    #[serde(default)]
    name: Option<String>,
    /// Operating mode for the sub-agent.
    #[serde(default)]
    mode: Option<AgentMode>,
    /// Isolation level for the sub-agent.
    #[serde(default)]
    isolation: Option<AgentIsolation>,
    /// If true, run the agent in the background and return a task ID.
    #[serde(default)]
    run_in_background: Option<bool>,
}

#[derive(Tool)]
#[tool(
    name = "agent",
    description = "Spawn a sub-agent to handle a task. The agent runs with its own context and can \
        optionally run in the background. Use this to delegate complex work to a separate \
        agent instance.",
    input = Input,
    // FUTURE-TRAP: `read_only` is correct for today's stub (no real
    // sub-agent is spawned — see execute_impl below) but WILL BECOME
    // WRONG once sub-agent runtime is wired. Spawned agents can mutate
    // FS, send messages, etc. — that's not read-only. When wiring lands,
    // drop this attr (same fix shape as T8/T9 in bugs.jsonl) and update
    // the is_read_only assertion in `tests::is_read_only_is_always_true`.
    read_only,
)]
pub struct AgentTool;

impl AgentTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let agent_name = params.name.as_deref().unwrap_or("sub-agent");
        let is_background = params.run_in_background.unwrap_or(false);

        if is_background {
            let output_dir = ctx
                .workspace()
                .map(|w| w.join(".tasks"))
                .unwrap_or_else(|| PathBuf::from("/tmp/.tasks"));

            let state = create_task_state(
                TaskType::LocalAgent,
                params.description.clone(),
                None,
                output_dir.join(format!("{}.log", agent_name)),
            );
            let task_id = state.id.clone();

            Ok(ToolOutput::text(format!(
                "Agent '{}' started in background.\n  Task ID: {}\n  Description: {}\n  \
                 Use task_get or task_output to check progress.",
                agent_name, task_id, params.description
            )))
        } else {
            // In a full implementation, this would actually spawn the sub-agent,
            // wait for it to complete, and return its result.
            let model_info = params.model.as_deref().unwrap_or("default");
            let mode_info = match params.mode {
                Some(AgentMode::Code) => "code",
                Some(AgentMode::Research) => "research",
                Some(AgentMode::Review) => "review",
                Some(AgentMode::Plan) => "plan",
                None => "code",
            };
            let isolation_info = match params.isolation {
                Some(AgentIsolation::None) | None => "none",
                Some(AgentIsolation::Worktree) => "worktree",
                Some(AgentIsolation::Sandbox) => "sandbox",
            };

            Ok(ToolOutput::text(format!(
                "Agent '{}' completed.\n  Model: {}\n  Mode: {}\n  Isolation: {}\n  \
                 Description: {}\n  Prompt length: {} chars\n  \
                 Result: Sub-agent execution is not yet wired to the runtime.",
                agent_name,
                model_info,
                mode_info,
                isolation_info,
                params.description,
                params.prompt.len()
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::Path;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
        workspace: Option<PathBuf>,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn workspace(&self) -> Option<&Path> {
            self.workspace.as_deref()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
            workspace: Some(PathBuf::from("/workspace")),
        }
    }

    #[tokio::test]
    async fn foreground_uses_defaults_when_unspecified() {
        let tool = AgentTool;
        let out = tool
            .execute(
                json!({ "prompt": "refactor login", "description": "Login flow refactor" }),
                &ctx(),
            )
            .await
            .expect("foreground should succeed");

        assert!(!out.is_error);
        let text = out.model_text();
        // Name default = "sub-agent"
        assert!(text.contains("'sub-agent'"), "name default missing: {text}");
        // Model default = "default"
        assert!(
            text.contains("Model: default"),
            "model default missing: {text}"
        );
        // Mode default = "code" (both None and explicit Code map to "code")
        assert!(text.contains("Mode: code"), "mode default missing: {text}");
        // Isolation default = "none"
        assert!(
            text.contains("Isolation: none"),
            "isolation default missing: {text}"
        );
        // Description echoed
        assert!(
            text.contains("Login flow refactor"),
            "description missing: {text}"
        );
        // Prompt length echoed
        assert!(
            text.contains("Prompt length: 14"),
            "prompt length wrong: {text}"
        );
    }

    #[tokio::test]
    async fn foreground_echoes_custom_mode_and_isolation() {
        let tool = AgentTool;
        let out = tool
            .execute(
                json!({
                    "prompt": "analyze deps",
                    "description": "Dep audit",
                    "name": "auditor",
                    "model": "claude-opus-4-7",
                    "mode": "research",
                    "isolation": "worktree",
                }),
                &ctx(),
            )
            .await
            .expect("foreground custom should succeed");

        let text = out.model_text();
        assert!(text.contains("'auditor'"), "custom name missing: {text}");
        assert!(
            text.contains("Model: claude-opus-4-7"),
            "custom model missing: {text}"
        );
        assert!(
            text.contains("Mode: research"),
            "custom mode missing: {text}"
        );
        assert!(
            text.contains("Isolation: worktree"),
            "custom isolation missing: {text}"
        );
    }

    #[tokio::test]
    async fn background_generates_task_id_and_advertises_followup() {
        let tool = AgentTool;
        let out = tool
            .execute(
                json!({
                    "prompt": "long task",
                    "description": "Big background job",
                    "name": "worker",
                    "run_in_background": true,
                }),
                &ctx(),
            )
            .await
            .expect("background should succeed");

        let text = out.model_text();
        assert!(
            text.contains("background"),
            "background marker missing: {text}"
        );
        assert!(text.contains("Task ID:"), "task id label missing: {text}");
        // task ids generated via create_task_state(TaskType::LocalAgent) start with 'a'
        // (see TaskType::prefix in alva-kernel-abi). Assert presence of a hint.
        assert!(
            text.contains("task_get or task_output"),
            "followup hint missing: {text}"
        );
    }

    #[tokio::test]
    async fn missing_required_prompt_returns_input_error() {
        let tool = AgentTool;
        let err = tool
            .execute(json!({ "description": "no prompt" }), &ctx())
            .await
            .expect_err("missing required `prompt` should error");

        let msg = format!("{err}");
        assert!(
            msg.contains("invalid input") || msg.contains("prompt"),
            "expected invalid-input error mentioning `prompt`, got: {msg}"
        );
    }

    /// Stub-output contract guard: foreground path's "not yet wired"
    /// disclosure must remain visible until sub-agent runtime is wired.
    #[tokio::test]
    async fn foreground_stub_advertises_unwired_runtime() {
        let tool = AgentTool;
        let out = tool
            .execute(json!({ "prompt": "x", "description": "y" }), &ctx())
            .await
            .expect("execute should succeed");
        assert!(
            out.model_text().contains("not yet wired"),
            "stub disclosure missing — if you wired sub-agent runtime, update this test"
        );
    }

    #[test]
    fn is_read_only_is_always_true() {
        let tool = AgentTool;
        // Macro `read_only` attribute makes the spawner declare itself
        // read-only — note: this is a current declaration choice; if a
        // future PR removes `read_only` (because sub-agents can mutate FS),
        // this test fails deliberately and the choice gets reconsidered.
        assert!(tool.is_read_only(&json!({ "prompt": "x", "description": "y" })));
        assert!(tool.is_read_only(&json!({
            "prompt": "x", "description": "y", "run_in_background": true
        })));
    }
}
