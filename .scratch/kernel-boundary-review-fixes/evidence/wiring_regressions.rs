// INPUT: production plugin and subagent assembly, deterministic mock model
// OUTPUT: temporary review regression probes
// POS: Verify production wiring after the kernel-boundary migration.
use std::sync::{Arc, atomic::{AtomicBool, Ordering}};
use std::time::Duration;
use alva_kernel_abi::{Tool, ToolExecutionContext, ToolOutput, ToolSchemaContext, Scheduling, ResourceKey, ExecutionMode};
use alva_app_core::extension::{AnalyticsPlugin, SubAgentPlugin, ToolLockRegistryPlugin};
use alva_test::{mock_provider::MockLanguageModel, fixtures::{make_assistant_message, make_tool_call_message}, assertions::collect_events};

#[tokio::test]
async fn production_analytics_plugin_records_llm_call() {
    let tmp = tempfile::tempdir().unwrap();
    let path = tmp.path().join("analytics.jsonl");
    let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("done")));
    let agent = alva_agent_core::Agent::builder().workspace(tmp.path()).model(model)
        .plugin(Box::new(AnalyticsPlugin::new().with_path(&path))).build().await.unwrap();
    use futures::StreamExt;
    let mut events = std::pin::pin!(agent.run(vec![alva_kernel_abi::AgentMessage::Standard(alva_kernel_abi::Message::user("hi"))], alva_kernel_abi::CancellationToken::new()));
    while events.next().await.is_some() {}
    let contents = std::fs::read_to_string(path).unwrap();
    assert!(contents.contains("llm_call"), "production plugin wrote no LLM event: {contents:?}");
}

struct LockedTool(Arc<AtomicBool>);
#[async_trait::async_trait]
impl Tool for LockedTool {
    fn name(&self) -> &str { "review_locked_tool" }
    fn description(&self) -> &str { "Record a synthetic resource access" }
    fn parameters_schema(&self, _: &ToolSchemaContext) -> serde_json::Value { serde_json::json!({"type":"object"}) }
    fn scheduling(&self, _: &serde_json::Value) -> Scheduling {
        Scheduling::default().with_resource_keys(vec![ResourceKey::write("review-resource")])
    }
    async fn execute(&self, _: serde_json::Value, _: &dyn ToolExecutionContext) -> Result<ToolOutput, alva_kernel_abi::AgentError> {
        self.0.store(true, Ordering::SeqCst);
        Ok(ToolOutput::text("done"))
    }
}

#[tokio::test]
async fn spawned_child_obeys_shared_tool_lock() {
    let tmp = tempfile::tempdir().unwrap();
    let called = Arc::new(AtomicBool::new(false));
    let registry = Arc::new(alva_agent_core::ToolLockRegistry::new().with_acquire_timeout(Duration::from_millis(20)));
    let held = registry.acquire(&[], ExecutionMode::SerialGlobal).await;
    let model = Arc::new(MockLanguageModel::new()
        .with_response(make_tool_call_message("agent", serde_json::json!({"task":"use the granted tool", "role":"worker", "tools":["review_locked_tool"]})))
        .with_response(make_tool_call_message("review_locked_tool", serde_json::json!({})))
        .with_response(make_assistant_message("child done"))
        .with_response(make_assistant_message("parent done")));
    let agent = alva_app_core::BaseAgent::builder().workspace(tmp.path())
        .tool(Box::new(LockedTool(called.clone())))
        .plugin(Box::new(ToolLockRegistryPlugin::with_registry(registry)))
        .plugin(Box::new(SubAgentPlugin::new(3, Duration::from_secs(5))))
        .max_iterations(6).build(model).await.unwrap();
    let events = tokio::time::timeout(Duration::from_secs(10), async {
        collect_events(agent.prompt_text("spawn a worker").await).await
    }).await.unwrap();
    assert!(events.iter().any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })), "parent must finish");
    assert!(!called.load(Ordering::SeqCst), "child executed while another agent held the global write lock");
    drop(held);
}

struct FailedStartSession(alva_agent_core::agent_session::InMemoryAgentSession);
#[async_trait::async_trait]
impl alva_kernel_abi::ConversationLog for FailedStartSession {
    fn session_id(&self) -> &str { self.0.session_id() }
    async fn messages(&self) -> Result<Vec<alva_kernel_abi::AgentMessage>, alva_kernel_abi::SessionError> { self.0.messages().await }
    async fn recent_messages(&self, n: usize) -> Result<Vec<alva_kernel_abi::AgentMessage>, alva_kernel_abi::SessionError> { self.0.recent_messages(n).await }
    async fn append_message(&self, m: alva_kernel_abi::AgentMessage, p: Option<String>) -> Result<(), alva_kernel_abi::SessionError> { self.0.append_message(m, p).await }
    async fn append(&self, _: alva_kernel_abi::SessionEvent) -> Result<(), alva_kernel_abi::SessionError> { Err(alva_kernel_abi::SessionError::Other("injected write failure".into())) }
    async fn append_records(&self, r: Vec<alva_kernel_abi::SessionRecord>) -> Result<alva_kernel_abi::AppendReceipt, alva_kernel_abi::SessionError> { self.0.append_records(r).await }
    fn supports_atomic_records(&self) -> bool { true }
    async fn flush(&self) -> Result<(), alva_kernel_abi::SessionError> { Ok(()) }
}
#[async_trait::async_trait]
impl alva_kernel_abi::SessionStore for FailedStartSession {
    fn parent_session_id(&self) -> Option<&str> { None }
    async fn query(&self, q: &alva_kernel_abi::EventQuery) -> Vec<alva_kernel_abi::EventMatch> { self.0.query(q).await }
    async fn count(&self, q: &alva_kernel_abi::EventQuery) -> usize { self.0.count(q).await }
    async fn rollback_after(&self, u: &str) -> usize { self.0.rollback_after(u).await }
    async fn save_snapshot(&self, d: &[u8]) { self.0.save_snapshot(d).await }
    async fn load_snapshot(&self) -> Option<Vec<u8>> { self.0.load_snapshot().await }
    async fn restore(&self) -> Result<(), alva_kernel_abi::SessionError> { Ok(()) }
    async fn close(&self) -> Result<(), alva_kernel_abi::SessionError> { Ok(()) }
    async fn clear(&self) -> Result<(), alva_kernel_abi::SessionError> { self.0.clear().await }
    async fn compact_messages(&self, m: Vec<alva_kernel_abi::AgentMessage>) { self.0.compact_messages(m).await }
}
impl alva_kernel_abi::AgentSession for FailedStartSession {}

#[tokio::test]
async fn failed_run_start_still_emits_terminal_event() {
    use futures::StreamExt;
    let model = Arc::new(MockLanguageModel::new().with_response(make_assistant_message("done")));
    let agent = alva_agent_core::Agent::builder().model(model)
        .session(Arc::new(FailedStartSession(alva_agent_core::agent_session::InMemoryAgentSession::new())))
        .build().await.unwrap();
    let events: Vec<_> = agent.run(vec![], alva_kernel_abi::CancellationToken::new()).collect().await;
    assert!(events.iter().any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: Some(_) })), "write failure silently closes event stream: {events:?}");
}
