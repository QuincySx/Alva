// INPUT:  std::collections::HashMap, alva_engine_runtime::{RuntimeEvent, RuntimeUsage}, alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolResult}, crate::protocol
// OUTPUT: pub(crate) struct EventMapper
// POS:    Stateful mapper that converts bridge protocol messages into unified RuntimeEvent sequences.

use std::collections::HashMap;

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolOutput};

use crate::protocol::{BridgeMessage, SdkContentBlock, SdkMessage};

/// Stateful mapper that converts bridge messages to runtime events.
///
/// Maintains a tool_use_id -> tool_name lookup to populate ToolEnd.name,
/// since SDK tool_result blocks only carry tool_use_id.
pub(crate) struct EventMapper {
    session_id: String,
    tool_names: HashMap<String, String>,
}

impl EventMapper {
    pub fn new() -> Self {
        Self {
            session_id: String::new(),
            tool_names: HashMap::new(),
        }
    }

    /// Accessor for the current session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Map a bridge message to zero or more runtime events.
    pub fn map(&mut self, msg: BridgeMessage) -> Vec<RuntimeEvent> {
        match msg {
            BridgeMessage::SdkMessage { message } => self.map_sdk_message(message),
            BridgeMessage::PermissionRequest {
                request_id,
                tool_name,
                tool_input,
            } => {
                vec![RuntimeEvent::PermissionRequest {
                    request_id,
                    tool_name,
                    tool_input,
                    description: None,
                }]
            }
            BridgeMessage::Done => vec![],
            BridgeMessage::Error { message } => vec![
                RuntimeEvent::Error {
                    message: message.clone(),
                    recoverable: false,
                },
                RuntimeEvent::Completed {
                    session_id: self.session_id.clone(),
                    result: None,
                    usage: None,
                },
            ],
        }
    }

    fn map_sdk_message(&mut self, msg: SdkMessage) -> Vec<RuntimeEvent> {
        match msg {
            SdkMessage::System {
                subtype,
                session_id,
                model,
                tools,
            } => {
                if subtype.as_deref() == Some("init") {
                    if let Some(sid) = &session_id {
                        self.session_id = sid.clone();
                    }
                    vec![RuntimeEvent::SessionStarted {
                        session_id: session_id.unwrap_or_default(),
                        model,
                        tools: tools.unwrap_or_default(),
                    }]
                } else {
                    vec![]
                }
            }

            SdkMessage::Assistant { uuid, message, .. } => {
                let Some(payload) = message else {
                    return vec![];
                };
                let Some(blocks) = payload.content else {
                    return vec![];
                };
                let msg_id = uuid.unwrap_or_default();
                let mut events = Vec::new();

                // Split: text/reasoning -> Message, tool_use -> ToolStart, tool_result -> ToolEnd
                let mut text_blocks = Vec::new();
                for block in blocks {
                    match block {
                        SdkContentBlock::Text { text } => {
                            text_blocks.push(ContentBlock::Text { text });
                        }
                        SdkContentBlock::Thinking { thinking } => {
                            text_blocks.push(ContentBlock::Reasoning {
                                text: thinking,
                                signature: None,
                            });
                        }
                        SdkContentBlock::ToolUse { id, name, input } => {
                            self.tool_names.insert(id.clone(), name.clone());
                            events.push(RuntimeEvent::ToolStart { id, name, input });
                        }
                        SdkContentBlock::ToolResult {
                            tool_use_id,
                            content,
                            is_error,
                        } => {
                            let name = self
                                .tool_names
                                .get(&tool_use_id)
                                .cloned()
                                .unwrap_or_default();
                            let is_err = is_error.unwrap_or(false);
                            let text = content.unwrap_or_default();
                            events.push(RuntimeEvent::ToolEnd {
                                id: tool_use_id,
                                name,
                                result: if is_err {
                                    ToolOutput::error(text)
                                } else {
                                    ToolOutput::text(text)
                                },
                                duration_ms: None,
                            });
                        }
                        SdkContentBlock::Other => {}
                    }
                }

                if !text_blocks.is_empty() {
                    events.insert(
                        0,
                        RuntimeEvent::Message {
                            id: msg_id,
                            role: MessageRole::Assistant,
                            content: text_blocks,
                        },
                    );
                }

                events
            }

            SdkMessage::StreamEvent { uuid, event } => {
                let msg_id = uuid.unwrap_or_default();
                let Some(event_val) = event else {
                    return vec![];
                };

                // Parse the stream event delta
                if let Some(delta) = parse_stream_delta(&event_val) {
                    vec![RuntimeEvent::MessageDelta { id: msg_id, delta }]
                } else {
                    vec![]
                }
            }

            SdkMessage::Result {
                subtype,
                session_id,
                result,
                total_cost_usd,
                duration_ms,
                num_turns,
                usage,
            } => {
                let sid = session_id.unwrap_or_else(|| self.session_id.clone());
                let is_error = subtype.as_deref() != Some("success");

                let usage_data = RuntimeUsage {
                    input_tokens: usage.as_ref().and_then(|u| u.input_tokens).unwrap_or(0) as u32,
                    output_tokens: usage.as_ref().and_then(|u| u.output_tokens).unwrap_or(0) as u32,
                    total_cost_usd,
                    duration_ms: duration_ms.unwrap_or(0),
                    num_turns: num_turns.unwrap_or(0),
                };

                let mut events = Vec::new();
                if is_error {
                    events.push(RuntimeEvent::Error {
                        message: format!(
                            "Session ended with: {}",
                            subtype.as_deref().unwrap_or("unknown")
                        ),
                        recoverable: false,
                    });
                }
                events.push(RuntimeEvent::Completed {
                    session_id: sid,
                    result: if is_error { None } else { result },
                    usage: Some(usage_data),
                });
                events
            }

            SdkMessage::Unknown => vec![],
        }
    }
}

/// Parse a raw stream event JSON into a StreamEvent.
fn parse_stream_delta(event: &serde_json::Value) -> Option<StreamEvent> {
    let event_type = event.get("type")?.as_str()?;
    match event_type {
        "content_block_delta" => {
            let delta = event.get("delta")?;
            let delta_type = delta.get("type")?.as_str()?;
            match delta_type {
                "text_delta" => {
                    let text = delta.get("text")?.as_str()?.to_string();
                    Some(StreamEvent::TextDelta { text })
                }
                "thinking_delta" => {
                    let text = delta.get("thinking")?.as_str()?.to_string();
                    Some(StreamEvent::ReasoningDelta { text })
                }
                "input_json_delta" => {
                    let partial = delta.get("partial_json")?.as_str()?.to_string();
                    // index -> tool call id (from content_block_start)
                    let id = event
                        .get("index")
                        .and_then(|v| v.as_u64())
                        .unwrap_or(0)
                        .to_string();
                    Some(StreamEvent::ToolCallDelta {
                        id,
                        name: None,
                        arguments_delta: partial,
                    })
                }
                _ => None,
            }
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::*;

    #[test]
    fn test_map_system_init() {
        let mut mapper = EventMapper::new();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::System {
                subtype: Some("init".into()),
                session_id: Some("s1".into()),
                model: Some("claude-sonnet-4-6".into()),
                tools: Some(vec!["Read".into(), "Write".into()]),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], RuntimeEvent::SessionStarted { session_id, .. } if session_id == "s1")
        );
    }

    #[test]
    fn test_map_assistant_splits_tool_use() {
        let mut mapper = EventMapper::new();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Assistant {
                uuid: Some("u1".into()),
                session_id: Some("s1".into()),
                message: Some(SdkAssistantPayload {
                    content: Some(vec![
                        SdkContentBlock::Text {
                            text: "Let me read that.".into(),
                        },
                        SdkContentBlock::ToolUse {
                            id: "tc1".into(),
                            name: "Read".into(),
                            input: serde_json::json!({"file_path": "/tmp/a.rs"}),
                        },
                    ]),
                }),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Message { content, .. } if content.len() == 1));
        assert!(matches!(&events[1], RuntimeEvent::ToolStart { name, .. } if name == "Read"));
    }

    #[test]
    fn test_map_tool_result_resolves_name() {
        let mut mapper = EventMapper::new();
        // First: register tool name via ToolUse
        mapper.tool_names.insert("tc1".into(), "Bash".into());
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Assistant {
                uuid: Some("u2".into()),
                session_id: Some("s1".into()),
                message: Some(SdkAssistantPayload {
                    content: Some(vec![SdkContentBlock::ToolResult {
                        tool_use_id: "tc1".into(),
                        content: Some("output".into()),
                        is_error: Some(false),
                    }]),
                }),
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::ToolEnd { name, .. } if name == "Bash"));
    }

    #[test]
    fn test_map_result_error_emits_error_then_completed() {
        let mut mapper = EventMapper::new();
        mapper.session_id = "s1".into();
        let msg = BridgeMessage::SdkMessage {
            message: SdkMessage::Result {
                subtype: Some("error_max_turns".into()),
                session_id: Some("s1".into()),
                result: None,
                total_cost_usd: Some(0.1),
                duration_ms: Some(5000),
                num_turns: Some(10),
                usage: None,
            },
        };
        let events = mapper.map(msg);
        assert_eq!(events.len(), 2);
        assert!(matches!(
            &events[0],
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(matches!(
            &events[1],
            RuntimeEvent::Completed { result: None, .. }
        ));
    }

    #[test]
    fn test_map_bridge_error() {
        let mut mapper = EventMapper::new();
        let events = mapper.map(BridgeMessage::Error {
            message: "crash".into(),
        });
        assert_eq!(events.len(), 2);
        assert!(matches!(&events[0], RuntimeEvent::Error { .. }));
        assert!(matches!(&events[1], RuntimeEvent::Completed { .. }));
    }

    // -- Loop 150 gap-fill: parse_stream_delta SSE wire + edge SdkMessage
    //    branches that were 0-test until now --------------------------

    // -- parse_stream_delta: 5 wire-shape pins --------------------------

    #[test]
    fn parse_stream_delta_text_delta_yields_text_delta_event() {
        // CRITICAL: Anthropic SSE `content_block_delta` with
        // `delta.type == "text_delta"` → StreamEvent::TextDelta.
        // A silent rename of either string would drop EVERY streamed
        // text chunk from the SSE pipeline; the UI would stay blank
        // through the entire response until the final Completed event.
        let event = serde_json::json!({
            "type": "content_block_delta",
            "delta": {"type": "text_delta", "text": "hello world"}
        });
        match parse_stream_delta(&event) {
            Some(StreamEvent::TextDelta { text }) => assert_eq!(text, "hello world"),
            other => panic!("expected TextDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_delta_thinking_delta_yields_reasoning_delta() {
        // Extended-thinking SSE: `thinking_delta` carries a `thinking`
        // field (NOT `text`). A silent change to read `text` here would
        // make every extended-thinking stream surface as empty
        // ReasoningDelta — Claude's reasoning UI panel would stay blank.
        let event = serde_json::json!({
            "type": "content_block_delta",
            "delta": {"type": "thinking_delta", "thinking": "let me think"}
        });
        match parse_stream_delta(&event) {
            Some(StreamEvent::ReasoningDelta { text }) => assert_eq!(text, "let me think"),
            other => panic!("expected ReasoningDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_delta_input_json_delta_uses_event_index_as_tool_id() {
        // SILENT CONTRACT: tool-args streaming arrives as
        // `input_json_delta` chunks; the chunk doesn't carry the tool
        // ID directly. Mapping uses the outer `event.index` field
        // (set by `content_block_start` upstream) stringified as the
        // RuntimeEvent::ToolCallDelta.id. A refactor that read
        // delta.id or skipped the index lookup would route every
        // streamed arg fragment to id="" and the UI would merge them
        // all into one bogus tool call.
        let event = serde_json::json!({
            "type": "content_block_delta",
            "index": 3,
            "delta": {"type": "input_json_delta", "partial_json": "{\"k\":"}
        });
        match parse_stream_delta(&event) {
            Some(StreamEvent::ToolCallDelta {
                id,
                name,
                arguments_delta,
            }) => {
                assert_eq!(id, "3", "index 3 → id \"3\"");
                assert!(name.is_none(), "name is unknown at delta time");
                assert_eq!(arguments_delta, "{\"k\":");
            }
            other => panic!("expected ToolCallDelta, got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_delta_input_json_delta_defaults_index_to_zero_when_missing() {
        // Pin: when the event omits `index` entirely (defensive
        // upstream), the code path defaults to "0" via
        // `.unwrap_or(0).to_string()`. A change here could panic or
        // produce silent "".
        let event = serde_json::json!({
            "type": "content_block_delta",
            "delta": {"type": "input_json_delta", "partial_json": "x"}
        });
        match parse_stream_delta(&event) {
            Some(StreamEvent::ToolCallDelta { id, .. }) => assert_eq!(id, "0"),
            other => panic!("expected ToolCallDelta with id=\"0\", got {other:?}"),
        }
    }

    #[test]
    fn parse_stream_delta_unknown_event_type_returns_none_for_forward_compat() {
        // Forward-compat pin: SSE specs evolve; unknown top-level
        // event types (e.g. "message_delta", "content_block_start",
        // future "audio_delta") MUST return None rather than panic.
        let event = serde_json::json!({
            "type": "some_future_event",
            "delta": {"type": "text_delta", "text": "x"}
        });
        assert!(parse_stream_delta(&event).is_none());
    }

    #[test]
    fn parse_stream_delta_unknown_delta_type_returns_none_for_forward_compat() {
        // Forward-compat pin: within content_block_delta, unknown
        // delta types (future "audio_chunk", "image_delta") MUST
        // return None.
        let event = serde_json::json!({
            "type": "content_block_delta",
            "delta": {"type": "future_delta_type", "text": "x"}
        });
        assert!(parse_stream_delta(&event).is_none());
    }

    // -- Edge SdkMessage / BridgeMessage branches -----------------------

    #[test]
    fn map_bridge_done_returns_empty_vec_no_runtime_events_emitted() {
        // SILENT CONTRACT: BridgeMessage::Done is a transport-layer
        // signal (SDK finished writing) that MUST NOT produce a
        // RuntimeEvent. The Completed event already fires from the
        // preceding Result message. A refactor that emitted a
        // duplicate Completed here would double-fire and break
        // downstream consumers that count completion exactly once.
        let mut mapper = EventMapper::new();
        let events = mapper.map(BridgeMessage::Done);
        assert!(
            events.is_empty(),
            "Done must emit zero events, got {events:?}"
        );
    }

    #[test]
    fn map_bridge_permission_request_emits_permission_event_with_description_none() {
        // Pin: PermissionRequest payload propagates request_id /
        // tool_name / tool_input verbatim; description defaults to None.
        // A refactor that auto-populated description (e.g. from
        // tool_name) would silently change UI prompt text.
        let mut mapper = EventMapper::new();
        let events = mapper.map(BridgeMessage::PermissionRequest {
            request_id: "req-1".into(),
            tool_name: "Bash".into(),
            tool_input: serde_json::json!({"cmd": "ls"}),
        });
        assert_eq!(events.len(), 1);
        match &events[0] {
            RuntimeEvent::PermissionRequest {
                request_id,
                tool_name,
                tool_input,
                description,
            } => {
                assert_eq!(request_id, "req-1");
                assert_eq!(tool_name, "Bash");
                assert_eq!(*tool_input, serde_json::json!({"cmd": "ls"}));
                assert!(description.is_none(), "description must default to None");
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }

    #[test]
    fn map_sdk_message_unknown_silently_returns_empty_vec_for_forward_compat() {
        // CRITICAL forward-compat: when the Claude SDK introduces a
        // new SdkMessage variant the host hasn't learned about yet,
        // the catch-all `SdkMessage::Unknown` returns vec![]. A
        // refactor to panic / Err would make a single new SDK message
        // type crash every host that hadn't been upgraded.
        let mut mapper = EventMapper::new();
        let events = mapper.map(BridgeMessage::SdkMessage {
            message: SdkMessage::Unknown,
        });
        assert!(
            events.is_empty(),
            "Unknown SdkMessage must emit zero events for forward-compat"
        );
    }
}
