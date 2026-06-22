// INPUT:  alva_kernel_abi, async_trait, schemars, serde
// OUTPUT: SendMessageTool
// POS:    Sends messages between agents for inter-agent communication.
//! send_message — send messages between agents

use alva_kernel_abi::{AgentError, Tool, ToolContent, ToolExecutionContext, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::services::{TeamMessage, TeamService};

#[derive(Debug, Deserialize, JsonSchema)]
struct Input {
    /// Recipient agent name or ID.
    to: String,
    /// Message content to send.
    message: String,
    /// Optional short summary of the message for context.
    #[serde(default)]
    summary: Option<String>,
}

#[derive(Tool)]
#[tool(
    name = "send_message",
    description = "Send a message to another agent by name or ID. Used for inter-agent communication \
        in multi-agent setups.",
    input = Input,
    // NOT read_only: send_message writes to the recipient's inbox via
    // TeamService. CheckpointMiddleware and any other consumer of
    // Tool::is_read_only would otherwise skip snapshotting / audit
    // before this call.
)]
pub struct SendMessageTool;

impl SendMessageTool {
    async fn execute_impl(
        &self,
        params: Input,
        ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let svc = ctx
            .bus()
            .and_then(|b| b.get::<dyn TeamService>())
            .ok_or_else(|| AgentError::ToolError {
                tool_name: "send_message".into(),
                message: "team service not registered on bus".into(),
            })?;

        let summary_info = params
            .summary
            .clone()
            .unwrap_or_else(|| "(no summary)".to_string());
        let msg_len = params.message.len();

        let msg = TeamMessage {
            from: ctx.session_id().to_string(),
            to: params.to.clone(),
            body: params.message,
            summary: params.summary,
            timestamp: chrono::Utc::now().timestamp() as u64,
        };

        if let Err(e) = svc.send_message(msg).await {
            return Ok(ToolOutput {
                content: vec![ToolContent::text(format!(
                    "Failed to send message to '{}': {e}",
                    params.to
                ))],
                is_error: true,
                details: None,
            });
        }

        Ok(ToolOutput::text(format!(
            "Message sent to '{}'.\n  Summary: {}\n  Length: {} chars",
            params.to, summary_info, msg_len
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

    use crate::services::{InMemoryTeamStore, TeamService, Teammate};

    struct TestContext {
        cancel: CancellationToken,
        bus: Option<BusHandle>,
        session: String,
    }

    impl ToolExecutionContext for TestContext {
        fn cancel_token(&self) -> &CancellationToken {
            &self.cancel
        }
        fn session_id(&self) -> &str {
            &self.session
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
                session: "test-session".into(),
            },
            store,
        )
    }

    #[tokio::test]
    async fn send_message_reports_recipient_and_length() {
        let (ctx, store) = ctx_with_store();
        store
            .create(Teammate {
                name: "agent-2".into(),
                agent_type: "general".into(),
                system_prompt: None,
            })
            .await
            .unwrap();

        let tool = SendMessageTool;
        let msg_body = "hello world"; // 11 chars
        let output = tool
            .execute(
                json!({
                    "to": "agent-2",
                    "message": msg_body,
                }),
                &ctx,
            )
            .await
            .expect("send_message should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("agent-2"), "got: {text}");
        assert!(text.contains("11 chars"), "got: {text}");
        assert!(
            text.contains("(no summary)"),
            "default summary missing: {text}"
        );

        let inbox = store.inbox("agent-2").await;
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].body, msg_body);
        assert_eq!(inbox[0].from, "test-session");
    }

    #[tokio::test]
    async fn send_message_includes_summary_when_present() {
        let (ctx, store) = ctx_with_store();
        store
            .create(Teammate {
                name: "coordinator".into(),
                agent_type: "general".into(),
                system_prompt: None,
            })
            .await
            .unwrap();

        let tool = SendMessageTool;
        let output = tool
            .execute(
                json!({
                    "to": "coordinator",
                    "message": "long body",
                    "summary": "ping",
                }),
                &ctx,
            )
            .await
            .expect("send_message should succeed");

        assert!(!output.is_error);
        let text = output.model_text();
        assert!(text.contains("Summary: ping"), "got: {text}");
        assert!(
            !text.contains("(no summary)"),
            "should use provided summary: {text}"
        );
    }

    #[tokio::test]
    async fn send_message_unknown_recipient_returns_is_error() {
        let (ctx, _) = ctx_with_store();
        let tool = SendMessageTool;
        let output = tool
            .execute(
                json!({
                    "to": "nobody",
                    "message": "hi",
                }),
                &ctx,
            )
            .await
            .expect("not-found shouldn't Err");
        assert!(output.is_error);
        let text = output.model_text();
        assert!(text.contains("nobody"), "got: {text}");
    }

    #[tokio::test]
    async fn send_message_rejects_missing_recipient() {
        let (ctx, _) = ctx_with_store();
        let tool = SendMessageTool;

        let err = tool
            .execute(json!({ "message": "hi" }), &ctx)
            .await
            .expect_err("missing recipient should error");
        let msg = format!("{err}");
        assert!(msg.contains("invalid input"), "got: {msg}");
    }

    #[tokio::test]
    async fn send_message_errors_when_service_not_on_bus() {
        let ctx = TestContext {
            cancel: CancellationToken::new(),
            bus: None,
            session: "test-session".into(),
        };
        let tool = SendMessageTool;
        let err = tool
            .execute(json!({ "to": "x", "message": "y" }), &ctx)
            .await
            .expect_err("missing service should error");
        assert!(format!("{err}").contains("team service not registered"));
    }

    /// T8 regression: send_message MUTATES (writes to recipient inbox)
    /// so it must NOT report `is_read_only=true`. CheckpointMiddleware
    /// (alva-host-native/src/middleware/checkpoint.rs:104) keys on this
    /// to decide whether to snapshot before the call; future middleware
    /// (audit, dry-run mode) may key on it too. Naming kept generic
    /// (`send_message_classification`) so adding more invariants later
    /// doesn't require renaming.
    #[test]
    fn send_message_classification() {
        let tool = SendMessageTool;
        assert_eq!(tool.name(), "send_message");
        assert!(
            !tool.is_destructive(&json!({ "to": "x", "message": "y" })),
            "not destructive — recipients can be re-messaged"
        );
        assert!(
            !tool.is_read_only(&json!({ "to": "x", "message": "y" })),
            "T8: NOT read-only — writes recipient inbox"
        );
    }
}
