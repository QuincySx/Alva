// INPUT:  alva_kernel_abi (StreamEvent, ToolCall, ToolOutput, ProgressEvent, AgentMessage)
// OUTPUT: AgentEvent
// POS:    Defines the event enum emitted by the agent loop for callers to observe progress, messages, and tool execution.
use serde::Serialize;

use alva_kernel_abi::AgentMessage;
use alva_kernel_abi::ProgressEvent;
use alva_kernel_abi::{StreamEvent, ToolCall, ToolOutput};

/// Events emitted by the agent loop so callers can observe progress.
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type")]
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
    /// The assistant message failed before completion.
    MessageError {
        message: AgentMessage,
        error: String,
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
