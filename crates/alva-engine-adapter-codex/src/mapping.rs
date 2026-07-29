// INPUT:  CodexEvent/CodexItem, alva_engine_runtime events, alva_kernel_abi types
// OUTPUT: EventMapper
// POS:    Maps Codex JSONL events into neutral RuntimeEvent stream items.

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_kernel_abi::{ContentBlock, MessageRole, StreamEvent, ToolOutput};
use uuid::Uuid;

use crate::protocol::{CodexEvent, CodexItem, CodexUsage};

/// Stateful mapper from Codex JSONL events to RuntimeEvents.
#[derive(Debug, Default)]
pub struct EventMapper {
    session_id: Option<String>,
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

    pub fn map(&mut self, event: CodexEvent) -> Vec<RuntimeEvent> {
        match event {
            CodexEvent::ThreadStarted { thread_id } => {
                let session_id = thread_id.unwrap_or_else(|| Uuid::new_v4().to_string());
                self.session_id = Some(session_id.clone());
                vec![RuntimeEvent::SessionStarted {
                    session_id,
                    model: None,
                    tools: Vec::new(),
                }]
            }
            CodexEvent::TurnStarted => Vec::new(),
            CodexEvent::TurnCompleted { usage } => {
                if let Some(usage) = usage {
                    self.apply_usage(usage);
                }
                Vec::new()
            }
            CodexEvent::TurnFailed { error } => {
                self.failed = true;
                let message = error
                    .and_then(|e| e.message)
                    .unwrap_or_else(|| "Codex turn failed".into());
                vec![RuntimeEvent::Error {
                    message,
                    recoverable: false,
                }]
            }
            CodexEvent::ItemStarted { item } => self.map_item_progress(item, ItemPhase::Started),
            CodexEvent::ItemUpdated { item } => self.map_item_progress(item, ItemPhase::Updated),
            CodexEvent::ItemCompleted { item } => self.map_item_progress(item, ItemPhase::Completed),
            CodexEvent::Error { message, error } => {
                self.failed = true;
                let message = message
                    .or_else(|| error.and_then(|e| e.message))
                    .unwrap_or_else(|| "Codex error".into());
                vec![RuntimeEvent::Error {
                    message,
                    recoverable: false,
                }]
            }
            CodexEvent::Unknown => Vec::new(),
        }
    }

    fn apply_usage(&mut self, usage: CodexUsage) {
        if let Some(v) = usage.input_tokens {
            self.usage.input_tokens = v;
        }
        if let Some(v) = usage.output_tokens {
            self.usage.output_tokens = v;
        }
        self.usage.num_turns = self.usage.num_turns.saturating_add(1);
    }

    fn map_item_progress(&mut self, item: CodexItem, phase: ItemPhase) -> Vec<RuntimeEvent> {
        match item {
            CodexItem::AgentMessage { id, text } if phase == ItemPhase::Completed => {
                let text = text.unwrap_or_default();
                if text.is_empty() {
                    return Vec::new();
                }
                self.last_agent_text = Some(text.clone());
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                vec![RuntimeEvent::Message {
                    id,
                    role: MessageRole::Assistant,
                    content: vec![ContentBlock::Text { text }],
                }]
            }
            CodexItem::Reasoning { id, text } if phase == ItemPhase::Completed => {
                let text = text.unwrap_or_default();
                if text.is_empty() {
                    return Vec::new();
                }
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                vec![RuntimeEvent::MessageDelta {
                    id,
                    delta: StreamEvent::ReasoningDelta { text },
                }]
            }
            CodexItem::CommandExecution {
                id,
                command,
                aggregated_output,
                exit_code,
                ..
            } => {
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                let name = "shell".to_string();
                let input = serde_json::json!({ "command": command });
                match phase {
                    ItemPhase::Started | ItemPhase::Updated => {
                        vec![RuntimeEvent::ToolStart { id, name, input }]
                    }
                    ItemPhase::Completed => {
                        let body = aggregated_output.unwrap_or_default();
                        let is_error = exit_code.map(|c| c != 0).unwrap_or(false);
                        let result = if is_error {
                            ToolOutput::error(body)
                        } else {
                            ToolOutput::text(body)
                        };
                        vec![RuntimeEvent::ToolEnd {
                            id,
                            name,
                            result,
                            duration_ms: None,
                        }]
                    }
                }
            }
            CodexItem::McpToolCall {
                id,
                name,
                arguments,
                result,
                ..
            } => {
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                let name = name.unwrap_or_else(|| "mcp_tool".into());
                let input = arguments.unwrap_or_else(|| serde_json::json!({}));
                match phase {
                    ItemPhase::Started | ItemPhase::Updated => {
                        vec![RuntimeEvent::ToolStart { id, name, input }]
                    }
                    ItemPhase::Completed => {
                        let body = result.map(|v| v.to_string()).unwrap_or_default();
                        vec![RuntimeEvent::ToolEnd {
                            id,
                            name,
                            result: ToolOutput::text(body),
                            duration_ms: None,
                        }]
                    }
                }
            }
            CodexItem::FileChange { id, changes, .. } if phase == ItemPhase::Completed => {
                let id = id.unwrap_or_else(|| Uuid::new_v4().to_string());
                let input = changes.unwrap_or_else(|| serde_json::json!({}));
                vec![
                    RuntimeEvent::ToolStart {
                        id: id.clone(),
                        name: "file_change".into(),
                        input: input.clone(),
                    },
                    RuntimeEvent::ToolEnd {
                        id,
                        name: "file_change".into(),
                        result: ToolOutput::text(input.to_string()),
                        duration_ms: None,
                    },
                ]
            }
            _ => Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ItemPhase {
    Started,
    Updated,
    Completed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_agent_message_and_thread_start() {
        let mut mapper = EventMapper::new();
        let started = mapper.map(CodexEvent::ThreadStarted {
            thread_id: Some("thr-1".into()),
        });
        assert!(matches!(
            &started[0],
            RuntimeEvent::SessionStarted { session_id, .. } if session_id == "thr-1"
        ));

        let msg = mapper.map(CodexEvent::ItemCompleted {
            item: CodexItem::AgentMessage {
                id: Some("item_1".into()),
                text: Some("hello from codex".into()),
            },
        });
        match &msg[0] {
            RuntimeEvent::Message { content, .. } => {
                assert_eq!(content[0].as_text(), Some("hello from codex"));
            }
            other => panic!("expected Message, got {other:?}"),
        }
        assert_eq!(mapper.last_agent_text(), Some("hello from codex"));
    }
}
