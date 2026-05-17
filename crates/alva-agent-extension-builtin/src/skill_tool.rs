// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: SkillTool
// POS:    Invokes a named skill/command.
//! skill_tool — invoke skills/commands

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name of the skill to invoke (e.g. 'commit', 'review-pr').
    skill: String,
    /// Optional arguments to pass to the skill.
    #[serde(default)]
    args: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "skill",
    description = "Invoke a skill or command by name. Skills are specialized capabilities registered \
        with the agent framework.",
    input = Input,
    // FUTURE-TRAP: `read_only` matches today's stub (no SkillRegistry
    // wiring — see Loop 3 in .alva/looper-state.md), but skill invocation
    // CAN mutate (a skill may run side-effecting tools internally). When
    // SkillRegistry is wired, drop this attr — same fix shape as T8/T9
    // — and update tests::stub_text_advertises_unwired_registry +
    // surrounding assertions to assert !is_read_only.
    read_only,
)]
pub struct SkillTool;

impl SkillTool {
    async fn execute_impl(
        &self,
        params: Input,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let args_info = params.args.as_deref().unwrap_or("(none)");

        // In a full implementation, this would look up the skill from a registry
        // and invoke it with the given arguments.
        Ok(ToolOutput::text(format!(
            "Skill '{}' invoked with args: {}\n\
             Skill execution is not yet wired to the skill registry.",
            params.skill, args_info
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;

    use super::*;
    use alva_kernel_abi::{CancellationToken, Tool};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx() -> TestContext {
        TestContext {
            cancel: CancellationToken::new(),
        }
    }

    #[tokio::test]
    async fn invocation_echoes_skill_name_and_args() {
        let tool = SkillTool;
        let out = tool
            .execute(
                json!({ "skill": "commit", "args": "--amend" }),
                &ctx(),
            )
            .await
            .expect("execute should succeed");

        assert!(!out.is_error, "stub invocation should not be an error");
        let text = out.model_text();
        assert!(text.contains("commit"), "skill name missing: {text}");
        assert!(text.contains("--amend"), "args missing: {text}");
    }

    #[tokio::test]
    async fn args_default_to_none_marker_when_omitted() {
        let tool = SkillTool;
        let out = tool
            .execute(json!({ "skill": "review-pr" }), &ctx())
            .await
            .expect("execute should succeed");

        let text = out.model_text();
        assert!(text.contains("review-pr"));
        assert!(
            text.contains("(none)"),
            "expected '(none)' placeholder when args omitted: {text}"
        );
    }

    #[tokio::test]
    async fn missing_skill_field_returns_input_error() {
        let tool = SkillTool;
        let err = tool
            .execute(json!({ "args": "ignored" }), &ctx())
            .await
            .expect_err("missing required `skill` should error");

        let msg = format!("{err}");
        // schemars/serde error mentions the missing field; tolerate either
        // phrasing (different serde versions word it slightly differently).
        assert!(
            msg.contains("invalid input") || msg.contains("skill"),
            "expected invalid-input error mentioning `skill`, got: {msg}"
        );
    }

    /// Stub-output contract guard: when SkillRegistry wiring lands later,
    /// this string must change deliberately (and so must this test). Until
    /// then it documents the user-visible "not wired" hint.
    #[tokio::test]
    async fn stub_text_advertises_unwired_registry() {
        let tool = SkillTool;
        let out = tool
            .execute(json!({ "skill": "x" }), &ctx())
            .await
            .expect("execute should succeed");
        assert!(
            out.model_text().contains("not yet wired"),
            "stub disclosure missing — if you wired the registry, update this test"
        );
    }
}
