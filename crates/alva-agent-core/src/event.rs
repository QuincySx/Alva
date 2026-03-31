// INPUT:  alva_types (StreamEvent, ToolCall, ToolOutput, ProgressEvent, AgentMessage)
// OUTPUT: AgentEvent
// POS:    Defines the event enum emitted by the agent loop for callers to observe progress, messages, and tool execution.
use alva_types::{StreamEvent, ToolCall, ToolOutput};
use alva_types::ProgressEvent;
use alva_types::AgentMessage;

/// Events emitted by the agent loop so callers can observe progress.
#[derive(Debug, Clone)]
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
