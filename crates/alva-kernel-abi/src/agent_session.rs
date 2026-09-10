// INPUT:  async_trait, serde, serde_json, uuid, chrono, futures_util, std,
//         crate::AgentMessage
// OUTPUT: AgentSession (= ConversationLog + SessionStore) trait, StoreSessionCell
//         (shared "current full session" handle), PersistentSessions capability
//         bridge (R18), SessionEvent, SessionMessage, EventEmitter, EmitterKind,
//         ComponentDescriptor, ScopedSession, SessionError, EventQuery, EventMatch,
//         SessionEventListener, SessionEventStream, typed-record capability via
//         AppendReceipt/SessionRecord
// POS:    Unified session CONTRACT — ConversationLog（内核入口要求）+ SessionStore
//         （harness/UI 超集）组合；具体内存后端在 alva-agent-core。StoreSessionCell
//         供装配层经 PersistentSessions 桥暴露"当前会话"，swap 原地更新。

use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};

use crate::conversation_log::ConversationLog;
use crate::session_store::SessionStore;

// ===========================================================================
// Errors
// ===========================================================================

/// Errors that can be returned from `AgentSession` lifecycle methods.
#[derive(Debug, thiserror::Error)]
pub enum SessionError {
    #[error("session I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("session serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("session not found: {0}")]
    NotFound(String),

    #[error("session error: {0}")]
    Other(String),
}

// ===========================================================================
// Emitter identity
// ===========================================================================

/// Identifies who wrote a session event.
///
/// The `kind` is set by the extension point (Runtime / Tool / Middleware /
/// Extension); the `id` is the stable name of the concrete component within
/// that kind (e.g. `read_file` for a tool, `loop_detection` for a middleware).
/// Third-party code NEVER sets these fields directly — the scoped session
/// wrapper injects them when events are appended through it.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EventEmitter {
    pub kind: EmitterKind,
    pub id: String,
    pub instance: Option<String>,
}

impl EventEmitter {
    /// Runtime emitter used for kernel-core skeleton events.
    pub fn runtime() -> Self {
        Self {
            kind: EmitterKind::Runtime,
            id: "kernel_core".to_string(),
            instance: None,
        }
    }
}

/// Base categories for event emitters. Use `Other` for future extension points
/// that do not fit into the existing kinds — once a new kind stabilizes it can
/// be promoted to a named variant.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum EmitterKind {
    /// kernel-core runtime itself — used for skeleton events only.
    Runtime,
    /// A `Tool` during `Tool::execute`.
    Tool,
    /// A `Middleware` during one of its hook methods.
    Middleware,
    /// An `Extension` during its lifecycle or a service it provides.
    Extension,
    /// Escape hatch for future extension points not currently modeled.
    Other(String),
}

/// Descriptor for a runtime component. Registered once per run via a
/// `component_registry` event at `run_start`; subsequent events only carry
/// the lightweight `EventEmitter { kind, id, instance }` and can be joined
/// back to this descriptor for display.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComponentDescriptor {
    pub kind: EmitterKind,
    pub id: String,
    pub name: String,
}

// ===========================================================================
// Events
// ===========================================================================

/// A single event in a session's append-only log.
///
/// `seq` is assigned by the backend at `append` time, not by the caller.
/// Callers constructing `SessionEvent` instances should leave `seq` at 0;
/// the backend overwrites it before persisting. Readers always order by
/// `seq`, never by `timestamp`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionEvent {
    /// Strictly monotonic within a session. 0 means "not yet assigned".
    pub seq: u64,

    /// Unique id for this event.
    pub uuid: String,

    /// Causal parent (e.g. `tool_result.parent_uuid == tool_use.uuid`).
    pub parent_uuid: Option<String>,

    /// Wall-clock epoch millis. Display only.
    pub timestamp: i64,

    /// Event type discriminator.
    #[serde(rename = "type")]
    pub event_type: String,

    /// Who wrote the event. Filled by `ScopedSession` at the extension point.
    pub emitter: EventEmitter,

    /// Message payload for user/assistant/tool_result events.
    pub message: Option<SessionMessage>,

    /// Arbitrary JSON payload for non-message events.
    pub data: Option<serde_json::Value>,
}

/// A conversation message embedded inside a `SessionEvent`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMessage {
    /// "user" | "assistant" | "tool"
    pub role: String,
    /// Content — a string or a content-block array.
    pub content: serde_json::Value,
}

impl SessionEvent {
    /// Construct a raw event with a fresh uuid and current timestamp.
    /// `seq` is 0 (will be overwritten by the backend).
    /// `emitter` is a placeholder — `ScopedSession::append` overwrites it.
    fn new(event_type: impl Into<String>) -> Self {
        Self {
            seq: 0,
            uuid: uuid::Uuid::new_v4().to_string(),
            parent_uuid: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
            event_type: event_type.into(),
            emitter: EventEmitter::runtime(),
            message: None,
            data: None,
        }
    }

    /// Construct a user-message event.
    pub fn user_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("user");
        e.message = Some(SessionMessage {
            role: "user".into(),
            content,
        });
        e
    }

    /// Construct an assistant-message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("assistant");
        e.message = Some(SessionMessage {
            role: "assistant".into(),
            content,
        });
        e
    }

    /// Construct a tool_result event linked to a parent tool_use uuid.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        let mut e = Self::new("tool_result");
        e.parent_uuid = Some(parent_tool_use_uuid.to_string());
        e.message = Some(SessionMessage {
            role: "tool".into(),
            content,
        });
        e
    }

    /// Construct a progress event with arbitrary data.
    pub fn progress(data: serde_json::Value) -> Self {
        let mut e = Self::new("progress");
        e.data = Some(data);
        e
    }

    /// Construct a system event with arbitrary data.
    pub fn system(data: serde_json::Value) -> Self {
        let mut e = Self::new("system");
        e.data = Some(data);
        e
    }

    /// Construct a runtime event with the given `event_type`. Used by
    /// kernel-core to emit skeleton events.
    pub fn new_runtime(event_type: impl Into<String>) -> Self {
        Self::new(event_type)
    }
}

// ===========================================================================
// Query
// ===========================================================================

/// Filter for `AgentSession::query`. All fields optional; `None` means "don't
/// filter on this field".
#[derive(Debug, Clone, Default)]
pub struct EventQuery {
    pub event_type: Option<String>,
    pub role: Option<String>,
    pub text_contains: Option<String>,
    pub after_uuid: Option<String>,
    pub last_n: Option<usize>,
    pub limit: usize,
}

/// A query result with a short preview text for display.
#[derive(Debug, Clone)]
pub struct EventMatch {
    pub event: SessionEvent,
    pub preview: String,
}

// ===========================================================================
// AgentSession trait
// ===========================================================================

/// 统一会话契约 —— [`ConversationLog`] 与 [`SessionStore`] 的组合。
///
/// 这是 2026-09 内核边界重构（ticket 05/06）拆分后的收口形态：
/// - [`ConversationLog`]：内核循环需要的最小面（读消息、追加、flush），
///   方法一律可失败；
/// - [`SessionStore`]：harness 与 UI 的超集需求（查询、回滚、快照、订阅、
///   压缩、持久化、子会话派生）。
///
/// 想跑通内核，实现 [`ConversationLog`] 就够；想给 Inspector / UI 提供完整
/// 会话面，再实现 [`SessionStore`]。原来那个三十方法的巨物不再单独存在。
pub trait AgentSession: ConversationLog + SessionStore {}

/// R05/R09: 完整会话（SessionStore 能力）的**当前**句柄。装配层在构建时创建
/// 并注入 `state.extensions` 与 MiddlewareContext；`swap_session` 等会话切换
/// 通过 [`set`](Self::set) 原地更新，所有持有者（压缩中间件、工具上下文的
/// create_child_session、session_handle）读取时总是拿到当前会话。
#[derive(Clone)]
pub struct StoreSessionCell(Arc<std::sync::RwLock<Arc<dyn AgentSession>>>);

impl StoreSessionCell {
    pub fn new(session: Arc<dyn AgentSession>) -> Self {
        Self(Arc::new(std::sync::RwLock::new(session)))
    }

    /// 当前完整会话。
    pub fn get(&self) -> Arc<dyn AgentSession> {
        self.0
            .read()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
            .clone()
    }

    /// 切换当前完整会话（R10：swap_session 必须经此更新）。
    pub fn set(&self, session: Arc<dyn AgentSession>) {
        *self
            .0
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = session;
    }
}

// ===========================================================================
// Session event stream
// ===========================================================================

/// Stream of `SessionEvent`s. Returned by `AgentSession::subscribe_events`.
pub type SessionEventStream = futures_util::stream::BoxStream<'static, SessionEvent>;

// ===========================================================================
// ScopedSession
// ===========================================================================

/// A wrapper around an `AgentSession` handle that stamps every appended event
/// with a fixed `EventEmitter`. Third-party tools, middleware, and extensions
/// receive a `ScopedSession` from their execution context — they can append
/// events without ever touching the `emitter` field, because the wrapper
/// fills it at construction time.
///
/// This is the structural guarantee that `emitter.kind` always matches the
/// actual call path: runtime constructs the `ScopedSession` with the correct
/// kind and id for each extension point, and the wrapper prevents the caller
/// from overriding it.
#[derive(Clone)]
pub struct ScopedSession {
    /// 扩展点的追加面是对话记录（R05）：emit-stamped append 只需要
    /// ConversationLog；查询/计数属于 SessionStore，由装配层经
    /// `MiddlewareContext::store_session` 显式提供给需要的中间件。
    inner: Arc<dyn crate::conversation_log::ConversationLog>,
    emitter: EventEmitter,
}

impl ScopedSession {
    /// Create a new scoped wrapper. The emitter is baked in — it cannot be
    /// changed after construction. Accepts any conversation log (a full
    /// `Arc<dyn AgentSession>` coerces).
    pub fn new(
        inner: Arc<dyn crate::conversation_log::ConversationLog>,
        emitter: EventEmitter,
    ) -> Self {
        Self { inner, emitter }
    }

    /// The session id of the underlying session.
    pub fn session_id(&self) -> &str {
        self.inner.session_id()
    }

    /// The emitter that will be stamped on every event appended through this
    /// wrapper.
    pub fn emitter(&self) -> &EventEmitter {
        &self.emitter
    }

    /// Return a clone of the wrapped conversation-log handle. Callers that
    /// bypass emitter stamping (e.g. to forward events with their original
    /// emitter intact) use this escape hatch.
    pub fn inner(&self) -> Arc<dyn crate::conversation_log::ConversationLog> {
        self.inner.clone()
    }

    /// Append an event. The `emitter` field of the event is overwritten with
    /// this wrapper's emitter; any value set by the caller is discarded.
    pub async fn append(&self, mut event: SessionEvent) -> Result<(), SessionError> {
        event.emitter = self.emitter.clone();
        self.inner.append(event).await
    }
}

// ===========================================================================
// SessionEventListener (contract)
// ===========================================================================

/// Observer that receives a copy of every `SessionEvent` written to a
/// `ListenableInMemorySession`. Listeners are called in subscription order,
/// synchronously within each write call (after the write itself completes),
/// so they see the assigned `seq`.
///
/// Typical use: forward child-agent events into a parent session so that
/// the parent's event log contains a complete nested sub-run view.
#[async_trait]
pub trait SessionEventListener: Send + Sync {
    async fn on_event(&self, event: &SessionEvent);

    /// Is this listener still worth broadcasting to? A broadcasting session
    /// reaps listeners that report `false` after each notify, so a listener
    /// backed by a dropped consumer (e.g. a `subscribe_events` stream that
    /// was dropped) does not accumulate forever. Defaults to `true` — a
    /// listener with no liveness signal is treated as permanent (the right
    /// default for forwarders that live as long as their target).
    fn is_active(&self) -> bool {
        true
    }
}

/// R18: 窄能力桥——装配层实现本 trait 并注入扩展；内核工具执行路径只认识
/// 此抽象，不识别具体 `StoreSessionCell`、仓库或 spawn 存储语义。返回的
/// cell 供嵌套 spawn 原样转发，保证子链始终跟随当前会话。
#[async_trait::async_trait]
pub trait PersistentSessions: Send + Sync {
    /// 当前完整会话（SessionStore 能力）；最小装配不提供。
    fn store_session(&self) -> Option<Arc<dyn AgentSession>> {
        None
    }

    /// 共享 cell 本体——嵌套 spawn 原样传给 child extensions。
    fn store_session_cell(&self) -> Option<Arc<StoreSessionCell>> {
        None
    }
}
