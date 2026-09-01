// INPUT:  alva_kernel_abi event values, tokio mpsc
// OUTPUT: AgentEvent, AgentEventSender, AgentEventSendError, DEFAULT_AGENT_EVENT_CHANNEL_CAPACITY
// POS:    Defines agent observability events and the bounded/unbounded sender used by the run loop.
use serde::Serialize;
use tokio::sync::mpsc;

use alva_kernel_abi::AgentMessage;
use alva_kernel_abi::ProgressEvent;
use alva_kernel_abi::{StreamEvent, ToolCall, ToolOutput};

/// Default capacity for production-facing bounded event streams.
pub const DEFAULT_AGENT_EVENT_CHANNEL_CAPACITY: usize = 256;

/// Events emitted by the agent loop so callers can observe progress.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
pub enum AgentEvent {
    /// The overall agent execution has started.
    AgentStart,
    /// The overall agent execution has ended (with optional error).
    AgentEnd { error: Option<String> },

    /// A new turn (inner-loop iteration) has started.
    TurnStart,
    /// A turn has ended.
    TurnEnd,

    /// An assistant message has started streaming / been initiated.
    MessageStart { message: AgentMessage },
    /// A streaming delta for the current message.
    MessageUpdate {
        message: AgentMessage,
        delta: StreamEvent,
    },
    /// The assistant message failed before completion.
    MessageError {
        message: AgentMessage,
        error: String,
    },
    /// The assistant message is complete.
    MessageEnd { message: AgentMessage },

    /// A tool execution is about to begin.
    ToolExecutionStart { tool_call: ToolCall },
    /// An intermediate update from a running tool.
    ToolExecutionUpdate {
        tool_call_id: String,
        event: ProgressEvent,
    },
    /// A tool execution has finished.
    ToolExecutionEnd {
        tool_call: ToolCall,
        result: ToolOutput,
    },
}

/// Event transport accepted by the kernel loop.
///
/// The unbounded variant preserves the pre-existing SDK API and executor-free
/// wasm path. Native product harnesses should prefer `Bounded`: lifecycle and
/// message events then apply real producer backpressure instead of growing an
/// unbounded queue when a UI or adapter stops draining temporarily.
#[derive(Clone)]
pub enum AgentEventSender {
    Unbounded(mpsc::UnboundedSender<AgentEvent>),
    Bounded(mpsc::Sender<AgentEvent>),
}

/// Small transport error that does not carry a potentially large event.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum AgentEventSendError {
    #[error("agent event receiver is closed")]
    Closed,
    #[error("agent event channel is full")]
    Full,
}

impl AgentEventSender {
    /// Send a lossless event. A bounded transport waits for capacity.
    pub async fn send(&self, event: AgentEvent) -> Result<(), AgentEventSendError> {
        match self {
            Self::Unbounded(sender) => sender.send(event).map_err(|_| AgentEventSendError::Closed),
            Self::Bounded(sender) => sender
                .send(event)
                .await
                .map_err(|_| AgentEventSendError::Closed),
        }
    }

    /// Send a best-effort high-frequency event without blocking a synchronous
    /// callback. Used only for tool progress updates; lifecycle, message, and
    /// terminal tool events always go through [`Self::send`].
    pub fn try_send(&self, event: AgentEvent) -> Result<(), AgentEventSendError> {
        match self {
            Self::Unbounded(sender) => sender.send(event).map_err(|_| AgentEventSendError::Closed),
            Self::Bounded(sender) => sender.try_send(event).map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => AgentEventSendError::Full,
                mpsc::error::TrySendError::Closed(_) => AgentEventSendError::Closed,
            }),
        }
    }
}

impl From<mpsc::UnboundedSender<AgentEvent>> for AgentEventSender {
    fn from(sender: mpsc::UnboundedSender<AgentEvent>) -> Self {
        Self::Unbounded(sender)
    }
}

impl From<mpsc::Sender<AgentEvent>> for AgentEventSender {
    fn from(sender: mpsc::Sender<AgentEvent>) -> Self {
        Self::Bounded(sender)
    }
}

#[cfg(test)]
mod tests {
    //! Tests for AgentEvent serde wire shape.
    //!
    //! The enum uses `#[serde(tag = "type")]` (internal tagging) — the
    //! discriminant is `type` and struct fields are spread at top level
    //! alongside it (NOT nested under a per-variant key like external
    //! tagging would do). Tauri Inspector + CLI event handlers consume
    //! this shape directly; any drift breaks UI silently.
    //!
    //! AgentEvent only derives Serialize (no Deserialize); we test
    //! `to_value` shape only — full roundtrip is not part of the API.
    use super::*;
    use alva_kernel_abi::base::content::ContentBlock;
    use alva_kernel_abi::base::message::Message;
    use serde_json::{json, Value};

    fn ev_value(ev: AgentEvent) -> Value {
        serde_json::to_value(&ev).expect("AgentEvent must serialize")
    }

    // -- Unit variants: just `{"type": "Name"}` ---------------------------

    #[test]
    fn agent_start_unit_variant_renders_only_type_tag() {
        assert_eq!(
            ev_value(AgentEvent::AgentStart),
            json!({"type": "AgentStart"})
        );
    }

    #[test]
    fn turn_start_unit_variant_renders_only_type_tag() {
        assert_eq!(
            ev_value(AgentEvent::TurnStart),
            json!({"type": "TurnStart"})
        );
    }

    #[test]
    fn turn_end_unit_variant_renders_only_type_tag() {
        assert_eq!(ev_value(AgentEvent::TurnEnd), json!({"type": "TurnEnd"}));
    }

    // -- Struct variants: fields spread at top level alongside type ------

    #[test]
    fn agent_end_with_no_error_renders_error_as_null() {
        // Pin current behavior: no `skip_serializing_if` on `error`,
        // so None serializes as JSON null. Consumers may rely on the
        // key being present even on success.
        let v = ev_value(AgentEvent::AgentEnd { error: None });
        assert_eq!(v, json!({"type": "AgentEnd", "error": null}));
    }

    #[test]
    fn agent_end_with_some_error_renders_string() {
        let v = ev_value(AgentEvent::AgentEnd {
            error: Some("boom".into()),
        });
        assert_eq!(v, json!({"type": "AgentEnd", "error": "boom"}));
    }

    #[test]
    fn message_start_spreads_message_alongside_type_tag() {
        // Internal tag pin: fields appear at TOP LEVEL — NOT nested
        // under a per-variant key. UI consumers walk the JSON
        // expecting this shape; nesting would break every consumer.
        let m = Message::user("hi");
        let v = ev_value(AgentEvent::MessageStart {
            message: AgentMessage::Standard(m),
        });
        assert_eq!(v["type"], json!("MessageStart"));
        // `message` field exists at the top level (not nested).
        assert!(v.get("message").is_some(), "message must be top-level: {v}");
        // No nested "MessageStart" object — that's external tagging, not what we use.
        assert!(v.get("MessageStart").is_none());
    }

    #[test]
    fn message_end_includes_message_field() {
        let m = Message::user("done");
        let v = ev_value(AgentEvent::MessageEnd {
            message: AgentMessage::Standard(m),
        });
        assert_eq!(v["type"], json!("MessageEnd"));
        assert!(v.get("message").is_some());
    }

    #[test]
    fn message_update_includes_both_message_and_delta_top_level() {
        // Struct with two named fields: both must appear at top level
        // alongside "type", side by side (a refactor that grouped
        // them under a nested object would break consumers).
        let m = Message::user("hi");
        let v = ev_value(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(m),
            delta: StreamEvent::TextDelta { text: "h".into() },
        });
        assert_eq!(v["type"], json!("MessageUpdate"));
        assert!(v.get("message").is_some(), "message must be top-level");
        assert!(v.get("delta").is_some(), "delta must be top-level");
    }

    #[test]
    fn message_error_includes_message_and_error_string() {
        let m = Message::user("hi");
        let v = ev_value(AgentEvent::MessageError {
            message: AgentMessage::Standard(m),
            error: "stream cut".into(),
        });
        assert_eq!(v["type"], json!("MessageError"));
        assert_eq!(v["error"], json!("stream cut"));
        assert!(v.get("message").is_some());
    }

    #[test]
    fn tool_execution_start_includes_tool_call_field() {
        let tc = ToolCall {
            id: "tc1".into(),
            name: "read_file".into(),
            arguments: json!({}),
        };
        let v = ev_value(AgentEvent::ToolExecutionStart { tool_call: tc });
        assert_eq!(v["type"], json!("ToolExecutionStart"));
        assert!(v.get("tool_call").is_some());
        assert_eq!(v["tool_call"]["name"], json!("read_file"));
    }

    #[test]
    fn tool_execution_update_uses_snake_case_tool_call_id() {
        // Field name "tool_call_id" not "toolCallId" — pin since the
        // CLI / web consumers key off the snake_case form.
        let v = ev_value(AgentEvent::ToolExecutionUpdate {
            tool_call_id: "tc1".into(),
            event: ProgressEvent::Status {
                message: "running".into(),
            },
        });
        assert_eq!(v["type"], json!("ToolExecutionUpdate"));
        assert_eq!(v["tool_call_id"], json!("tc1"));
        // Ensure camelCase form would NOT match (regression guard
        // against a future #[serde(rename_all = "camelCase")] addition).
        assert!(v.get("toolCallId").is_none());
    }

    #[test]
    fn tool_execution_end_includes_tool_call_and_result_fields() {
        let tc = ToolCall {
            id: "tc1".into(),
            name: "bash".into(),
            arguments: json!({"cmd": "ls"}),
        };
        let v = ev_value(AgentEvent::ToolExecutionEnd {
            tool_call: tc,
            result: alva_kernel_abi::ToolOutput::text("ok"),
        });
        assert_eq!(v["type"], json!("ToolExecutionEnd"));
        assert!(v.get("tool_call").is_some());
        assert!(v.get("result").is_some());
        // result.is_error pinned via ToolOutput::text (false)
        assert_eq!(v["result"]["is_error"], json!(false));
        // Sanity that ContentBlock import compiles in this test mod.
        let _ = ContentBlock::Text {
            text: "smoke".into(),
        };
    }

    #[test]
    fn message_update_text_delta_wire_shape_is_externally_tagged() {
        let m = Message::user("hi");
        let v = ev_value(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(m),
            delta: StreamEvent::TextDelta {
                text: "hello".into(),
            },
        });
        // Pin the nested StreamEvent shape that Tauri web consumers match
        // (`delta.TextDelta.text` in Home.tsx).
        assert_eq!(
            v["delta"],
            json!({"TextDelta": {"text": "hello"}}),
            "delta wire shape: {v}"
        );
    }

    #[tokio::test]
    async fn bounded_sender_waits_for_receiver_capacity() {
        let (tx, mut rx) = mpsc::channel(1);
        let sender = AgentEventSender::from(tx);
        sender.send(AgentEvent::AgentStart).await.unwrap();

        assert!(
            tokio::time::timeout(
                std::time::Duration::from_millis(20),
                sender.send(AgentEvent::TurnStart),
            )
            .await
            .is_err(),
            "a full bounded channel must push back on lossless events"
        );

        assert!(matches!(rx.recv().await, Some(AgentEvent::AgentStart)));
        sender.send(AgentEvent::TurnStart).await.unwrap();
        assert!(matches!(rx.recv().await, Some(AgentEvent::TurnStart)));
    }

    #[tokio::test]
    async fn bounded_try_send_drops_only_when_full() {
        let (tx, mut rx) = mpsc::channel(1);
        let sender = AgentEventSender::from(tx);
        sender.try_send(AgentEvent::AgentStart).unwrap();
        assert_eq!(
            sender.try_send(AgentEvent::TurnStart),
            Err(AgentEventSendError::Full)
        );
        assert!(matches!(rx.recv().await, Some(AgentEvent::AgentStart)));
    }
}
