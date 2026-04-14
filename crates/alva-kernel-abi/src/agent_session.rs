// INPUT:  async_trait, serde, serde_json, uuid, chrono, tokio, std, crate::AgentMessage
// OUTPUT: AgentSession trait, InMemoryAgentSession, SessionEvent, SessionMessage,
//         EventEmitter, EmitterKind, ComponentDescriptor, ScopedSession,
//         SessionError, EventQuery, EventMatch
// POS:    Unified session abstraction — the single source of truth for everything
//         an agent does during a run. Replaces the legacy message-buffer-only
//         AgentSession that used to live in src/session.rs.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

use crate::base::message::AgentMessage;

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
        e.message = Some(SessionMessage { role: "user".into(), content });
        e
    }

    /// Construct an assistant-message event.
    pub fn assistant_message(content: serde_json::Value) -> Self {
        let mut e = Self::new("assistant");
        e.message = Some(SessionMessage { role: "assistant".into(), content });
        e
    }

    /// Construct a tool_result event linked to a parent tool_use uuid.
    pub fn tool_result(parent_tool_use_uuid: &str, content: serde_json::Value) -> Self {
        let mut e = Self::new("tool_result");
        e.parent_uuid = Some(parent_tool_use_uuid.to_string());
        e.message = Some(SessionMessage { role: "tool".into(), content });
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
