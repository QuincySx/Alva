// INPUT:  serde and serde_json values from Pi JSON mode
// OUTPUT: PiEvent plus minimal nested message/tool wire types
// POS:    Stable tolerant wire projection for `pi --print --mode json` JSONL.

use serde::Deserialize;
use serde_json::Value;

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PiEvent {
    #[serde(rename = "session")]
    Session {
        id: String,
        #[serde(default)]
        #[allow(dead_code)]
        cwd: Option<String>,
    },
    #[serde(rename = "message_start")]
    MessageStart { message: PiMessage },
    #[serde(rename = "message_update")]
    MessageUpdate {
        #[serde(rename = "assistantMessageEvent")]
        assistant_message_event: PiAssistantMessageEvent,
    },
    #[serde(rename = "message_end")]
    MessageEnd { message: PiMessage },
    #[serde(rename = "tool_execution_start")]
    ToolExecutionStart {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(default)]
        args: Value,
    },
    #[serde(rename = "tool_execution_end")]
    ToolExecutionEnd {
        #[serde(rename = "toolCallId")]
        tool_call_id: String,
        #[serde(rename = "toolName")]
        tool_name: String,
        #[serde(default)]
        result: Value,
        #[serde(rename = "isError", default)]
        is_error: bool,
    },
    #[serde(rename = "auto_retry_start")]
    AutoRetryStart {
        #[serde(rename = "errorMessage")]
        error_message: String,
        attempt: u32,
        #[serde(rename = "maxAttempts")]
        max_attempts: u32,
    },
    #[serde(rename = "auto_retry_end")]
    AutoRetryEnd {
        success: bool,
        #[serde(rename = "finalError", default)]
        final_error: Option<String>,
    },
    #[serde(rename = "agent_end")]
    AgentEnd {
        #[serde(rename = "willRetry", default)]
        will_retry: bool,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PiMessage {
    #[serde(default)]
    pub role: String,
    #[serde(default)]
    pub content: Vec<PiContent>,
    #[serde(default)]
    #[allow(dead_code)]
    pub model: Option<String>,
    #[serde(rename = "stopReason", default)]
    pub stop_reason: Option<String>,
    #[serde(rename = "errorMessage", default)]
    pub error_message: Option<String>,
    #[serde(default)]
    pub usage: Option<PiUsage>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PiContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "thinking")]
    Thinking {
        #[allow(dead_code)]
        thinking: String,
    },
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PiUsage {
    #[serde(default)]
    pub input: u32,
    #[serde(default)]
    pub output: u32,
    #[serde(rename = "totalTokens", default)]
    #[allow(dead_code)]
    pub total_tokens: u32,
    #[serde(default)]
    pub cost: Option<PiCost>,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct PiCost {
    #[serde(default)]
    pub total: f64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum PiAssistantMessageEvent {
    #[serde(rename = "text_delta")]
    TextDelta { delta: String },
    #[serde(rename = "thinking_delta")]
    ThinkingDelta { delta: String },
    #[serde(rename = "error")]
    Error { error: PiMessage },
    #[serde(other)]
    Unknown,
}
