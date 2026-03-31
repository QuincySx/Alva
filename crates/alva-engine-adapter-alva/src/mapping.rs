// INPUT:  alva_agent_core::AgentEvent, alva_types (AgentMessage, Message, ContentBlock, MessageRole, StreamEvent, ToolCall, ToolResult)
// OUTPUT: Vec<alva_engine_runtime::RuntimeEvent>
// POS:    Stateful mapper that converts AgentEvent stream into RuntimeEvent stream for the EngineRuntime interface.
use std::collections::HashMap;

use alva_agent_core::AgentEvent;
use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_types::{AgentMessage, ContentBlock};

/// Stateful mapper — converts `AgentEvent` to `Vec<RuntimeEvent>`.
///
/// Maintains:
/// - `session_id`  — set on `AgentStart`, propagated to `Completed`
/// - `turn_count`  — incremented on `TurnStart`, used for `RuntimeUsage::num_turns`
/// - `tool_names`  — `tool_use_id → name` lookup so `ToolEnd` can resolve the name
pub(crate) struct EventMapper {
    session_id: String,
    turn_count: u32,
    tool_names: HashMap<String, String>,
}

impl EventMapper {
    pub fn new(session_id: String) -> Self {
        Self {
            session_id,
            turn_count: 0,
            tool_names: HashMap::new(),
        }
    }

    /// Current session ID (set at construction, used by adapter for routing).
    #[allow(dead_code)]
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Map a single `AgentEvent` to zero or more `RuntimeEvent`s.
    pub fn map(&mut self, event: AgentEvent) -> Vec<RuntimeEvent> {
        match event {
            // ── Lifecycle ──────────────────────────────────────────────────
            AgentEvent::AgentStart => {
                vec![RuntimeEvent::SessionStarted {
                    session_id: self.session_id.clone(),
                    model: None,
                    tools: vec![],
                }]
            }

            AgentEvent::TurnStart => {
                self.turn_count += 1;
                vec![]
            }

            AgentEvent::TurnEnd => vec![],

            // ── Messages ───────────────────────────────────────────────────
            AgentEvent::MessageStart { .. } => vec![],

            AgentEvent::MessageUpdate { message, delta } => {
                let id = message_id(&message);
                vec![RuntimeEvent::MessageDelta { id, delta }]
            }

            AgentEvent::MessageEnd { message } => {
                self.map_message_end(message)
            }

            // ── Tool execution ─────────────────────────────────────────────
            AgentEvent::ToolExecutionStart { .. } => vec![],

            AgentEvent::ToolExecutionUpdate { .. } => vec![],

            AgentEvent::ToolExecutionEnd { tool_call, result } => {
                let name = self
                    .tool_names
                    .get(&tool_call.id)
                    .cloned()
                    .unwrap_or_else(|| tool_call.name.clone());
                vec![RuntimeEvent::ToolEnd {
                    id: tool_call.id,
                    name,
                    result,
                    duration_ms: None,
                }]
            }

            // ── Agent end ──────────────────────────────────────────────────
            AgentEvent::AgentEnd { error: None } => {
                vec![RuntimeEvent::Completed {
                    session_id: self.session_id.clone(),
                    result: Some("completed".to_string()),
                    usage: Some(RuntimeUsage {
                        num_turns: self.turn_count,
                        ..Default::default()
                    }),
                }]
            }

            AgentEvent::AgentEnd { error: Some(msg) } => {
                vec![
                    RuntimeEvent::Error {
                        message: msg,
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
                ]
            }
        }
    }

    // ── Private helpers ────────────────────────────────────────────────────

    fn map_message_end(&mut self, message: AgentMessage) -> Vec<RuntimeEvent> {
        let msg = match &message {
            AgentMessage::Standard(m) => m,
            AgentMessage::Steering(_)
            | AgentMessage::FollowUp(_)
            | AgentMessage::Marker(_)
            | AgentMessage::Extension { .. } => return vec![],
        };

        let msg_id = msg.id.clone();
        let role = msg.role.clone();
        let mut events: Vec<RuntimeEvent> = Vec::new();

        // Separate non-tool-use blocks from tool-use blocks.
        let mut content_blocks: Vec<ContentBlock> = Vec::new();

        for block in &msg.content {
            if let Some((id, name, input)) = block.as_tool_use() {
                // Register id→name for later ToolEnd lookup.
                self.tool_names.insert(id.to_owned(), name.to_owned());
                events.push(RuntimeEvent::ToolStart {
                    id: id.to_owned(),
                    name: name.to_owned(),
                    input: input.clone(),
                });
            } else {
                content_blocks.push(block.clone());
            }
        }

        // Emit Message event only when there are non-tool-use blocks.
        if !content_blocks.is_empty() {
            // Insert the Message event before all ToolStart events.
            events.insert(
                0,
                RuntimeEvent::Message {
                    id: msg_id,
                    role,
                    content: content_blocks,
                },
            );
        }

        events
    }
}

/// Extract the message ID from an `AgentMessage`.
fn message_id(message: &AgentMessage) -> String {
    match message {
        AgentMessage::Standard(m) => m.id.clone(),
        AgentMessage::Steering(m) | AgentMessage::FollowUp(m) => m.id.clone(),
        AgentMessage::Marker(_) => "marker".to_string(),
        AgentMessage::Extension { type_name, .. } => type_name.clone(),
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::{Message, MessageRole, StreamEvent, ToolCall, ToolOutput};

    fn make_session_id() -> String {
        "test-session".to_string()
    }

    fn assistant_message(id: &str, content: Vec<ContentBlock>) -> Message {
        Message {
            id: id.to_string(),
            role: MessageRole::Assistant,
            content,
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        }
    }

    // ── Test 1: AgentStart → SessionStarted ───────────────────────────────

    #[test]
    fn test_agent_start_emits_session_started() {
        let mut mapper = EventMapper::new(make_session_id());
        let events = mapper.map(AgentEvent::AgentStart);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], RuntimeEvent::SessionStarted { session_id, model: None, tools }
                if session_id == "test-session" && tools.is_empty())
        );
    }

    // ── Test 2: TurnStart increments counter, produces no events ──────────

    #[test]
    fn test_turn_start_increments_counter_no_events() {
        let mut mapper = EventMapper::new(make_session_id());
        let events1 = mapper.map(AgentEvent::TurnStart);
        let events2 = mapper.map(AgentEvent::TurnStart);
        assert!(events1.is_empty());
        assert!(events2.is_empty());
        assert_eq!(mapper.turn_count, 2);
    }

    // ── Test 3: MessageUpdate → MessageDelta ──────────────────────────────

    #[test]
    fn test_message_update_emits_message_delta() {
        let mut mapper = EventMapper::new(make_session_id());
        let msg = AgentMessage::Standard(assistant_message("msg1", vec![]));
        let delta = StreamEvent::TextDelta { text: "hello".to_string() };
        let events = mapper.map(AgentEvent::MessageUpdate {
            message: msg,
            delta: delta.clone(),
        });
        assert_eq!(events.len(), 1);
        assert!(matches!(
            &events[0],
            RuntimeEvent::MessageDelta { id, .. } if id == "msg1"
        ));
    }

    // ── Test 4: MessageEnd splits ToolUse blocks ──────────────────────────

    #[test]
    fn test_message_end_splits_tool_use_blocks() {
        let mut mapper = EventMapper::new(make_session_id());
        let content = vec![
            ContentBlock::Text { text: "I'll read the file.".to_string() },
            ContentBlock::ToolUse {
                id: "tu1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/a.rs"}),
            },
        ];
        let msg = AgentMessage::Standard(assistant_message("msg2", content));
        let events = mapper.map(AgentEvent::MessageEnd { message: msg });

        // Expect: Message (text only) + ToolStart
        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], RuntimeEvent::Message { id, content, .. }
                if id == "msg2" && content.len() == 1 && content[0].is_text())
        );
        assert!(
            matches!(&events[1], RuntimeEvent::ToolStart { id, name, .. }
                if id == "tu1" && name == "Read")
        );

        // Also verify the tool name is registered for future ToolEnd lookup.
        assert_eq!(mapper.tool_names.get("tu1").map(|s| s.as_str()), Some("Read"));
    }

    // ── Test 5: MessageEnd with only ToolUse — no Message event ──────────

    #[test]
    fn test_message_end_only_tool_use_no_message_event() {
        let mut mapper = EventMapper::new(make_session_id());
        let content = vec![ContentBlock::ToolUse {
            id: "tu2".to_string(),
            name: "Bash".to_string(),
            input: serde_json::json!({"cmd": "ls"}),
        }];
        let msg = AgentMessage::Standard(assistant_message("msg3", content));
        let events = mapper.map(AgentEvent::MessageEnd { message: msg });

        assert_eq!(events.len(), 1);
        assert!(matches!(&events[0], RuntimeEvent::ToolStart { name, .. } if name == "Bash"));
    }

    // ── Test 6: ToolExecutionEnd resolves name from registry ─────────────

    #[test]
    fn test_tool_execution_end_resolves_name() {
        let mut mapper = EventMapper::new(make_session_id());
        mapper.tool_names.insert("tu3".to_string(), "Grep".to_string());

        let tool_call = ToolCall {
            id: "tu3".to_string(),
            name: "Grep".to_string(),
            arguments: serde_json::json!({}),
        };
        let result = ToolOutput::text("found 3 matches");
        let events = mapper.map(AgentEvent::ToolExecutionEnd { tool_call, result });

        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], RuntimeEvent::ToolEnd { id, name, duration_ms: None, .. }
                if id == "tu3" && name == "Grep")
        );
    }

    // ── Test 7a: AgentEnd (no error) → Completed with usage ──────────────

    #[test]
    fn test_agent_end_success_emits_completed() {
        let mut mapper = EventMapper::new(make_session_id());
        mapper.turn_count = 3;
        let events = mapper.map(AgentEvent::AgentEnd { error: None });

        assert_eq!(events.len(), 1);
        assert!(
            matches!(&events[0], RuntimeEvent::Completed {
                session_id,
                result: Some(r),
                usage: Some(u),
            } if session_id == "test-session" && r == "completed" && u.num_turns == 3)
        );
    }

    // ── Test 7b: AgentEnd (with error) → Error + Completed ───────────────

    #[test]
    fn test_agent_end_error_emits_error_then_completed() {
        let mut mapper = EventMapper::new(make_session_id());
        mapper.turn_count = 2;
        let events = mapper.map(AgentEvent::AgentEnd {
            error: Some("max turns exceeded".to_string()),
        });

        assert_eq!(events.len(), 2);
        assert!(
            matches!(&events[0], RuntimeEvent::Error { message, recoverable: false }
                if message == "max turns exceeded")
        );
        assert!(
            matches!(&events[1], RuntimeEvent::Completed {
                result: None,
                usage: Some(u),
                ..
            } if u.num_turns == 2)
        );
    }
}
