// INPUT:  async_trait, crate::{AgentSession, AgentMessage, SessionError, SessionEvent, SessionEventStream, EventMatch, EventQuery, SessionRecord, AppendReceipt}
// OUTPUT: SessionStore trait
// POS:    会话存储契约 —— harness 与 UI 对会话的要求：查询、计数、回滚、快照、
//         订阅、压缩、持久化、子会话派生。这是 ConversationLog 的超集需求，
//         但不是内核的依赖（见 CONTEXT.md）。

use async_trait::async_trait;

use crate::agent_session::{
    AgentSession, EventMatch, EventQuery, SessionError, SessionEventStream,
};
use crate::base::message::AgentMessage;
use crate::session_record::SessionRecord;

/// 会话存储（Session Store）—— harness 与 UI 对会话的要求。
///
/// 查询、计数、回滚、快照、订阅、压缩、持久化。这是
/// [`ConversationLog`](crate::ConversationLog) 的**超集**：能实现 Session
/// Store 的东西一定能当 Conversation Log 用，反之不必。
#[async_trait]
pub trait SessionStore: Send + Sync {
    /// Parent session id for sub-agents. `None` for root sessions.
    fn parent_session_id(&self) -> Option<&str>;

    /// Create an independently persisted child session and commit its causal
    /// link before the child performs external work. Backends without a
    /// repository return `None`; callers may use an isolated in-memory child.
    async fn create_child_session(
        &self,
        _parent_run_id: &str,
        _spawn_tool_call_id: &str,
    ) -> Result<Option<std::sync::Arc<dyn AgentSession>>, SessionError> {
        Ok(None)
    }

    /// Query events matching the filter, ordered by `seq` ascending.
    async fn query(&self, filter: &EventQuery) -> Vec<EventMatch>;

    /// Count events matching the filter.
    async fn count(&self, filter: &EventQuery) -> usize;

    /// Read the canonical typed projection in total durable sequence order.
    async fn records(&self, from_seq: u64) -> Vec<SessionRecord> {
        self.query(&EventQuery::default())
            .await
            .into_iter()
            .filter(|matched| matched.event.seq > from_seq)
            .map(|matched| SessionRecord::from_event(matched.event))
            .collect()
    }

    /// Return messages whose source event has `seq > after_seq`, in append
    /// order. Used by remote backends to incrementally sync a local mirror.
    ///
    /// The default implementation scans the event log via `query` and
    /// deserializes the `AgentMessage` out of each event's `data` field.
    /// Network-backed backends should override with an efficient
    /// single-roundtrip query.
    async fn messages_since(&self, after_seq: u64) -> Vec<AgentMessage> {
        let matches = self
            .query(&crate::agent_session::EventQuery::default())
            .await;
        matches
            .into_iter()
            .filter(|m| m.event.seq > after_seq)
            .filter_map(|m| {
                m.event
                    .data
                    .and_then(|d| serde_json::from_value::<AgentMessage>(d).ok())
            })
            .collect()
    }

    /// Subscribe to the session's event stream, returning a stream that
    /// yields (in order): all events currently in the log with
    /// `seq > from_seq`, then — on backends that support live tailing —
    /// all subsequently appended events.
    ///
    /// The default implementation covers the history replay only and ends
    /// immediately after. Live-tailing backends MUST override to provide
    /// both halves with no gap or duplication at the boundary. Dropping the
    /// returned stream unsubscribes.
    async fn subscribe_events(&self, from_seq: u64) -> SessionEventStream {
        let matches = self
            .query(&crate::agent_session::EventQuery::default())
            .await;
        let history: Vec<crate::agent_session::SessionEvent> = matches
            .into_iter()
            .filter(|m| m.event.seq > from_seq)
            .map(|m| m.event)
            .collect();
        Box::pin(futures_util::stream::iter(history))
    }

    /// Drop all events with `seq` greater than the event identified by `uuid`.
    /// Returns the number of events dropped.
    async fn rollback_after(&self, uuid: &str) -> usize;

    /// Store an opaque snapshot blob (used by `ContextStore` for L0..L3 state).
    async fn save_snapshot(&self, data: &[u8]);

    /// Load the most recent snapshot, if any.
    async fn load_snapshot(&self) -> Option<Vec<u8>>;

    /// Warm internal projections from durable storage. Called once after
    /// construction; idempotent.
    async fn restore(&self) -> Result<(), SessionError>;

    /// Flush then release resources. Callers MUST stop writing after it returns.
    async fn close(&self) -> Result<(), SessionError>;

    /// Drop all events and snapshots (explicit user-initiated reset or tests;
    /// never called by the runtime during normal operation).
    async fn clear(&self) -> Result<(), SessionError>;

    /// Replace the live conversation projection with `replacement` while
    /// PRESERVING the append-only event log. This is what compaction uses
    /// instead of `clear()`.
    ///
    /// 无默认实现：清空整个日志是存储层的动词（`clear`），压缩要的只是
    /// 替换消息投影。把"清空重来"当作压缩的默认实现，正是拆分前
    /// Inspector / 评估 / 审计 / 因果图在压缩后全部断掉的原因。
    async fn compact_messages(&self, replacement: Vec<AgentMessage>);
}
