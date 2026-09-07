// INPUT:  alva_agent_core::{ToolLockMiddleware, ToolLockRegistry}, alva_kernel_core::{state, tool_batch, middleware}, alva_kernel_abi
// OUTPUT: regression coverage for the assembly-layer lock middleware
// POS:    Ticket 08 — the lock table + acquisition moved out of the kernel;
//         deadlock-shaped contention must still surface as a bounded tool
//         ERROR, with byte-identical error text and timing semantics.

use std::sync::Arc;

use alva_agent_core::tool_lock::ToolLockRegistry;
use alva_agent_core::ToolLockMiddleware;
use alva_kernel_abi::base::error::AgentError;
use alva_kernel_abi::base::message::Message;
use alva_kernel_abi::tool::execution::ToolExecutionContext;
use alva_kernel_abi::tool::scheduler::{ExecutionMode, Scheduling};
use alva_kernel_abi::tool::{Tool, ToolCall, ToolOutput};
use alva_kernel_abi::AgentSession;
use alva_kernel_abi::CancellationToken;
use alva_kernel_core::agent_session::InMemoryAgentSession;
use alva_kernel_core::middleware::{Middleware, MiddlewareStack};
use alva_kernel_core::state::{AgentConfig, AgentState};
use alva_kernel_core::tool_batch::ToolBatchCoordinator;
use async_trait::async_trait;

// ---------------------------------------------------------------------------
// Stubs
// ---------------------------------------------------------------------------

struct UnusedModel;

#[async_trait]
impl alva_kernel_abi::model::LanguageModel for UnusedModel {
    fn model_id(&self) -> &str {
        "unused"
    }
    fn provider_id(&self) -> &str {
        "unused"
    }
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &alva_kernel_abi::ModelConfig,
    ) -> Result<alva_kernel_abi::CompletionResponse, AgentError> {
        Err(AgentError::Other("unused".into()))
    }
    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &alva_kernel_abi::ModelConfig,
    ) -> std::pin::Pin<
        Box<dyn futures_core::stream::Stream<Item = alva_kernel_abi::StreamEvent> + Send>,
    > {
        unimplemented!("unused in this test")
    }
}

/// A tool that never gets to run under contention: scheduling descriptor
/// opts into parallel + a resource key, so the lock middleware wraps it.
struct BlockedTool {
    name: String,
    result: String,
}

#[async_trait]
impl Tool for BlockedTool {
    fn name(&self) -> &str {
        &self.name
    }
    fn description(&self) -> &str {
        "blocked tool"
    }
    fn parameters_schema(&self, _ctx: &alva_kernel_abi::ToolSchemaContext) -> serde_json::Value {
        serde_json::json!({"type": "object"})
    }
    fn scheduling(&self, _input: &serde_json::Value) -> Scheduling {
        Scheduling::default()
            .concurrency_safe()
            .with_resource_keys(vec![alva_kernel_abi::ResourceKey::read("/shared")])
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn ToolExecutionContext,
    ) -> Result<ToolOutput, AgentError> {
        Ok(ToolOutput::text(self.result.clone()))
    }
}

fn test_config(bus: Option<alva_kernel_abi::BusHandle>) -> AgentConfig {
    AgentConfig {
        middleware: MiddlewareStack::new(),
        system_prompt: vec![],
        max_iterations: 10,
        model_config: Default::default(),
        context_window: 0,
        workspace: None,
        bus,
        context_system: None,
        context_token_budget: None,
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

fn log_handle(state: &AgentState) -> Arc<dyn alva_kernel_abi::ConversationLog> {
    state.session.clone()
}

/// Deadlock-shaped lock contention must surface as a bounded tool ERROR,
/// not an indefinite hang. The registry's acquire_bounded deadline is the
/// guard: here another holder keeps the GLOBAL write lock (as a
/// serial-global tool would), and a parallel tool's read acquisition must
/// time out and land an error result instead of parking the batch forever.
#[tokio::test]
async fn lock_contention_times_out_as_tool_error_instead_of_hanging() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![Arc::new(BlockedTool {
            name: "blocked_tool".into(),
            result: "never runs".into(),
        })],
        session,
        extensions: alva_kernel_core::shared::Extensions::new(),
    };

    // Registry with a 100ms fuse, published on the bus like production; the
    // middleware rides the same bus (configure()).
    let registry = Arc::new(
        ToolLockRegistry::new().with_acquire_timeout(std::time::Duration::from_millis(100)),
    );
    let bus = alva_kernel_abi::Bus::new();
    bus.writer().provide::<ToolLockRegistry>(registry.clone());

    // Simulate an in-flight serial-global tool: hold the GLOBAL write lock
    // for the whole test.
    let _global_write = registry.acquire(&[], ExecutionMode::SerialGlobal).await;

    let mut config = test_config(Some(bus.handle()));
    let mut stack = MiddlewareStack::new();
    stack.push(Arc::new(ToolLockMiddleware::new().with_bus(bus.handle())));
    config.middleware = stack;

    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![ToolCall {
        id: "toolu_blocked".to_string(),
        name: "blocked_tool".to_string(),
        arguments: serde_json::json!({}),
    }];

    let committed = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        ToolBatchCoordinator::new().execute_batch(
            &state,
            &log_handle(&state),
            &config,
            CancellationToken::new(),
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        ),
    )
    .await
    .expect("lock contention must NOT hang the batch (bounded acquire)")
    .unwrap();

    assert_eq!(committed.len(), 1);
    assert!(
        committed[0].result.is_error,
        "the blocked tool must land an error result"
    );
    let text = committed[0].result.model_text();
    assert!(
        text.contains("timed out") || text.contains("Timeout"),
        "error must say the lock wait timed out, got: {text}"
    );
}

/// Without the middleware (or the registry) on the bus, the batch runs with
/// no locking at all — the kernel no longer consults any cross-agent
/// facility of its own accord.
#[tokio::test]
async fn unlocked_agent_runs_without_any_lock_infrastructure() {
    let session: Arc<dyn AgentSession> = Arc::new(InMemoryAgentSession::new());
    let state = AgentState {
        model: Arc::new(UnusedModel),
        tools: vec![Arc::new(BlockedTool {
            name: "plain_tool".into(),
            result: "ran".into(),
        })],
        session,
        extensions: alva_kernel_core::shared::Extensions::new(),
    };

    let config = test_config(None);
    let (event_tx, _event_rx) = tokio::sync::mpsc::unbounded_channel();
    let tool_calls = vec![ToolCall {
        id: "toolu_plain".to_string(),
        name: "plain_tool".to_string(),
        arguments: serde_json::json!({}),
    }];

    let committed = ToolBatchCoordinator::new()
        .execute_batch(
            &state,
            &log_handle(&state),
            &config,
            CancellationToken::new(),
            &tool_calls,
            "llm_parent".to_string(),
            event_tx,
        )
        .await
        .unwrap();

    assert_eq!(committed.len(), 1);
    assert!(!committed[0].result.is_error);
    assert_eq!(committed[0].result.model_text(), "ran");
}
