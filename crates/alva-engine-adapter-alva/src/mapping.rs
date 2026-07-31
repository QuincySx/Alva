// INPUT:  alva_kernel_core::AgentEvent, alva_kernel_abi::{AgentMessage, ContentBlock, StreamEvent}, alva_engine_runtime::{RuntimeEvent, RuntimeUsage, is_consumer_visible_delta}
// OUTPUT: EventMapper
// POS:    Projects complete BaseAgent loop events onto the engine contract.

use std::collections::HashMap;

use alva_engine_runtime::{is_consumer_visible_delta, RuntimeEvent, RuntimeUsage};
use alva_kernel_abi::{AgentMessage, ContentBlock, MessageRole};
use alva_kernel_core::AgentEvent;

pub(crate) struct EventMapper {
    session_id: String,
    model: String,
    tools: Vec<String>,
    turn_count: u32,
    tool_names: HashMap<String, String>,
    /// Last non-empty assistant text for Completed.result (never a placeholder).
    last_assistant_text: Option<String>,
}

impl EventMapper {
    pub(crate) fn new(session_id: String, model: String, tools: Vec<String>) -> Self {
        Self {
            session_id,
            model,
            tools,
            turn_count: 0,
            tool_names: HashMap::new(),
            last_assistant_text: None,
        }
    }

    pub(crate) fn map(&mut self, event: AgentEvent) -> Vec<RuntimeEvent> {
        match event {
            AgentEvent::AgentStart => vec![RuntimeEvent::SessionStarted {
                session_id: self.session_id.clone(),
                model: Some(self.model.clone()),
                tools: self.tools.clone(),
            }],
            AgentEvent::TurnStart => {
                self.turn_count += 1;
                vec![]
            }
            AgentEvent::TurnEnd | AgentEvent::MessageStart { .. } => vec![],
            AgentEvent::MessageUpdate { message, delta } => {
                // Stream contract: only consumer-visible deltas leave the adapter.
                if !is_consumer_visible_delta(&delta) {
                    return vec![];
                }
                vec![RuntimeEvent::MessageDelta {
                    id: message_id(&message),
                    delta,
                }]
            }
            AgentEvent::MessageError { error, .. } => vec![RuntimeEvent::Error {
                message: error,
                recoverable: true,
            }],
            AgentEvent::MessageEnd { message } => self.map_message_end(message),
            // ToolStart is announced when the assistant message declares tool_use
            // (map_message_end). Execution lifecycle is ToolEnd only here so we
            // do not double-open the same id.
            AgentEvent::ToolExecutionStart { .. } | AgentEvent::ToolExecutionUpdate { .. } => {
                vec![]
            }
            AgentEvent::ToolExecutionEnd { tool_call, result } => vec![RuntimeEvent::ToolEnd {
                name: self
                    .tool_names
                    .get(&tool_call.id)
                    .cloned()
                    .unwrap_or_else(|| tool_call.name.clone()),
                id: tool_call.id,
                result,
                duration_ms: None,
            }],
            AgentEvent::AgentEnd { error: None } => vec![RuntimeEvent::Completed {
                session_id: self.session_id.clone(),
                result: self.last_assistant_text.clone(),
                usage: Some(RuntimeUsage {
                    num_turns: self.turn_count,
                    ..Default::default()
                }),
            }],
            AgentEvent::AgentEnd { error: Some(error) } => vec![
                RuntimeEvent::Error {
                    message: error,
                    recoverable: false,
                },
                RuntimeEvent::Completed {
                    session_id: self.session_id.clone(),
                    result: None,
                    usage: Some(RuntimeUsage {
                        num_turns: self.turn_count,
                        ..Default::default()
                    }),
                },
            ],
        }
    }

    fn map_message_end(&mut self, message: AgentMessage) -> Vec<RuntimeEvent> {
        let AgentMessage::Standard(message) = message else {
            return vec![];
        };
        let is_assistant = matches!(&message.role, MessageRole::Assistant);
        let mut assistant_text = String::new();
        let mut content = Vec::new();
        let mut events = Vec::new();
        for block in message.content {
            match block {
                ContentBlock::ToolUse { id, name, input } => {
                    self.tool_names.insert(id.clone(), name.clone());
                    // Announce tools here (message commit order), before execution ToolEnd.
                    events.push(RuntimeEvent::ToolStart { id, name, input });
                }
                ContentBlock::ToolResult { .. } => {
                    // Never surface tool results inside Message — ToolEnd owns them.
                }
                ContentBlock::Text { ref text } => {
                    if is_assistant && !text.is_empty() {
                        assistant_text.push_str(text);
                    }
                    content.push(block);
                }
                other => content.push(other),
            }
        }
        if !assistant_text.is_empty() {
            self.last_assistant_text = Some(assistant_text);
        }
        if !content.is_empty() {
            events.insert(
                0,
                RuntimeEvent::Message {
                    id: message.id,
                    role: message.role,
                    content,
                },
            );
        }
        events
    }
}

fn message_id(message: &AgentMessage) -> String {
    match message {
        AgentMessage::Standard(message)
        | AgentMessage::Steering(message)
        | AgentMessage::FollowUp(message) => message.id.clone(),
        AgentMessage::Marker(_) => "marker".into(),
        AgentMessage::Extension { type_name, .. } => type_name.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_engine_runtime::validate_event_stream;
    use alva_kernel_abi::{Message, MessageRole, StreamEvent, ToolCall, ToolOutput};
    use alva_kernel_core::AgentEvent;
    use serde_json::json;

    fn mapper() -> EventMapper {
        EventMapper::new("sess".into(), "model".into(), vec!["bash".into()])
    }

    #[test]
    fn filters_control_frame_deltas() {
        let mut m = mapper();
        let events = m.map(AgentEvent::MessageUpdate {
            message: AgentMessage::Standard(Message {
                id: "m1".into(),
                role: MessageRole::Assistant,
                content: vec![],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            }),
            delta: StreamEvent::Start,
        });
        assert!(events.is_empty());
    }

    #[test]
    fn completed_result_contains_all_blocks_from_last_assistant_message() {
        let mut m = mapper();
        let mut stream = Vec::new();
        stream.extend(m.map(AgentEvent::AgentStart));
        stream.extend(m.map(AgentEvent::MessageEnd {
            message: AgentMessage::Standard(Message {
                id: "m1".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "final ".into(),
                    },
                    ContentBlock::Reasoning {
                        text: "hidden".into(),
                        signature: None,
                    },
                    ContentBlock::Text {
                        text: "answer".into(),
                    },
                ],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            }),
        }));
        stream.extend(m.map(AgentEvent::AgentEnd { error: None }));
        assert!(validate_event_stream(&stream).is_ok());
        match stream.last() {
            Some(RuntimeEvent::Completed {
                result: Some(text), ..
            }) => assert_eq!(text, "final answer"),
            other => panic!("expected Completed with text, got {other:?}"),
        }
    }

    #[test]
    fn tool_use_is_tool_start_not_message_content() {
        let mut m = mapper();
        let events = m.map(AgentEvent::MessageEnd {
            message: AgentMessage::Standard(Message {
                id: "m1".into(),
                role: MessageRole::Assistant,
                content: vec![
                    ContentBlock::Text {
                        text: "calling".into(),
                    },
                    ContentBlock::ToolUse {
                        id: "t1".into(),
                        name: "bash".into(),
                        input: json!({"cmd": "ls"}),
                    },
                ],
                tool_call_id: None,
                usage: None,
                timestamp: 0,
            }),
        });
        assert!(matches!(
            &events[0],
            RuntimeEvent::Message { content, .. }
                if content.iter().all(|b| !matches!(b, ContentBlock::ToolUse { .. }))
        ));
        assert!(matches!(
            &events[1],
            RuntimeEvent::ToolStart { id, name, .. } if id == "t1" && name == "bash"
        ));
        let end = m.map(AgentEvent::ToolExecutionEnd {
            tool_call: ToolCall {
                id: "t1".into(),
                name: "bash".into(),
                arguments: json!({}),
            },
            result: ToolOutput::text("ok"),
        });
        assert!(matches!(&end[0], RuntimeEvent::ToolEnd { id, .. } if id == "t1"));
    }
}
