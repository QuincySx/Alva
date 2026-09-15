// INPUT: BaseAgent + real CompactionMiddleware, scripted model, three InMemoryAgentSessions
// OUTPUT: R10 combination evidence — across an A→B→C→B swap chain the compaction marker,
//         the rewritten message projection and the durable records land in the session that is
//         CURRENT when compaction fires (B on the way back), never in a stale one
// POS:    Session-switch × compaction combination test (R48's R10 supplement). The middleware
//         captures the PersistentSessions narrow capability once at configure time and must
//         resolve the CURRENT session on every write — this test pins that property end to end.

use alva_kernel_abi::{
    AgentMessage, EventQuery, LanguageModel, Message, ModelConfig, StreamEvent,
};
use std::pin::Pin;
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

/// Turns stream a fixed text; `complete` answers summarization calls with a
/// distinctive, assertable summary. Two independent counters keep the main
/// loop and the summarizer decoupled.
struct ScriptedModel {
    turns: AtomicUsize,
    summaries: AtomicUsize,
}

#[async_trait::async_trait]
impl LanguageModel for ScriptedModel {
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn alva_kernel_abi::Tool],
        _config: &ModelConfig,
    ) -> Result<alva_kernel_abi::CompletionResponse, alva_kernel_abi::AgentError> {
        let n = self.summaries.fetch_add(1, Ordering::SeqCst);
        Ok(alva_test::mock_provider::MockLanguageModel::new()
            .with_response(Message {
                id: format!("summary-{n}"),
                role: alva_kernel_abi::MessageRole::Assistant,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: format!("B-history-summary-{n}"),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            })
            .complete(_messages, _tools, _config)
            .await
            .expect("mock complete"))
    }

    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn alva_kernel_abi::Tool],
        _config: &ModelConfig,
    ) -> Pin<Box<dyn futures::Stream<Item = StreamEvent> + Send>> {
        let n = self.turns.fetch_add(1, Ordering::SeqCst);
        Box::pin(futures::stream::iter([
            StreamEvent::Start,
            StreamEvent::TextDelta {
                text: format!("reply-{n}"),
            },
            StreamEvent::Done,
        ]))
    }

    fn model_id(&self) -> &str {
        "r10-scripted-model"
    }
}

async fn run_turn(agent: &alva_app_core::BaseAgent, prompt: &str) {
    let events = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        async move {
            alva_test::assertions::collect_events(agent.prompt_text(prompt).await).await
        },
    )
    .await
    .expect("turn must finish");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, alva_kernel_core::AgentEvent::AgentEnd { error: None })),
        "turn must end cleanly: {events:?}"
    );
}

fn compaction_markers(session: &dyn alva_kernel_abi::agent_session::AgentSession) -> usize {
    let records = tokio::task::block_in_place(|| {
        futures::executor::block_on(session.records(0))
    });
    records
        .iter()
        .filter(|record| record.type_name == "compaction")
        .count()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn compaction_across_swap_chain_lands_in_the_current_session() {
    let tmp = tempfile::tempdir().unwrap();
    let model = Arc::new(ScriptedModel {
        turns: AtomicUsize::new(0),
        summaries: AtomicUsize::new(0),
    });
    // trigger_tokens floors at 1, so any turn whose conversation already
    // carries 3+ messages compacts; fresh sessions (1 message) do not.
    let compaction = Arc::new(
        alva_agent_context::middleware::CompactionMiddleware::new(
            alva_agent_context::middleware::CompactionConfig {
                trigger_tokens: 1,
                reserve_tokens: 0,
                keep_recent_tokens: 1,
            },
        ),
    );
    let agent = Arc::new(
        alva_app_core::BaseAgent::builder()
            .workspace(tmp.path())
            .middleware(compaction.clone())
            .build(model)
            .await
            .unwrap(),
    );

    let session_a: Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
        alva_agent_core::agent_session::InMemoryAgentSession::with_id("r10-a".into()),
    );
    let session_b: Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
        alva_agent_core::agent_session::InMemoryAgentSession::with_id("r10-b".into()),
    );
    let session_c: Arc<dyn alva_kernel_abi::agent_session::AgentSession> = Arc::new(
        alva_agent_core::agent_session::InMemoryAgentSession::with_id("r10-c".into()),
    );

    // A → B → C → B.
    run_turn(&agent, "turn one in A").await;
    assert!(agent.swap_session(session_b.clone()).await, "swap to B");
    run_turn(&agent, "turn two in B").await;
    assert!(agent.swap_session(session_c.clone()).await, "swap to C");
    run_turn(&agent, "turn three in C").await;
    assert!(agent.swap_session(session_b.clone()).await, "swap back to B");

    // B now carries [assistant2, user4] of conversation — 3+ messages at the
    // next before_llm_call, so THIS turn compacts, in B.
    run_turn(&agent, "turn four in B triggers compaction").await;

    assert_eq!(
        compaction.compaction_count(),
        1,
        "compaction must have fired exactly once across the chain"
    );

    // The marker and the rewritten projection landed in B — the CURRENT
    // session — not in A or C.
    let current = agent.session_handle().await;
    assert_eq!(current.session_id(), "r10-b");
    assert_eq!(
        compaction_markers(current.as_ref()),
        1,
        "B carries exactly one compaction marker"
    );
    let b_texts: Vec<String> = current
        .messages()
        .await
        .unwrap()
        .iter()
        .map(|m: &AgentMessage| match m {
            AgentMessage::Standard(m) => m.text_content(),
            _ => String::new(),
        })
        .collect();
    assert!(
        b_texts
            .iter()
            .any(|text| text.contains("B-history-summary-0")),
        "the rewritten projection must contain the summarizer output: {b_texts:?}"
    );

    let query = EventQuery::default();
    for (label, stale) in [("A", &session_a), ("C", &session_c)] {
        let matches = stale.query(&query).await;
        assert!(
            matches
                .iter()
                .all(|em| em.event.event_type != "compaction"),
            "{label} must not carry the compaction marker"
        );
    }
    assert_eq!(
        compaction_markers(session_a.as_ref()),
        0,
        "A must be untouched"
    );
    assert_eq!(
        compaction_markers(session_c.as_ref()),
        0,
        "C must be untouched"
    );
}
