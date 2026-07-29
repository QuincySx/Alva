// INPUT:  serde, serde_json::Value
// OUTPUT: CodexEvent and nested item types for `codex exec --json` JSONL
// POS:    Wire types for Codex non-interactive JSONL event stream.

use serde::Deserialize;
use serde_json::Value;

/// One line from `codex exec --json` stdout.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum CodexEvent {
    #[serde(rename = "thread.started")]
    ThreadStarted {
        #[serde(default)]
        thread_id: Option<String>,
    },

    #[serde(rename = "turn.started")]
    TurnStarted,

    #[serde(rename = "turn.completed")]
    TurnCompleted {
        #[serde(default)]
        usage: Option<CodexUsage>,
    },

    #[serde(rename = "turn.failed")]
    TurnFailed {
        #[serde(default)]
        error: Option<CodexErrorBody>,
    },

    #[serde(rename = "item.started")]
    ItemStarted { item: CodexItem },

    #[serde(rename = "item.updated")]
    ItemUpdated { item: CodexItem },

    #[serde(rename = "item.completed")]
    ItemCompleted { item: CodexItem },

    #[serde(rename = "error")]
    Error {
        #[serde(default)]
        message: Option<String>,
        #[serde(default)]
        error: Option<CodexErrorBody>,
    },

    #[serde(other)]
    Unknown,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexUsage {
    #[serde(default)]
    pub input_tokens: Option<u32>,
    #[serde(default)]
    pub output_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    pub cached_input_tokens: Option<u32>,
    #[serde(default)]
    #[allow(dead_code)]
    pub reasoning_output_tokens: Option<u32>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct CodexErrorBody {
    #[serde(default)]
    pub message: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type")]
pub enum CodexItem {
    #[serde(rename = "agent_message")]
    AgentMessage {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },

    #[serde(rename = "reasoning")]
    Reasoning {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        text: Option<String>,
    },

    #[serde(rename = "command_execution")]
    CommandExecution {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        command: Option<String>,
        #[serde(default)]
        aggregated_output: Option<String>,
        #[serde(default)]
        exit_code: Option<i32>,
        #[serde(default)]
        #[allow(dead_code)]
        status: Option<String>,
    },

    #[serde(rename = "file_change")]
    FileChange {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        changes: Option<Value>,
        #[serde(default)]
        #[allow(dead_code)]
        status: Option<String>,
    },

    #[serde(rename = "mcp_tool_call")]
    McpToolCall {
        #[serde(default)]
        id: Option<String>,
        #[serde(default)]
        name: Option<String>,
        #[serde(default)]
        arguments: Option<Value>,
        #[serde(default)]
        result: Option<Value>,
        #[serde(default)]
        #[allow(dead_code)]
        status: Option<String>,
    },

    #[serde(other)]
    Unknown,
}
