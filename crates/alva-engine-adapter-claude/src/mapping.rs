// INPUT:  std::collections::HashMap, alva_engine_runtime::{RuntimeEvent, RuntimeUsage}, alva_types::{ContentBlock, MessageRole, StreamEvent, ToolResult}, crate::protocol
// OUTPUT: pub(crate) struct EventMapper
// POS:    Stateful mapper that converts bridge protocol messages into unified RuntimeEvent sequences.

use std::collections::HashMap;

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_types::{ContentBlock, MessageRole, StreamEvent, ToolOutput};

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

            SdkMessage::Assistant {
                uuid, message, ..
            } => {
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
                            text_blocks.push(ContentBlock::Reasoning { text: thinking });
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
                    input_tokens: usage
                        .as_ref()
                        .and_then(|u| u.input_tokens)
                        .unwrap_or(0) as u32,
                    output_tokens: usage
                        .as_ref()
                        .and_then(|u| u.output_tokens)
                        .unwrap_or(0) as u32,
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
        assert!(
            matches!(&events[0], RuntimeEvent::Message { content, .. } if content.len() == 1)
        );
        assert!(
            matches!(&events[1], RuntimeEvent::ToolStart { name, .. } if name == "Read")
        );
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
        assert!(
            matches!(&events[0], RuntimeEvent::ToolEnd { name, .. } if name == "Bash")
        );
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
}
