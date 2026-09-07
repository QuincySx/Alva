// INPUT:  async_trait, crate::{AgentMessage, SessionError, SessionEvent, SessionRecord, AppendReceipt}
// OUTPUT: ConversationLog trait
// POS:    对话记录契约 —— 内核对"历史"的全部要求：读得到消息、追加得进消息。
//         会话存储（查询/回滚/快照/订阅/压缩等）见 session_store；那是 harness
//         与 UI 的超集需求，不是内核的依赖。见 CONTEXT.md 的定义。

use async_trait::async_trait;

use crate::agent_session::{SessionError, SessionEvent};
use crate::base::message::AgentMessage;
use crate::session_record::{AppendReceipt, SessionRecord};

/// 对话记录（Conversation Log）—— 内核循环对历史的全部要求。
///
/// **读得到消息、追加得进消息**，外加内核自己那部分簿记的落地：循环期间
/// 产生的骨架事件（run_start / llm_call_* / tool_use…）与 typed 记录
/// （tool intent/outcome）也经由这里追加，周期性 durability 由 `flush`
/// 同步。仅此而已 —— 不要求可查询、可回滚、可快照、可订阅。
///
/// 一个二十行的内存实现就能满足这个契约；远端/持久化后端同样可以实现它
/// （内部的写队列与重试属于实现细节）。
#[async_trait]
pub trait ConversationLog: Send + Sync {
    /// Unique identifier for this session.
    fn session_id(&self) -> &str;

    /// All messages in append order.
    async fn messages(&self) -> Vec<AgentMessage>;

    /// The last N messages, in append order.
    async fn recent_messages(&self, n: usize) -> Vec<AgentMessage>;

    /// Append a conversation message (user / assistant / tool result).
    async fn append_message(&self, msg: AgentMessage, parent_uuid: Option<String>);

    /// Append a raw session event (the kernel's run skeleton bookkeeping).
    /// The backend assigns `event.seq` atomically.
    async fn append(&self, event: SessionEvent);

    /// Commit one or more typed records as one visible state transition.
    /// Backends that cannot make the batch atomic reject the operation —
    /// see [`crate::AgentSession::supports_atomic_records`].
    async fn append_records(
        &self,
        records: Vec<SessionRecord>,
    ) -> Result<AppendReceipt, SessionError>;

    /// Whether [`append_records`](Self::append_records) commits atomically.
    fn supports_atomic_records(&self) -> bool;

    /// Durability sync point: after this returns, every append issued before
    /// the call must be durably persisted.
    async fn flush(&self) -> Result<(), SessionError>;
}
