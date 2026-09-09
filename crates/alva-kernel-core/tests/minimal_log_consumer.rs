// INPUT:  alva_kernel_abi::ConversationLog (ONLY), alva_kernel_core::{AgentState, AgentConfig, run_agent}
// OUTPUT: R05 acceptance — a minimal consumer implementing ONLY ConversationLog
//         runs the kernel for one text turn.
// POS:    Proves the kernel entry no longer requires SessionStore (query/
//         rollback/snapshot/subscribe/compact/persist): this log has no
//         AgentSession/SessionStore impl and no dependency on SDK-assembly
//         or product crates.

use std::collections::VecDeque;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use alva_kernel_abi::agent_session::SessionError;
use alva_kernel_abi::base::message::{Message, MessageRole};
use alva_kernel_abi::base::stream::StreamEvent;
use alva_kernel_abi::model::LanguageModel;
use alva_kernel_abi::session_record::{AppendReceipt, SessionRecord};
use alva_kernel_abi::tool::execution::ToolExecutionContext;
use alva_kernel_abi::tool::{Tool, ToolOutput};
use alva_kernel_abi::{
    AgentError, AgentMessage, CancellationToken, CompletionResponse, ConversationLog, ModelConfig,
    SessionEvent,
};

/// A 20-line conversation log: message vector + event counter. Implements
/// ONLY `ConversationLog` — deliberately no `AgentSession`/`SessionStore`.
struct MinimalLog {
    session_id: String,
    messages: RwLock<VecDeque<AgentMessage>>,
    events: RwLock<Vec<SessionEvent>>,
    seq: AtomicU64,
}

use std::sync::atomic::{AtomicU64, Ordering};

impl MinimalLog {
    fn new() -> Self {
        Self {
            session_id: uuid::Uuid::new_v4().to_string(),
            messages: RwLock::new(VecDeque::new()),
            events: RwLock::new(Vec::new()),
            seq: AtomicU64::new(1),
        }
    }
}

#[async_trait]
impl ConversationLog for MinimalLog {
    fn session_id(&self) -> &str {
        &self.session_id
    }

    async fn messages(&self) -> Result<Vec<AgentMessage>, SessionError> {
        Ok(self.messages.read().await.iter().cloned().collect())
    }

    async fn recent_messages(&self, n: usize) -> Result<Vec<AgentMessage>, SessionError> {
        let msgs = self.messages.read().await;
        let len = msgs.len();
        Ok(if n >= len {
            msgs.iter().cloned().collect()
        } else {
            msgs.iter().skip(len - n).cloned().collect()
        })
    }

    async fn append_message(
        &self,
        msg: AgentMessage,
        parent_uuid: Option<String>,
    ) -> Result<(), SessionError> {
        let mut event = SessionEvent::new_runtime(match &msg {
            AgentMessage::Standard(m) => match m.role {
                MessageRole::User => "user",
                MessageRole::Assistant => "assistant",
                MessageRole::System => "system",
                MessageRole::Tool => "tool_result",
            },
            _ => "message",
        });
        event.parent_uuid = parent_uuid;
        event.data = Some(serde_json::to_value(&msg).unwrap_or(serde_json::Value::Null));
        event.seq = self.seq.fetch_add(1, Ordering::SeqCst);
        self.events.write().await.push(event);
        self.messages.write().await.push_back(msg);
        Ok(())
    }

    async fn append(&self, event: SessionEvent) -> Result<(), SessionError> {
        let mut event = event;
        event.seq = self.seq.fetch_add(1, Ordering::SeqCst);
        self.events.write().await.push(event);
        Ok(())
    }

    async fn append_records(
        &self,
        records: Vec<SessionRecord>,
    ) -> Result<AppendReceipt, SessionError> {
        let first = self.seq.load(Ordering::SeqCst);
        let mut events = self.events.write().await;
        for mut record in records {
            record.seq = self.seq.fetch_add(1, Ordering::SeqCst);
            events.push(record.into_event());
        }
        let last = self.seq.load(Ordering::SeqCst) - 1;
        Ok(AppendReceipt {
            first_seq: first,
            last_seq: last,
            count: (last - first + 1) as usize,
        })
    }

    fn supports_atomic_records(&self) -> bool {
        true
    }

    async fn flush(&self) -> Result<(), SessionError> {
        Ok(())
    }
}

/// Model that answers one plain-text turn.
struct TextModel;

#[async_trait]
impl LanguageModel for TextModel {
    fn model_id(&self) -> &str {
        "minimal-text-model"
    }
    fn provider_id(&self) -> &str {
        "test"
    }
    async fn complete(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> Result<CompletionResponse, AgentError> {
        Ok(CompletionResponse {
            message: Message {
                id: "m1".into(),
                role: MessageRole::Assistant,
                content: vec![alva_kernel_abi::ContentBlock::Text {
                    text: "done".into(),
                }],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            },
            raw: None,
        })
    }
    fn stream(
        &self,
        _messages: &[Message],
        _tools: &[&dyn Tool],
        _config: &ModelConfig,
    ) -> std::pin::Pin<Box<dyn futures_core::stream::Stream<Item = StreamEvent> + Send>> {
        Box::pin(tokio_stream::iter(vec![
            StreamEvent::Start,
            StreamEvent::Done,
        ]))
    }
}

#[tokio::test]
async fn minimal_conversation_log_runs_one_text_turn() {
    fn assert_conversation_log<T: ConversationLog>() {}
    assert_conversation_log::<MinimalLog>();

    let log = Arc::new(MinimalLog::new());
    let state = alva_kernel_core::state::AgentState {
        model: Arc::new(TextModel),
        tools: vec![],
        session: log.clone() as Arc<dyn ConversationLog>,
        extensions: alva_kernel_core::shared::Extensions::new(),
    };
    let config = alva_kernel_core::state::AgentConfig {
        middleware: alva_kernel_core::middleware::MiddlewareStack::new(),
        system_prompt: vec![],
        max_iterations: 5,
        model_config: ModelConfig::default(),
        context_window: 0,
        workspace: None,
        bus: None,
        context_system: None,
        context_token_budget: None,
    };

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    alva_kernel_core::run_agent(
        &state,
        &config,
        CancellationToken::new(),
        vec![AgentMessage::Standard(Message::user("hi"))],
        tx,
    )
    .await
    .expect("one text turn completes on a minimal log");
    while rx.recv().await.is_some() {}

    let messages = log.messages().await.unwrap();
    let roles: Vec<MessageRole> = messages
        .iter()
        .filter_map(|m| match m {
            AgentMessage::Standard(m) => Some(m.role.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(roles, vec![MessageRole::User, MessageRole::Assistant]);
}
