// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TeamCreateTool
// POS:    Creates a multi-agent team with a unique name.
//! team_create — create a multi-agent team

use alva_kernel_abi::{
    AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput,
};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::{TeamService, Teammate};

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Unique name for the team.
    team_name: String,
    /// Description of the team's purpose.
    description: String,
    /// Type of agents in the team (e.g. 'code', 'research', 'review').
    #[serde(default)]
    agent_type: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "team_create",
    description = "Create a new multi-agent team. Teams allow coordinating work across multiple agents.",
    input = Input,
)]
pub struct TeamCreateTool;

impl TeamCreateTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TeamService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "team_create".into(),
                message: "team service not registered on bus".into(),
            })?;

        let agent_type = params
            .agent_type
            .clone()
            .unwrap_or_else(|| "general".to_string());

        let mate = Teammate {
            name: params.team_name.clone(),
            agent_type: agent_type.clone(),
            // Reuse the description as the system prompt for now — it
            // describes the team's purpose, which is exactly the seed a
            // sub-agent runtime would want.
            system_prompt: Some(params.description.clone()),
        };

        if let Err(e) = svc.create(mate).await {
            return Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Failed to create team '{}': {e}",
                    params.team_name
                ))],
                is_error: true,
                details: None,
            });
        }

        Ok(ToolOutput::text(format!(
            "Team created successfully.\n  Name: {}\n  Description: {}\n  Agent type: {}",
            params.team_name, params.description, agent_type
        )))
    }
}

#[cfg(test)]
mod tests {
    use std::any::Any;
    use std::path::Path;
    use std::sync::Arc;

    use super::*;
    use alva_kernel_abi::{Bus, BusHandle, CancellationToken, Tool, ToolExecutionContext};
    use serde_json::json;

    use crate::services::{InMemoryTeamStore, TeamService};

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
        fn workspace(&self) -> Option<&Path> {
            None
        }
        fn as_any(&self) -> &dyn Any {
            self
        }
        fn bus(&self) -> Option<&BusHandle> {
            self.bus.as_ref()
        }
    }

    fn ctx_with_store() -> (TestContext, Arc<InMemoryTeamStore>) {
        let store = Arc::new(InMemoryTeamStore::new());
        let bus = Bus::new();
        bus.writer()
            .provide::<dyn TeamService>(store.clone());
        (
            TestContext {
                cancel: CancellationToken::new(),
                bus: Some(bus.handle()),
            },
            store,
        )
    }

    #[tokio::test]
    async fn team_create_defaults_to_general_agent_type() {
        let (ctx, store) = ctx_with_store();
        let tool = TeamCreateTool;

        let output = tool
            .execute(
                json!({
                    "team_name": "alpha",
                    "description": "Test team",
                }),
                &ctx,
            )
            .await
            .expect("team_create should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("alpha"), "got: {text}");
        assert!(text.contains("Test team"), "got: {text}");
        assert!(text.contains("general"), "expected default type: {text}");

        let mate = store.get("alpha").await.expect("teammate stored");
        assert_eq!(mate.agent_type, "general");
    }

    #[tokio::test]
    async fn team_create_includes_custom_agent_type() {
        let (ctx, store) = ctx_with_store();
        let tool = TeamCreateTool;

        let output = tool
            .execute(
                json!({
                    "team_name": "beta",
                    "description": "Research team",
                    "agent_type": "research",
                }),
                &ctx,
            )
            .await
            .expect("team_create should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("research"), "got: {text}");
        let mate = store.get("beta").await.unwrap();
        assert_eq!(mate.agent_type, "research");
    }

    #[tokio::test]
    async fn team_create_duplicate_returns_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = TeamCreateTool;

        let _ = tool
            .execute(
                json!({
                    "team_name": "dup",
                    "description": "first",
                }),
                &ctx,
            )
            .await
            .unwrap();
        let output = tool
            .execute(
                json!({
                    "team_name": "dup",
                    "description": "second",
                }),
                &ctx,
            )
            .await
            .expect("duplicate shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("dup"), "got: {text}");
    }

    #[tokio::test]
    async fn team_create_rejects_missing_team_name() {
        let (ctx, _) = ctx_with_store();
        let tool = TeamCreateTool;

        let err = tool
            .execute(json!({ "description": "no name" }), &ctx)
            .await
            .expect_err("missing team_name should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn team_create_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TeamCreateTool;
        let err = tool
            .execute(
                json!({
                    "team_name": "x",
                    "description": "y",
                }),
                &ctx,
            )
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("team service not registered"));
    }
}
