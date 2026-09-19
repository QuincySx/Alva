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
    /// R95: 暂时性 assistant 错误（未定性的失败候选）。Pi 0.85.1 的真实
    /// 时序是 message_update(error) → message_end(error) →
    /// agent_end(willRetry) → auto_retry_start → 重试 → auto_retry_end /
    /// agent_end(willRetry:false)：单个 assistant 错误只是"本轮尚未定
    /// 性"，CLI 仍可能合法重试。在可判定之前保持状态、不外发任何 fatal；
    /// 定性点在 agent_end(willRetry:false)（携带未恢复错误 → 唯一 fatal）
    /// 或 auto_retry_end(success:false)（重试耗尽，既有路径）。
    pending_error: Option<String>,
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

    /// R96: settle a still-pending error at the END of the whole request
    /// (process EOF). A pending error that survived to settlement — no
    /// compacted/retried answer ever arrived — becomes the single fatal, with
    /// the original message preserved. Returns the fatal to forward, if any.
    pub fn settle_at_eof(&mut self) -> Option<RuntimeEvent> {
        let message = self.pending_error.take()?;
        self.failed = true;
        Some(RuntimeEvent::Error {
            message,
            recoverable: false,
        })
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
                // R96: 耗尽即定性。待定错误在此被 auto_retry_end 的唯一
                // fatal 消费；若残留到 EOF，settle_at_eof 会再升一个 fatal，
                // 违反 R2（fatal 之后只允许 Completed）。
                self.pending_error = None;
                vec![RuntimeEvent::Error {
                    message: final_error.unwrap_or_else(|| "Pi retry failed".into()),
                    recoverable: false,
                }]
            }
            PiEvent::AgentEnd { will_retry: true } => Vec::new(),
            PiEvent::AgentEnd { will_retry: false } => {
                // R95: 原生终态。R96: agent_end(willRetry:false) 携带未决
                // 错误不再立即定性——Pi 0.85.1 的 context overflow 走
                // agent_end(false) → post-run 压缩 → 继续产出恢复答案，
                // 在这里升 fatal 会杀掉合法恢复。改为外发一条可恢复提示
                // （用户可见"未定性，等待结算"），未决错误保持待定；唯一
                // fatal 推迟到完整请求的 settlement（EOF，见 settle_at_eof）
                // 或 auto_retry_end(success:false)（既有路径）。正常终态
                // 不产生事件。
                if let Some(message) = self.pending_error.clone() {
                    vec![RuntimeEvent::Error {
                        message: format!(
                            "Pi ended the turn with an unresolved error, awaiting settlement: {message}"
                        ),
                        recoverable: true,
                    }]
                } else {
                    Vec::new()
                }
            }
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
                // R95: 暂时性 assistant 错误只记录待定状态，不立即定性为
                // fatal——随后的 agent_end(willRetry=true) + auto_retry
                // 会合法恢复；真正失败在 agent_end(willRetry=false) 定性。
                self.pending_error = Some(
                    error
                        .error_message
                        .unwrap_or_else(|| "Pi assistant request failed".into()),
                );
                Vec::new()
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
            // R95: 记为待定错误（usage 已在上方入账），是否 fatal 推迟到
            // agent_end / auto_retry_end 可判定时。
            self.pending_error = Some(
                message
                    .error_message
                    .unwrap_or_else(|| "Pi turn failed".into()),
            );
            return Vec::new();
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
        // R95: 一条成功的 assistant 消息意味着此前待定的错误已被重试恢复。
        self.pending_error = None;
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

    /// R95/R96: 单个 assistant 错误在 agent_end 之前不得外发 fatal——真实
    /// 时序里错误之后还有 willRetry 决定；overflow 场景 agent_end(false)
    /// 之后还有压缩恢复。因此 agent_end(false) 只外发可恢复提示，唯一
    /// fatal 由 settle_at_eof 在完整请求结算时定性（R96）。
    #[test]
    fn provisional_assistant_error_defers_settlement_to_eof() {
        let mut mapper = EventMapper::new();
        let _ = mapper.map(PiEvent::Session {
            id: "pi-1".into(),
            cwd: None,
        });
        assert!(
            mapper
                .map(PiEvent::MessageUpdate {
                    assistant_message_event: PiAssistantMessageEvent::Error {
                        error: PiMessage {
                            role: "assistant".into(),
                            stop_reason: Some("error".into()),
                            error_message: Some("503 Service Unavailable".into()),
                            ..PiMessage::default()
                        },
                    },
                })
                .is_empty(),
            "a provisional assistant error must stay buffered, not go fatal"
        );
        assert!(
            mapper
                .map(PiEvent::MessageEnd {
                    message: PiMessage {
                        role: "assistant".into(),
                        stop_reason: Some("error".into()),
                        error_message: Some("503 Service Unavailable".into()),
                        ..PiMessage::default()
                    },
                })
                .is_empty(),
            "message_end(error) must not settle the turn either"
        );
        let agent_end = mapper.map(PiEvent::AgentEnd { will_retry: false });
        assert!(
            !agent_end.iter().any(|event| matches!(
                event,
                RuntimeEvent::Error {
                    recoverable: false,
                    ..
                }
            )),
            "agent_end(false) must not settle a fatal while recovery may follow: {agent_end:#?}"
        );
        assert!(
            agent_end.iter().any(|event| matches!(
                event,
                RuntimeEvent::Error {
                    recoverable: true,
                    ..
                }
            )),
            "the unresolved state stays visible as a recoverable notice: {agent_end:#?}"
        );
        assert!(!mapper.failed());
        let settled = mapper.settle_at_eof();
        assert!(
            matches!(
                &settled,
                Some(RuntimeEvent::Error {
                    message,
                    recoverable: false,
                    ..
                }) if message == "503 Service Unavailable"
            ),
            "EOF settlement promotes the pending error to the single fatal: {settled:#?}"
        );
        assert!(mapper.failed());
        assert!(
            mapper.settle_at_eof().is_none(),
            "settlement is one-shot"
        );
    }

    /// R95: error → agent_end(true) → 重试成功 → agent_end(false) 全程无
    /// fatal，成功答案恢复且待定错误被清除。
    #[test]
    fn retried_error_recovers_without_any_fatal() {
        let mut mapper = EventMapper::new();
        let _ = mapper.map(PiEvent::Session {
            id: "pi-1".into(),
            cwd: None,
        });
        let _ = mapper.map(PiEvent::MessageUpdate {
            assistant_message_event: PiAssistantMessageEvent::Error {
                error: PiMessage {
                    role: "assistant".into(),
                    stop_reason: Some("error".into()),
                    error_message: Some("503 Service Unavailable".into()),
                    ..PiMessage::default()
                },
            },
        });
        let _ = mapper.map(PiEvent::MessageEnd {
            message: PiMessage {
                role: "assistant".into(),
                stop_reason: Some("error".into()),
                error_message: Some("503 Service Unavailable".into()),
                ..PiMessage::default()
            },
        });
        assert!(mapper
            .map(PiEvent::AgentEnd { will_retry: true })
            .is_empty());
        let retried = mapper.map(PiEvent::MessageEnd {
            message: PiMessage {
                role: "assistant".into(),
                stop_reason: Some("stop".into()),
                content: vec![PiContent::Text {
                    text: "recovered answer".into(),
                }],
                ..PiMessage::default()
            },
        });
        assert!(matches!(&retried[0], RuntimeEvent::Message { .. }));
        assert!(mapper.map(PiEvent::AgentEnd { will_retry: false }).is_empty());
        assert!(!mapper.failed());
        assert_eq!(mapper.last_agent_text(), Some("recovered answer"));
    }
}
