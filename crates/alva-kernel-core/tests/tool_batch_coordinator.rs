use std::sync::Arc;

use alva_kernel_abi::agent_session::{AgentSession, EventQuery, InMemoryAgentSession};
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{CancellationToken, Message, ModelConfig, StreamEvent, ToolCall, ToolOutput};
use alva_kernel_core::middleware::MiddlewareStack;
use alva_kernel_core::shared::Extensions;
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::tool_batch::ToolBatchCoordinator;
use alva_test::mock_tool::MockTool;
use async_trait::async_trait;
use futures_core::Stream;

struct UnusedModel;

#[async_trait]
impl LanguageModel for UnusedModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        unreachable!("tool batch tests do not call the model")
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        unreachable!("tool batch tests do not call the model")
    }

    fn model_id(&self) -> &str {
        "unused"
    }
}

struct CancellingTool {
    cancel: CancellationToken,
}

#[async_trait]
impl Tool for CancellingTool {
    fn name(&self) -> &str {
        "cancel_first"
    }

    fn description(&self) -> &str {
        "Cancels the batch after returning"
    }

    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({ "type": "object" })
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn alva_kernel_abi::tool::execution::ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        self.cancel.cancel();
        Ok(ToolOutput::text("cancelled after this"))
    }
}

fn test_config() -> AgentConfig {
    AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: Vec::new(),
        max_iterations: 10,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    }
}

#[tokio::test]
async fn coordinator_commits_tool_results_in_model_order() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let first_tool = MockTool::new("first").with_result(ToolOutput::text("one"));
    let second_tool = MockTool::new("second").with_result(ToolOutput::text("two"));
    let mut state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![Arc::new(first_tool), Arc::new(second_tool)],
        session: session.clone(),
        extensions: Extensions::new(),
    };
    let config = test_config();
    let cancel = CancellationToken::new();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![
        ToolCall {
            id: "toolu_second".to_string(),
            name: "second".to_string(),
            arguments: serde_json::json!({"order": 2}),
        },
        ToolCall {
            id: "toolu_first".to_string(),
            name: "first".to_string(),
            arguments: serde_json::json!({"order": 1}),
        },
    ];

    let committed = ToolBatchCoordinator::new()
        .execute_batch(
            &mut state,
            &config,
            cancel,
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        )
        .await
        .unwrap();

    assert_eq!(committed.len(), 2);
    assert_eq!(committed[0].tool_call.id, "toolu_second");
    assert_eq!(committed[1].tool_call.id, "toolu_first");

    let events = session
        .query(&EventQuery {
            limit: 1000,
            ..Default::default()
        })
        .await;
    let tool_events: Vec<_> = events
        .iter()
        .filter(|em| matches!(em.event.event_type.as_str(), "tool_use" | "tool_result"))
        .collect();

    assert_eq!(tool_events.len(), 4);
    assert_eq!(
        tool_events
            .iter()
            .map(|em| em.event.event_type.as_str())
            .collect::<Vec<_>>(),
        vec!["tool_use", "tool_result", "tool_use", "tool_result"]
    );
    assert_eq!(
        tool_events[0].event.parent_uuid.as_deref(),
        Some("llm_parent")
    );
    assert_eq!(
        tool_events[1].event.parent_uuid.as_deref(),
        Some(tool_events[0].event.uuid.as_str())
    );
    assert_eq!(
        tool_events[2].event.parent_uuid.as_deref(),
        Some("llm_parent")
    );
    assert_eq!(
        tool_events[3].event.parent_uuid.as_deref(),
        Some(tool_events[2].event.uuid.as_str())
    );

    let tool_result_ids: Vec<String> = events
        .iter()
        .filter(|em| em.event.event_type == "tool_result")
        .filter_map(|em| {
            let content = &em.event.message.as_ref()?.content;
            content
                .as_array()?
                .iter()
                .find(|block| block.get("type").and_then(|ty| ty.as_str()) == Some("tool_result"))?
                .get("id")?
                .as_str()
                .map(str::to_string)
        })
        .collect();

    assert_eq!(tool_result_ids, vec!["toolu_second", "toolu_first"]);
}

#[tokio::test]
async fn coordinator_commits_cancelled_results_for_remaining_declared_calls() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let cancel = CancellationToken::new();
    let mut state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![
            Arc::new(CancellingTool {
                cancel: cancel.clone(),
            }),
            Arc::new(MockTool::new("second").with_result(ToolOutput::text("two"))),
        ],
        session: session.clone(),
        extensions: Extensions::new(),
    };
    let config = test_config();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![
        ToolCall {
            id: "toolu_cancel".to_string(),
            name: "cancel_first".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "toolu_second".to_string(),
            name: "second".to_string(),
            arguments: serde_json::json!({}),
        },
    ];

    let committed = ToolBatchCoordinator::new()
        .execute_batch(
            &mut state,
            &config,
            cancel,
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        )
        .await
        .unwrap();

    assert_eq!(committed.len(), 2);
    assert_eq!(committed[0].tool_call.id, "toolu_cancel");
    assert_eq!(committed[1].tool_call.id, "toolu_second");
    assert!(committed[1].result.is_error);
    assert!(
        committed[1].result.model_text().contains("cancelled"),
        "remaining declared tool call should get a cancelled result, got: {}",
        committed[1].result.model_text()
    );

    let events = session
        .query(&EventQuery {
            limit: 1000,
            ..Default::default()
        })
        .await;
    let tool_result_ids: Vec<String> = events
        .iter()
        .filter(|em| em.event.event_type == "tool_result")
        .filter_map(|em| {
            let content = &em.event.message.as_ref()?.content;
            content
                .as_array()?
                .iter()
                .find(|block| block.get("type").and_then(|ty| ty.as_str()) == Some("tool_result"))?
                .get("id")?
                .as_str()
                .map(str::to_string)
        })
        .collect();

    assert_eq!(tool_result_ids, vec!["toolu_cancel", "toolu_second"]);
}
