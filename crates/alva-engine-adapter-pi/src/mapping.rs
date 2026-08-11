// INPUT:  PiEvent wire values and engine runtime/kernel event types
// OUTPUT: EventMapper
// POS:    Stateful Pi JSONL to RuntimeEvent mapper.

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolOutput};
use uuid::Uuid;

use crate::protocol::{PiAssistantMessageEvent, PiContent, PiEvent, PiMessage};

#[derive(Debug, Default)]
pub struct EventMapper {
    session_id: Option<String>,
    current_message_id: Option<String>,
    last_agent_text: Option<String>,
    usage: RuntimeUsage,
    failed: bool,
}

impl EventMapper {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn session_id(&self) -> Option<&str> {
        self.session_id.as_deref()
    }

    pub fn last_agent_text(&self) -> Option<&str> {
        self.last_agent_text.as_deref()
    }

    pub fn usage(&self) -> RuntimeUsage {
        self.usage.clone()
    }

    pub fn failed(&self) -> bool {
        self.failed
    }

    pub fn map(&mut self, event: PiEvent) -> Vec<RuntimeEvent> {
        match event {
            PiEvent::Session { id, .. } => {
                self.session_id = Some(id.clone());
                vec![RuntimeEvent::SessionStarted {
                    session_id: id,
                    model: None,
                    tools: Vec::new(),
                }]
            }
            PiEvent::MessageStart { message } if message.role == "assistant" => {
                self.current_message_id = Some(Uuid::new_v4().to_string());
                Vec::new()
            }
            PiEvent::MessageUpdate {
                assistant_message_event,
            } => self.map_delta(assistant_message_event),
            PiEvent::MessageEnd { message } if message.role == "assistant" => {
                self.map_message_end(message)
            }
            PiEvent::ToolExecutionStart {
                tool_call_id,
                tool_name,
                args,
            } => vec![RuntimeEvent::ToolStart {
                id: tool_call_id,
                name: tool_name,
                input: args,
            }],
            PiEvent::ToolExecutionEnd {
                tool_call_id,
                tool_name,
                result,
                is_error,
            } => {
                let text = tool_result_text(&result);
                vec![RuntimeEvent::ToolEnd {
                    id: tool_call_id,
                    name: tool_name,
                    result: if is_error {
                        ToolOutput::error(text)
                    } else {
                        ToolOutput::text(text)
                    },
                    duration_ms: None,
                }]
            }
            PiEvent::AutoRetryStart {
                error_message,
                attempt,
                max_attempts,
            } => vec![RuntimeEvent::Error {
                message: format!(
                    "Pi retry {attempt}/{max_attempts}: {}",
                    error_message.trim()
                ),
                recoverable: true,
            }],
            PiEvent::AutoRetryEnd {
                success: false,
                final_error,
            } => {
                self.failed = true;
                vec![RuntimeEvent::Error {
                    message: final_error.unwrap_or_else(|| "Pi retry failed".into()),
                    recoverable: false,
                }]
            }
            PiEvent::AgentEnd { will_retry: true } => Vec::new(),
            _ => Vec::new(),
        }
    }

    fn map_delta(&mut self, event: PiAssistantMessageEvent) -> Vec<RuntimeEvent> {
        let id = self
            .current_message_id
            .get_or_insert_with(|| Uuid::new_v4().to_string())
            .clone();
        match event {
            PiAssistantMessageEvent::TextDelta { delta } if !delta.is_empty() => {
                vec![RuntimeEvent::MessageDelta {
                    id,
                    delta: StreamEvent::TextDelta { text: delta },
                }]
            }
            PiAssistantMessageEvent::ThinkingDelta { delta } if !delta.is_empty() => {
                vec![RuntimeEvent::MessageDelta {
                    id,
                    delta: StreamEvent::ReasoningDelta { text: delta },
                }]
            }
            PiAssistantMessageEvent::Error { error } => {
                self.failed = true;
                vec![RuntimeEvent::Error {
                    message: error
                        .error_message
                        .unwrap_or_else(|| "Pi assistant request failed".into()),
                    recoverable: false,
                }]
            }
            _ => Vec::new(),
        }
    }

    fn map_message_end(&mut self, message: PiMessage) -> Vec<RuntimeEvent> {
        if let Some(usage) = message.usage {
            self.usage.input_tokens = self.usage.input_tokens.saturating_add(usage.input);
            self.usage.output_tokens = self.usage.output_tokens.saturating_add(usage.output);
            if let Some(cost) = usage.cost {
                self.usage.total_cost_usd =
                    Some(self.usage.total_cost_usd.unwrap_or_default() + cost.total);
            }
            self.usage.num_turns = self.usage.num_turns.saturating_add(1);
        }
        if matches!(message.stop_reason.as_deref(), Some("error" | "aborted")) {
            self.failed = true;
            return vec![RuntimeEvent::Error {
                message: message
                    .error_message
                    .unwrap_or_else(|| "Pi turn failed".into()),
                recoverable: false,
            }];
        }

        let text = message
            .content
            .iter()
            .filter_map(|content| match content {
                PiContent::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("");
        if text.is_empty() {
            return Vec::new();
        }
        self.last_agent_text = Some(text.clone());
        vec![RuntimeEvent::Message {
            id: self
                .current_message_id
                .take()
                .unwrap_or_else(|| Uuid::new_v4().to_string()),
            role: MessageRole::Assistant,
            content: vec![ContentBlock::Text { text }],
        }]
    }
}

fn tool_result_text(value: &serde_json::Value) -> String {
    if let Some(content) = value.get("content").and_then(|value| value.as_array()) {
        let text = content
            .iter()
            .filter_map(|item| item.get("text").and_then(|value| value.as_str()))
            .collect::<Vec<_>>()
            .join("\n");
        if !text.is_empty() {
            return text;
        }
    }
    value
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| value.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{PiAssistantMessageEvent, PiContent, PiEvent, PiMessage, PiUsage};

    #[test]
    fn maps_session_delta_message_and_usage() {
        let mut mapper = EventMapper::new();
        let started = mapper.map(PiEvent::Session {
            id: "pi-1".into(),
            cwd: None,
        });
        assert!(matches!(
            &started[0],
            RuntimeEvent::SessionStarted { session_id, .. } if session_id == "pi-1"
        ));
        mapper.map(PiEvent::MessageStart {
            message: PiMessage {
                role: "assistant".into(),
                ..PiMessage::default()
            },
        });
        let delta = mapper.map(PiEvent::MessageUpdate {
            assistant_message_event: PiAssistantMessageEvent::TextDelta {
                delta: "hel".into(),
            },
        });
        assert!(matches!(delta[0], RuntimeEvent::MessageDelta { .. }));
        let message = mapper.map(PiEvent::MessageEnd {
            message: PiMessage {
                role: "assistant".into(),
                content: vec![PiContent::Text {
                    text: "hello".into(),
                }],
                usage: Some(PiUsage {
                    input: 10,
                    output: 2,
                    total_tokens: 12,
                    cost: None,
                }),
                ..PiMessage::default()
            },
        });
        assert!(matches!(message[0], RuntimeEvent::Message { .. }));
        assert_eq!(mapper.last_agent_text(), Some("hello"));
        assert_eq!(mapper.usage().input_tokens, 10);
    }

    #[test]
    fn retry_start_stays_recoverable() {
        let mut mapper = EventMapper::new();
        let events = mapper.map(PiEvent::AutoRetryStart {
            error_message: "network down".into(),
            attempt: 1,
            max_attempts: 3,
        });
        assert!(matches!(
            events[0],
            RuntimeEvent::Error {
                recoverable: true,
                ..
            }
        ));
    }
}
