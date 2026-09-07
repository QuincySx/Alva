// INPUT:  alva-kernel-core ToolBatchCoordinator, alva-test MockTool, Tokio test runtime
// OUTPUT: Regression coverage for ordered commits, cancellation, failure backfill, lock timeout, and parallel waves
// POS:    Integration contract tests for tool batch scheduling and durable result pairing.
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::model::{CompletionResponse, LanguageModel};
use alva_kernel_abi::tool::Tool;
use alva_kernel_abi::{
    CancellationToken, ExecutionRecord, Message, ModelConfig, SessionRecordPayload, StreamEvent,
    ToolCall, ToolOutput,
};
use alva_kernel_core::agent_session::{AgentSession, EventQuery, InMemoryAgentSession};
use alva_kernel_core::middleware::{Middleware, MiddlewareError, MiddlewareStack, ToolCallFn};
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

    fn parameters_schema(
        &self,
        _ctx: &alva_kernel_abi::tool::schema::ToolSchemaContext,
    ) -> serde_json::Value {
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

struct ConcurrencyProbeTool {
    name: &'static str,
    resource: alva_kernel_abi::ResourceKey,
    rendezvous: Option<Arc<tokio::sync::Barrier>>,
    active: Arc<AtomicUsize>,
    max_active: Arc<AtomicUsize>,
}

#[async_trait]
impl Tool for ConcurrencyProbeTool {
    fn name(&self) -> &str {
        self.name
    }

    fn description(&self) -> &str {
        "Records whether tool calls overlap"
    }

    fn parameters_schema(
        &self,
        _ctx: &alva_kernel_abi::tool::schema::ToolSchemaContext,
    ) -> serde_json::Value {
        serde_json::json!({ "type": "object" })
    }

    fn scheduling(&self, _input: &serde_json::Value) -> alva_kernel_abi::Scheduling {
        alva_kernel_abi::Scheduling::default()
            .concurrency_safe()
            .with_resource_keys(vec![self.resource.clone()])
    }

    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn alva_kernel_abi::tool::execution::ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        let active = self.active.fetch_add(1, Ordering::SeqCst) + 1;
        self.max_active.fetch_max(active, Ordering::SeqCst);
        if let Some(rendezvous) = self.rendezvous.as_ref() {
            rendezvous.wait().await;
        } else {
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
        self.active.fetch_sub(1, Ordering::SeqCst);
        Ok(ToolOutput::text(self.name))
    }
}

fn log_handle(
    state: &alva_kernel_core::state::AgentState,
) -> std::sync::Arc<dyn alva_kernel_abi::ConversationLog> {
    state.session.clone()
}

#[tokio::test]
async fn conflict_free_safe_calls_overlap_but_commit_in_model_order() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let rendezvous = Arc::new(tokio::sync::Barrier::new(2));
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![
            Arc::new(ConcurrencyProbeTool {
                name: "first",
                resource: alva_kernel_abi::ResourceKey::write("src/first.rs"),
                rendezvous: Some(rendezvous.clone()),
                active: active.clone(),
                max_active: max_active.clone(),
            }),
            Arc::new(ConcurrencyProbeTool {
                name: "second",
                resource: alva_kernel_abi::ResourceKey::write("src/second.rs"),
                rendezvous: Some(rendezvous),
                active,
                max_active: max_active.clone(),
            }),
        ],
        session: session.clone(),
        extensions: Extensions::new(),
    };
    let tool_calls = vec![
        ToolCall {
            id: "call-first".into(),
            name: "first".into(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "call-second".into(),
            name: "second".into(),
            arguments: serde_json::json!({}),
        },
    ];
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();

    let committed = tokio::time::timeout(
        Duration::from_secs(1),
        ToolBatchCoordinator::new().execute_batch(
            &state,
            &log_handle(&state),
            &test_config(),
            CancellationToken::new(),
            &tool_calls,
            "llm-parent".into(),
            event_tx,
        ),
    )
    .await
    .expect("conflict-free calls should reach the rendezvous concurrently")
    .unwrap();

    assert_eq!(max_active.load(Ordering::SeqCst), 2);
    assert_eq!(committed[0].tool_call.id, "call-first");
    assert_eq!(committed[1].tool_call.id, "call-second");

    let execution_order: Vec<_> = session
        .records(0)
        .await
        .into_iter()
        .filter_map(|record| match record.payload {
            SessionRecordPayload::Execution(ExecutionRecord::Intent { operation, .. }) => {
                Some(("intent", operation))
            }
            SessionRecordPayload::Execution(ExecutionRecord::Outcome { operation_id, .. }) => {
                Some(("outcome", operation_id))
            }
            _ => None,
        })
        .collect();
    assert_eq!(
        execution_order,
        vec![
            ("intent", "first".to_string()),
            ("intent", "second".to_string()),
            ("outcome", "call-first".to_string()),
            ("outcome", "call-second".to_string()),
        ]
    );
}

#[tokio::test]
async fn conflicting_safe_calls_remain_serial() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let active = Arc::new(AtomicUsize::new(0));
    let max_active = Arc::new(AtomicUsize::new(0));
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![
            Arc::new(ConcurrencyProbeTool {
                name: "first",
                resource: alva_kernel_abi::ResourceKey::write("src/shared.rs"),
                rendezvous: None,
                active: active.clone(),
                max_active: max_active.clone(),
            }),
            Arc::new(ConcurrencyProbeTool {
                name: "second",
                resource: alva_kernel_abi::ResourceKey::write("/workspace/src/shared.rs"),
                rendezvous: None,
                active,
                max_active: max_active.clone(),
            }),
        ],
        session,
        extensions: Extensions::new(),
    };
    let tool_calls = vec![
        ToolCall {
            id: "call-first".into(),
            name: "first".into(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "call-second".into(),
            name: "second".into(),
            arguments: serde_json::json!({}),
        },
    ];
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let mut config = test_config();
    config.workspace = Some(std::path::PathBuf::from("/workspace"));

    ToolBatchCoordinator::new()
        .execute_batch(
            &state,
            &log_handle(&state),
            &config,
            CancellationToken::new(),
            &tool_calls,
            "llm-parent".into(),
            event_tx,
        )
        .await
        .unwrap();

    assert_eq!(max_active.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn coordinator_commits_tool_results_in_model_order() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let first_tool = MockTool::new("first").with_result(ToolOutput::text("one"));
    let second_tool = MockTool::new("second").with_result(ToolOutput::text("two"));
    let state = AgentState {
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
            &state,
            &log_handle(&state),
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
    let records = session.records(0).await;
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(
                &record.payload,
                SessionRecordPayload::Execution(ExecutionRecord::Intent { .. })
            ))
            .count(),
        2
    );
    assert_eq!(
        records
            .iter()
            .filter(|record| matches!(
                &record.payload,
                SessionRecordPayload::Execution(ExecutionRecord::Outcome { .. })
            ))
            .count(),
        2
    );
}

#[tokio::test]
async fn coordinator_commits_cancelled_results_for_remaining_declared_calls() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let cancel = CancellationToken::new();
    let state = AgentState {
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
            &state,
            &log_handle(&state),
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

/// Ticket 07: the same dangling-call guarantee must hold for a middleware
/// failing in ANY phase of the tool batch. `before_tool_call` variant.
struct FailBeforeFirstMiddleware;

#[async_trait]
impl Middleware for FailBeforeFirstMiddleware {
    async fn before_tool_call(
        &self,
        _state: &AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        if tool_call.name == "first" {
            Err(MiddlewareError::from(AgentError::ConfigError(
                "boom in before_tool_call".into(),
            )))
        } else {
            Ok(())
        }
    }
}

/// `wrap_tool_call` variant — the failure happens around the actual
/// execution, past the intent commit.
struct FailWrapFirstMiddleware;

#[async_trait]
impl Middleware for FailWrapFirstMiddleware {
    async fn wrap_tool_call(
        &self,
        _state: &AgentState,
        _tool_call: &ToolCall,
        next: &dyn ToolCallFn,
    ) -> Result<ToolOutput, MiddlewareError> {
        let _output = next.call(_state, _tool_call).await?;
        Err(MiddlewareError::from(AgentError::ConfigError(
            "boom in wrap_tool_call".into(),
        )))
    }
}

async fn assert_no_dangling_calls_with<F>(make_stack: F)
where
    F: FnOnce() -> MiddlewareStack,
{
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![
            Arc::new(MockTool::new("first").with_result(ToolOutput::text("one"))),
            Arc::new(MockTool::new("second").with_result(ToolOutput::text("two"))),
        ],
        session: session.clone(),
        extensions: Extensions::new(),
    };
    let mut config = test_config();
    config.middleware = make_stack();

    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![
        ToolCall {
            id: "toolu_first".to_string(),
            name: "first".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "toolu_second".to_string(),
            name: "second".to_string(),
            arguments: serde_json::json!({}),
        },
    ];

    let result = ToolBatchCoordinator::new()
        .execute_batch(
            &state,
            &log_handle(&state),
            &config,
            CancellationToken::new(),
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        )
        .await;
    assert!(result.is_err(), "the batch must still abort");

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
    assert!(
        tool_result_ids.contains(&"toolu_first".to_string()),
        "aborting call must be paired; got {tool_result_ids:?}"
    );
    assert!(
        tool_result_ids.contains(&"toolu_second".to_string()),
        "remaining call must be backfilled; got {tool_result_ids:?}"
    );
}

#[tokio::test]
async fn before_phase_middleware_error_leaves_no_dangling_calls() {
    assert_no_dangling_calls_with(|| {
        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(FailBeforeFirstMiddleware));
        stack
    });
}

#[tokio::test]
async fn wrap_phase_middleware_error_leaves_no_dangling_calls() {
    assert_no_dangling_calls_with(|| {
        let mut stack = MiddlewareStack::new();
        stack.push(Arc::new(FailWrapFirstMiddleware));
        stack
    });
}

/// Fails `after_tool_call` for the tool named "first" with a non-Blocked
/// error — the fatal-middleware-error path. Pins finding #9.
struct FailAfterFirstMiddleware;

#[async_trait]
impl Middleware for FailAfterFirstMiddleware {
    async fn after_tool_call(
        &self,
        _state: &AgentState,
        tool_call: &ToolCall,
        _result: &mut ToolOutput,
    ) -> Result<(), MiddlewareError> {
        if tool_call.name == "first" {
            Err(MiddlewareError::from(AgentError::ConfigError(
                "boom in after_tool_call".into(),
            )))
        } else {
            Ok(())
        }
    }
}

#[tokio::test]
async fn middleware_abort_still_pairs_every_tool_call_with_a_result() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![
            Arc::new(MockTool::new("first").with_result(ToolOutput::text("one"))),
            Arc::new(MockTool::new("second").with_result(ToolOutput::text("two"))),
        ],
        session: session.clone(),
        extensions: Extensions::new(),
    };
    let mut config = test_config();
    let mut stack = MiddlewareStack::new();
    stack.push(Arc::new(FailAfterFirstMiddleware));
    config.middleware = stack;

    let cancel = CancellationToken::new();
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![
        ToolCall {
            id: "toolu_first".to_string(),
            name: "first".to_string(),
            arguments: serde_json::json!({}),
        },
        ToolCall {
            id: "toolu_second".to_string(),
            name: "second".to_string(),
            arguments: serde_json::json!({}),
        },
    ];

    let result = ToolBatchCoordinator::new()
        .execute_batch(
            &state,
            &log_handle(&state),
            &config,
            cancel,
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        )
        .await;

    // The fatal middleware error still aborts the batch...
    assert!(
        result.is_err(),
        "a non-Blocked middleware error must still abort the batch"
    );

    // ...but every declared tool_use id now has a matching tool_result, so the
    // next provider request cannot see a dangling tool call.
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

    assert!(
        tool_result_ids.contains(&"toolu_first".to_string()),
        "the aborting tool call must still get a tool_result; got {tool_result_ids:?}"
    );
    assert!(
        tool_result_ids.contains(&"toolu_second".to_string()),
        "the un-started remaining tool call must be backfilled with a tool_result; got {tool_result_ids:?}"
    );
}
