// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: TeamDeleteTool
// POS:    Deletes a multi-agent team by name.
//! team_delete — delete a team

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::TeamService;

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Name of the team to delete.
    team_name: String,
}

#[derive(Tool)]
#[tool(
    name = "team_delete",
    description = "Delete a multi-agent team by name. This stops all agents in the team and removes it.",
    input = Input,
    destructive,
)]
pub struct TeamDeleteTool;

impl TeamDeleteTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TeamService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "team_delete".into(),
                message: "team service not registered on bus".into(),
            })?;

        match svc.delete(&params.team_name).await {
            Ok(()) => Ok(ToolOutput::text(format!(
                "Team '{}' deleted successfully.",
                params.team_name
            ))),
            Err(e) => Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Failed to delete team '{}': {e}",
                    params.team_name
                ))],
                is_error: true,
                details: None,
            }),
        }
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

    use crate::services::{InMemoryTeamStore, TeamService, Teammate};

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
        bus.writer().provide::<dyn TeamService>(store.clone());
        (
            TestContext {
                cancel: CancellationToken::new(),
                bus: Some(bus.handle()),
            },
            store,
        )
    }

    #[tokio::test]
    async fn team_delete_returns_success_message() {
        let (ctx, store) = ctx_with_store();
        store
            .create(Teammate {
                name: "alpha".into(),
                agent_type: "general".into(),
                system_prompt: None,
            })
            .await
            .unwrap();

        let tool = TeamDeleteTool;

        let output = tool
            .execute(json!({ "team_name": "alpha" }), &ctx)
            .await
            .expect("team_delete should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("alpha"), "got: {text}");
        assert!(text.contains("deleted"), "got: {text}");
        assert!(store.get("alpha").await.is_none());
    }

    #[tokio::test]
    async fn team_delete_unknown_returns_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = TeamDeleteTool;

        let output = tool
            .execute(json!({ "team_name": "nope" }), &ctx)
            .await
            .expect("not-found shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("nope"), "got: {text}");
    }

    #[tokio::test]
    async fn team_delete_rejects_missing_team_name() {
        let (ctx, _) = ctx_with_store();
        let tool = TeamDeleteTool;

        let err = tool
            .execute(json!({}), &ctx)
            .await
            .expect_err("missing team_name should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn team_delete_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
        };
        let tool = TeamDeleteTool;
        let err = tool
            .execute(json!({ "team_name": "x" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("team service not registered"));
    }

    #[test]
    fn team_delete_is_destructive() {
        let tool = TeamDeleteTool;
        assert!(tool.is_destructive(&json!({ "team_name": "x" })));
        assert_eq!(tool.name(), "team_delete");
    }
}
