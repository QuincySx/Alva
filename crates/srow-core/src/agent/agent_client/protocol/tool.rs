// INPUT:  serde, serde_json
// OUTPUT: PreToolUseData, PostToolUseData, ToolCallData
// POS:    ACP tool execution notification types: pre-use, post-use, and complete call data.
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Pre-tool-use notification (Srow can decide to intercept after receiving this)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreToolUseData {
    pub tool_call_id: String,
    pub tool_name: String,
    /// Complete tool input parameters
    pub input: Value,
}

/// Post-tool-use notification (carries execution result)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PostToolUseData {
    pub tool_call_id: String,
    pub tool_name: String,
    pub output: String,
    #[serde(default)]
    pub is_error: bool,
    /// Execution duration (milliseconds)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
}

/// Complete tool call data (input + output, for persistence)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallData {
    pub tool_call_id: String,
    pub tool_name: String,
    pub input: Value,
    pub output: String,
    #[serde(default)]
    pub is_error: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pre_tool_use_serde() {
        let data = PreToolUseData {
            tool_call_id: "tc-1".to_string(),
            tool_name: "read_file".to_string(),
            input: serde_json::json!({"path": "/tmp/test.rs"}),
        };
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: PreToolUseData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.tool_name, "read_file");
    }

    #[test]
    fn test_post_tool_use_serde() {
        let data = PostToolUseData {
            tool_call_id: "tc-1".to_string(),
            tool_name: "read_file".to_string(),
            output: "file contents".to_string(),
            is_error: false,
            duration_ms: Some(42),
        };
        let json = serde_json::to_string(&data).unwrap();
        let deserialized: PostToolUseData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.duration_ms, Some(42));
        assert!(!deserialized.is_error);
    }
}
