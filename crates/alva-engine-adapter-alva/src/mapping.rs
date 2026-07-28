// INPUT:  alva_kernel_core::AgentEvent, alva_kernel_abi::{AgentMessage, ContentBlock}, alva_engine_runtime::{RuntimeEvent, RuntimeUsage}
// OUTPUT: EventMapper
// POS:    Projects complete BaseAgent loop events onto the engine contract.

use std::collections::HashMap;

use alva_engine_runtime::{RuntimeEvent, RuntimeUsage};
use alva_kernel_abi::{AgentMessage, ContentBlock};
use alva_kernel_core::AgentEvent;

pub(crate) struct EventMapper {
    session_id: String,
    model: String,
    tools: Vec<String>,
    turn_count: u32,
    tool_names: HashMap<String, String>,
}

impl EventMapper {
    pub(crate) fn new(session_id: String, model: String, tools: Vec<String>) -> Self {
        Self {
            session_id,
            model,
            tools,
            turn_count: 0,
            tool_names: HashMap::new(),
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
            AgentEvent::MessageUpdate { message, delta } => vec![RuntimeEvent::MessageDelta {
                id: message_id(&message),
                delta,
            }],
            AgentEvent::MessageError { error, .. } => vec![RuntimeEvent::Error {
                message: error,
                recoverable: true,
            }],
            AgentEvent::MessageEnd { message } => self.map_message(message),
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
                result: Some("completed".into()),
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

    fn map_message(&mut self, message: AgentMessage) -> Vec<RuntimeEvent> {
        let AgentMessage::Standard(message) = message else {
            return vec![];
        };
        let mut content = Vec::new();
        let mut tool_events = Vec::new();
        for block in message.content {
            match block {
                ContentBlock::ToolUse { id, name, input } => {
                    self.tool_names.insert(id.clone(), name.clone());
                    tool_events.push(RuntimeEvent::ToolStart { id, name, input });
                }
                block => content.push(block),
            }
        }
        if !content.is_empty() {
            tool_events.insert(
                0,
                RuntimeEvent::Message {
                    id: message.id,
                    role: message.role,
                    content,
                },
            );
        }
        tool_events
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
