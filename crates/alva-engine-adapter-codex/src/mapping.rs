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
                // A missing thread_id means there is no real resumable native
                // session. Fabricating a random uuid here would be echoed back
                // to `codex exec resume <uuid>` on the next turn and rejected
                // (ticket 11): the turn is session-less instead.
                if let Some(thread_id) = thread_id {
                    self.session_id = Some(thread_id.clone());
                    vec![RuntimeEvent::SessionStarted {
                        session_id: thread_id,
                        model: None,
                        tools: Vec::new(),
                    }]
                } else {
                    self.session_id = None;
                    Vec::new()
                }
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
                // TurnFailed is always terminal for `codex exec --json` (no
                // daemon willRetry loop on this path). Pair with Completed
                // in the adapter after the stream ends.
                vec![RuntimeEvent::Error {
                    message,
                    recoverable: false,
                }]
            }
            CodexEvent::ItemStarted { item } => self.map_item_progress(item, ItemPhase::Started),
            CodexEvent::ItemUpdated { item } => self.map_item_progress(item, ItemPhase::Updated),
            CodexEvent::ItemCompleted { item } => {
                self.map_item_progress(item, ItemPhase::Completed)
            }
            CodexEvent::Error { message, error } => {
                let message = message
                    .or_else(|| error.and_then(|e| e.message))
                    .unwrap_or_else(|| "Codex error".into());
                // cindy network-error / willRetry: reconnect progress is
                // non-terminal so UI must not free running. `codex exec`
                // rarely emits these, but classify if we see the fingerprint.
                let recoverable = is_recoverable_codex_error(&message);
                if !recoverable {
                    self.failed = true;
                }
                vec![RuntimeEvent::Error {
                    message,
                    recoverable,
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
                    // Only open once — item.updated must not re-emit ToolStart
                    // (stream contract: no duplicate open id).
                    ItemPhase::Started => {
                        vec![RuntimeEvent::ToolStart { id, name, input }]
                    }
                    ItemPhase::Updated => Vec::new(),
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
                    ItemPhase::Started => {
                        vec![RuntimeEvent::ToolStart { id, name, input }]
                    }
                    ItemPhase::Updated => Vec::new(),
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

/// cindy `isNetworkishErrorMessage` / reconnect progress — non-terminal.
///
/// Patterns intentionally conservative; false positives freeze UI less badly
/// than false negatives that free running mid-retry.
fn is_recoverable_codex_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("reconnecting")
        || lower.contains("econnrefused")
        || lower.contains("econnreset")
        || lower.contains("etimedout")
        || lower.contains("enotfound")
        || lower.contains("socket hang up")
        || lower.contains("network error")
        || lower.contains("fetch failed")
        || lower.contains("request timed out")
        || lower.contains("connection error")
        || lower.contains("bad gateway")
        || lower.contains("service unavailable")
        || lower.contains("gateway timeout")
        || lower.contains("upstream unreachable")
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

    /// ticket 11: a missing thread_id must not fabricate a random native
    /// session id that a later turn would hand to `codex exec resume <uuid>`.
    #[test]
    fn thread_started_without_id_does_not_fabricate_session_id() {
        let mut mapper = EventMapper::new();
        let events = mapper.map(CodexEvent::ThreadStarted { thread_id: None });
        assert!(
            events.is_empty(),
            "no SessionStarted may be fabricated: {events:?}"
        );
        assert_eq!(mapper.session_id(), None);
    }

    #[test]
    fn tool_item_updated_does_not_duplicate_tool_start() {
        use alva_engine_runtime::validate_event_stream;

        let mut mapper = EventMapper::new();
        let mut stream = Vec::new();
        stream.extend(mapper.map(CodexEvent::ThreadStarted {
            thread_id: Some("t".into()),
        }));
        stream.extend(mapper.map(CodexEvent::ItemStarted {
            item: CodexItem::CommandExecution {
                id: Some("c1".into()),
                command: Some("ls".into()),
                aggregated_output: None,
                exit_code: None,
                status: Some("in_progress".into()),
            },
        }));
        // Updated must not re-open the same tool id.
        stream.extend(mapper.map(CodexEvent::ItemUpdated {
            item: CodexItem::CommandExecution {
                id: Some("c1".into()),
                command: Some("ls".into()),
                aggregated_output: Some("partial".into()),
                exit_code: None,
                status: Some("in_progress".into()),
            },
        }));
        stream.extend(mapper.map(CodexEvent::ItemCompleted {
            item: CodexItem::CommandExecution {
                id: Some("c1".into()),
                command: Some("ls".into()),
                aggregated_output: Some("ok".into()),
                exit_code: Some(0),
                status: Some("completed".into()),
            },
        }));
        stream.extend(mapper.map(CodexEvent::ItemCompleted {
            item: CodexItem::AgentMessage {
                id: Some("a".into()),
                text: Some("done".into()),
            },
        }));
        // Synthesize terminal the way the adapter does after stdout EOF.
        stream.push(RuntimeEvent::Completed {
            session_id: "t".into(),
            result: mapper.last_agent_text().map(str::to_string),
            usage: Some(mapper.usage()),
        });
        assert!(
            validate_event_stream(&stream).is_ok(),
            "stream must satisfy multi-engine contract: {stream:?}"
        );
    }

    #[test]
    fn reconnect_style_error_is_recoverable_not_failed() {
        // cindy willRetry / network-error: reconnect progress must not free UI.
        use alva_engine_runtime::validate_event_stream;

        let mut mapper = EventMapper::new();
        let mut stream = Vec::new();
        stream.extend(mapper.map(CodexEvent::ThreadStarted {
            thread_id: Some("t".into()),
        }));
        let err = mapper.map(CodexEvent::Error {
            message: Some("Reconnecting... 2/5".into()),
            error: None,
        });
        assert!(matches!(
            &err[0],
            RuntimeEvent::Error {
                recoverable: true,
                ..
            }
        ));
        assert!(!mapper.failed());
        stream.extend(err);
        stream.extend(mapper.map(CodexEvent::ItemCompleted {
            item: CodexItem::AgentMessage {
                id: Some("a".into()),
                text: Some("recovered".into()),
            },
        }));
        stream.push(RuntimeEvent::Completed {
            session_id: "t".into(),
            result: mapper.last_agent_text().map(str::to_string),
            usage: None,
        });
        assert!(validate_event_stream(&stream).is_ok());
    }

    #[test]
    fn hard_error_is_fatal() {
        let mut mapper = EventMapper::new();
        let err = mapper.map(CodexEvent::Error {
            message: Some("authentication required".into()),
            error: None,
        });
        assert!(matches!(
            &err[0],
            RuntimeEvent::Error {
                recoverable: false,
                ..
            }
        ));
        assert!(mapper.failed());
    }
}
