// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: SkillRegistry, SkillRegistryError, SkillTool
// POS:    Unified named-skill invocation tool backed by a registry capability discovered on the runtime bus.
//! `skill` — invoke a named skill through the registry published by the
//! harness-level skills plugin.

use alva_kernel_abi::{AgentError, Tool, ToolExecutionContext, ToolOutput};
use async_trait::async_trait;
use schemars::JsonSchema;
use serde::Deserialize;

/// Failure returned by a skill registry implementation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SkillRegistryError {
    NotFound(String),
    Load(String),
}

impl std::fmt::Display for SkillRegistryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotFound(name) => write!(f, "skill '{name}' was not found or is disabled"),
            Self::Load(message) => f.write_str(message),
        }
    }
}

/// Bus capability implemented by the real skill subsystem.
#[async_trait]
pub trait SkillRegistry: Send + Sync {
    /// Load a named skill as an Explicit/Strict context block.
    async fn invoke(&self, skill: &str, args: Option<&str>) -> Result<String, SkillRegistryError>;

    /// All enabled skill names, including explicit-only skills.
    async fn available_names(&self) -> Vec<String>;
}

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
    // Invoking a skill only loads instructions. Any side-effecting tool the
    // model subsequently uses still passes through that tool's own policy.
    read_only,
)]
pub struct SkillTool;

impl SkillTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let Some(registry) = ctx.bus().and_then(|bus| bus.get::<dyn SkillRegistry>()) else {
            return Ok(ToolOutput::error(
                "skill registry is unavailable; enable the skills plugin",
            ));
        };

        match registry.invoke(&params.skill, params.args.as_deref()).await {
            Ok(injection) => Ok(ToolOutput::text(injection)),
            Err(SkillRegistryError::NotFound(_)) => {
                let mut names = registry.available_names().await;
                names.sort();
                let available = if names.is_empty() {
                    "(none)".to_string()
                } else {
                    names.join(", ")
                };
                Ok(ToolOutput::error(format!(
                    "Unknown skill '{}'. Available enabled skills: {available}",
                    params.skill
                )))
            }
            Err(SkillRegistryError::Load(message)) => Ok(ToolOutput::error(message)),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::sync::Arc;

    use super::*;
    use alva_kernel_abi::{Bus, BusHandle, CancellationToken, Tool};
    use serde_json::json;

    struct TestContext {
        cancel: CancellationToken,
        bus: Option<BusHandle>,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            "test-session"
        }
        fn bus(&self) -> Option<&BusHandle> {
            self.bus.as_ref()
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    fn ctx(registry: Option<Arc<dyn SkillRegistry>>) -> TestContext {
        let bus = registry.map(|registry| {
            let bus = Bus::new();
            bus.writer().provide::<dyn SkillRegistry>(registry);
            bus.handle()
        });
        TestContext {
            cancel: CancellationToken::new(),
            bus,
        }
    }

    struct TestRegistry;

    #[async_trait]
    impl SkillRegistry for TestRegistry {
        async fn invoke(
            &self,
            skill: &str,
            args: Option<&str>,
        ) -> Result<String, SkillRegistryError> {
            if skill == "commit" {
                Ok(format!(
                    "## Skill: commit\n\nCommit carefully.\n\n## Invocation Arguments\n\n{}",
                    args.unwrap_or("")
                ))
            } else {
                Err(SkillRegistryError::NotFound(skill.to_string()))
            }
        }

        async fn available_names(&self) -> Vec<String> {
            vec!["explicit-secret".into(), "commit".into()]
        }
    }

    #[tokio::test]
    async fn invocation_echoes_skill_name_and_args() {
        let tool = SkillTool;
        let out = tool
            .execute(
                json!({ "skill": "commit", "args": "--amend" }),
                &ctx(Some(Arc::new(TestRegistry))),
            )
            .await
            .expect("execute should succeed");

        assert!(!out.is_error, "registry invocation should not be an error");
        let text = out.model_text();
        assert!(text.contains("commit"), "skill name missing: {text}");
        assert!(text.contains("--amend"), "args missing: {text}");
        assert!(
            text.contains("Commit carefully."),
            "skill body missing: {text}"
        );
    }

    #[tokio::test]
    async fn unknown_skill_is_loud_and_lists_all_enabled_names() {
        let tool = SkillTool;
        let out = tool
            .execute(
                json!({ "skill": "missing" }),
                &ctx(Some(Arc::new(TestRegistry))),
            )
            .await
            .expect("unknown skill should return a model-visible tool error");

        assert!(out.is_error);
        let text = out.model_text();
        assert!(text.contains("Unknown skill 'missing'"), "{text}");
        assert!(text.contains("commit"), "{text}");
        assert!(text.contains("explicit-secret"), "{text}");
    }

    #[tokio::test]
    async fn missing_skill_field_returns_input_error() {
        let tool = SkillTool;
        let err = tool
            .execute(json!({ "args": "ignored" }), &ctx(None))
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

    #[tokio::test]
    async fn missing_registry_fails_with_actionable_error() {
        let tool = SkillTool;
        let out = tool
            .execute(json!({ "skill": "x" }), &ctx(None))
            .await
            .expect("missing registry should return a model-visible tool error");
        assert!(out.is_error);
        assert!(
            out.model_text().contains("enable the skills plugin"),
            "missing-registry guidance should be actionable: {}",
            out.model_text()
        );
    }
}
