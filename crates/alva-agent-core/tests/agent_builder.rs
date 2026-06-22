//! Integration tests for `alva_agent_core::Agent` + `AgentBuilder`.

use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use futures_core::Stream;
use tokio_stream::empty;

use alva_agent_core::extension::{PhaseContribution, PhaseHandler, PhaseOrder, Plugin, Registrar};
use alva_agent_core::AgentBuilder;
use alva_kernel_abi::scope::context::ContextLayer;
use alva_kernel_abi::{
    AgentError, AgentMessage, Bus, CompletionResponse, LanguageModel, Message, ModelConfig, Phase,
    PhaseEffect, StreamEvent, Tool, ToolCall,
};
use alva_kernel_core::middleware::{Middleware, MiddlewareError};

/// Stub model used to satisfy `AgentBuilder::model(...)`. Its methods are
/// never actually invoked in these tests — we only exercise the build
/// pipeline, not the agent loop.
struct DummyModel;

#[async_trait]
impl LanguageModel for DummyModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse::from_message(Message::system("ok")))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn Stream<Item = StreamEvent> + Send>> {
        Box::pin(empty())
    }

    fn model_id(&self) -> &str {
        "dummy-model"
    }
}

#[tokio::test]
async fn build_minimal_agent_no_plugins() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .system_prompt("you are a test agent")
        .max_iterations(1)
        .build()
        .await
        .expect("build should succeed");

    // No capabilities have been registered on the bus yet — sanity check
    // that `bus()` returns a real handle whose capability map is empty for
    // an arbitrary marker type.
    assert!(!agent.bus().has::<u32>());
}

#[tokio::test]
async fn builder_requires_model() {
    let result = AgentBuilder::new()
        .system_prompt("no model set")
        .build()
        .await;
    assert!(result.is_err(), "build without model must fail");
}

#[tokio::test]
async fn build_rejects_handle_only_bus_when_plugins_need_registrar() {
    let external_bus = Bus::new();
    let result = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .with_bus(external_bus.handle())
        .plugin(Box::new(ProvidesBusCapabilityPlugin))
        .build()
        .await;

    let err = match result {
        Ok(_) => panic!("handle-only bus must not silently drop plugin provides"),
        Err(err) => err,
    };
    assert!(
        format!("{err}").contains("with_bus_writer"),
        "error should tell callers how to pass a writable bus, got: {err}"
    );
}

struct NamedPlugin;

#[async_trait]
impl Plugin for NamedPlugin {
    fn name(&self) -> &str {
        "named-plugin"
    }

    async fn register(&self, r: &Registrar) {
        r.tool(NamedTool);
        r.middleware(Arc::new(NamedMiddleware));
        r.phase(PhaseContribution::new(
            "named-before-tool",
            Phase::BeforeToolCall,
            PhaseEffect::Decide,
            PhaseOrder::Hooks,
        ));
        r.system_prompt(ContextLayer::AlwaysPresent, "named prompt");
        r.command("named-command", "command from named plugin");
    }

    async fn finalize(&self, _cx: &alva_agent_core::extension::LateContext) -> Vec<Arc<dyn Tool>> {
        vec![Arc::new(LateNamedTool)]
    }
}

struct HandlerPlugin;

#[async_trait]
impl Plugin for HandlerPlugin {
    fn name(&self) -> &str {
        "handler-plugin"
    }

    async fn register(&self, r: &Registrar) {
        r.phase_handler(Arc::new(BlockingBeforeToolHandler));
    }
}

struct ProvidesBusCapabilityPlugin;

#[async_trait]
impl Plugin for ProvidesBusCapabilityPlugin {
    fn name(&self) -> &str {
        "provides-bus-capability"
    }

    async fn register(&self, r: &Registrar) {
        r.provide::<u32>(Arc::new(42));
    }
}

struct BlockingBeforeToolHandler;

#[async_trait]
impl PhaseHandler for BlockingBeforeToolHandler {
    fn contribution(&self) -> PhaseContribution {
        PhaseContribution::new(
            "blocking-before-tool",
            Phase::BeforeToolCall,
            PhaseEffect::Decide,
            PhaseOrder::Hooks,
        )
    }

    async fn before_tool_call(
        &self,
        _state: &mut alva_kernel_core::state::AgentState,
        tool_call: &ToolCall,
    ) -> Result<(), MiddlewareError> {
        Err(MiddlewareError::Blocked {
            reason: format!("blocked {}", tool_call.name),
        })
    }
}

struct InputCommittedPlugin {
    run_start_calls: Arc<AtomicUsize>,
    input_committed_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl Plugin for InputCommittedPlugin {
    fn name(&self) -> &str {
        "input-committed-plugin"
    }

    async fn register(&self, r: &Registrar) {
        r.phase_handler(Arc::new(InputCommittedHandler {
            run_start_calls: self.run_start_calls.clone(),
            input_committed_calls: self.input_committed_calls.clone(),
        }));
    }
}

struct InputCommittedHandler {
    run_start_calls: Arc<AtomicUsize>,
    input_committed_calls: Arc<AtomicUsize>,
}

#[async_trait]
impl PhaseHandler for InputCommittedHandler {
    fn contribution(&self) -> PhaseContribution {
        PhaseContribution::new(
            "tracked-input-committed",
            Phase::InputCommitted,
            PhaseEffect::Observe,
            PhaseOrder::Hooks,
        )
    }

    async fn run_start(
        &self,
        _state: &mut alva_kernel_core::state::AgentState,
    ) -> Result<(), MiddlewareError> {
        self.run_start_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    async fn input_committed(
        &self,
        _state: &mut alva_kernel_core::state::AgentState,
        message: &AgentMessage,
    ) -> Result<(), MiddlewareError> {
        assert!(matches!(message, AgentMessage::Standard(_)));
        self.input_committed_calls.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

struct NamedMiddleware;

#[async_trait]
impl Middleware for NamedMiddleware {
    fn name(&self) -> &str {
        "named-middleware"
    }
}

struct DirectMiddleware;

#[async_trait]
impl Middleware for DirectMiddleware {
    fn name(&self) -> &str {
        "direct-middleware"
    }
}

struct NamedTool;

#[async_trait]
impl Tool for NamedTool {
    fn name(&self) -> &str {
        "named-tool"
    }
    fn description(&self) -> &str {
        "named tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn alva_kernel_abi::ToolExecutionContext,
    ) -> Result<alva_kernel_abi::ToolOutput, AgentError> {
        Ok(alva_kernel_abi::ToolOutput::text("ok"))
    }
}

struct LateNamedTool;

#[async_trait]
impl Tool for LateNamedTool {
    fn name(&self) -> &str {
        "late-named-tool"
    }
    fn description(&self) -> &str {
        "late named tool"
    }
    fn parameters_schema(&self) -> serde_json::Value {
        serde_json::json!({})
    }
    async fn execute(
        &self,
        _input: serde_json::Value,
        _ctx: &dyn alva_kernel_abi::ToolExecutionContext,
    ) -> Result<alva_kernel_abi::ToolOutput, AgentError> {
        Ok(alva_kernel_abi::ToolOutput::text("ok"))
    }
}

#[tokio::test]
async fn build_records_structured_plugin_contributions() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .plugin(Box::new(NamedPlugin))
        .build()
        .await
        .expect("build should succeed");

    let snapshot = agent.assembly_snapshot();
    assert_eq!(snapshot.plugin_names, vec!["named-plugin"]);
    assert_eq!(snapshot.middleware_names, vec!["named-middleware"]);
    assert_eq!(snapshot.plugins.len(), 1);

    let plugin = &snapshot.plugins[0];
    assert_eq!(plugin.name, "named-plugin");
    assert_eq!(plugin.registered_tool_names, vec!["named-tool"]);
    assert_eq!(plugin.finalized_tool_names, vec!["late-named-tool"]);
    assert_eq!(plugin.middleware_names, vec!["named-middleware"]);
    assert_eq!(plugin.phase_contribution_names, vec!["named-before-tool"]);
    assert_eq!(plugin.command_names, vec!["named-command"]);
    assert_eq!(plugin.system_prompt_fragments, 1);
}

#[tokio::test]
async fn build_records_direct_middleware_separately_from_plugin_middleware() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .plugin(Box::new(NamedPlugin))
        .middleware(Arc::new(DirectMiddleware))
        .build()
        .await
        .expect("build should succeed");

    let snapshot = agent.assembly_snapshot();
    assert_eq!(snapshot.direct_middleware_names, vec!["direct-middleware"]);
    assert!(
        snapshot
            .middleware_names
            .iter()
            .any(|name| name == "named-middleware"),
        "plugin middleware should remain in final stack: {:?}",
        snapshot.middleware_names
    );
    assert!(
        snapshot
            .middleware_names
            .iter()
            .any(|name| name == "direct-middleware"),
        "direct middleware should remain in final stack: {:?}",
        snapshot.middleware_names
    );
    assert_eq!(
        snapshot.plugins[0].middleware_names,
        vec!["named-middleware"]
    );
}

#[tokio::test]
async fn phase_handler_contribution_is_recorded_and_executable() {
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .plugin(Box::new(HandlerPlugin))
        .build()
        .await
        .expect("build should succeed");

    let snapshot = agent.assembly_snapshot();
    assert_eq!(
        snapshot.plugins[0].phase_contribution_names,
        vec!["blocking-before-tool"]
    );
    assert!(
        snapshot
            .middleware_names
            .iter()
            .any(|name| name == "phase:blocking-before-tool"),
        "executable phase handler should be compiled into current middleware stack: {:?}",
        snapshot.middleware_names
    );

    let mut state = agent.state().lock().await;
    let config = agent.config().await;
    let tool_call = ToolCall {
        id: "call-1".to_string(),
        name: "shell".to_string(),
        arguments: serde_json::json!({}),
    };
    match config
        .middleware
        .run_before_tool_call(&mut state, &tool_call)
        .await
    {
        Err(MiddlewareError::Blocked { reason }) => assert_eq!(reason, "blocked shell"),
        other => panic!("expected executable phase handler to block, got {other:?}"),
    }
}

#[tokio::test]
async fn input_committed_phase_handler_runs_only_on_input_commit() {
    let run_start_calls = Arc::new(AtomicUsize::new(0));
    let input_committed_calls = Arc::new(AtomicUsize::new(0));
    let agent = AgentBuilder::new()
        .model(Arc::new(DummyModel))
        .plugin(Box::new(InputCommittedPlugin {
            run_start_calls: run_start_calls.clone(),
            input_committed_calls: input_committed_calls.clone(),
        }))
        .build()
        .await
        .expect("build should succeed");

    let mut state = agent.state().lock().await;
    let config = agent.config().await;
    config
        .middleware
        .run_on_agent_start(&mut state)
        .await
        .expect("run start should not trigger input committed");
    assert_eq!(run_start_calls.load(Ordering::SeqCst), 0);
    assert_eq!(input_committed_calls.load(Ordering::SeqCst), 0);

    config
        .middleware
        .run_input_committed(
            &mut state,
            &AgentMessage::Standard(Message::user("committed input")),
        )
        .await
        .expect("input committed phase should run");
    assert_eq!(input_committed_calls.load(Ordering::SeqCst), 1);
}

// Roster integration tests previously lived here. Roster types have moved
// to `alva-app-core::roster` (harness layer) and are no longer wired into
// `AgentBuilder` — see `crates/alva-app-core/src/roster.rs` for the unit
// tests of `MultiagentRoster::validate`, `RosterEntry`, and the
// `MultiagentRosterCap` bus capability. Whichever harness plugin
// (e.g. `SubAgentPlugin`) wants to publish the cap calls
// `bus_writer.provide(Arc::new(MultiagentRosterCap { ... }))` itself.
