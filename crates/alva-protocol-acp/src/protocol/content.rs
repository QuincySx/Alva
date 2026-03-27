// INPUT:  serde, serde_json::Value
// OUTPUT: pub enum ContentBlock
// POS:    Defines the ACP content block enum with Text (delta support), ToolUse, and ToolResult variants.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ContentBlock {
    /// Text output (streaming delta concatenation)
    Text {
        text: String,
        /// true = incremental delta; false = complete content
        #[serde(default)]
        is_delta: bool,
    },
    /// Tool call request
    ToolUse {
        id: String,
        name: String,
        /// Complete tool parameters (JSON)
        input: Value,
    },
    /// Tool execution result (returned by external Agent's tool execution)
    ToolResult {
        tool_use_id: String,
        content: String,
        #[serde(default)]
        is_error: bool,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_text_block_serde() {
        let block = ContentBlock::Text {
            text: "hello".to_string(),
            is_delta: true,
        };
        let json = serde_json::to_string(&block).unwrap();
        assert!(json.contains(r#""type":"text""#));
        assert!(json.contains(r#""is_delta":true"#));

        let deserialized: ContentBlock = serde_json::from_str(&json).unwrap();
        if let ContentBlock::Text { text, is_delta } = deserialized {
            assert_eq!(text, "hello");
            assert!(is_delta);
        } else {
            panic!("expected Text block");
        }
    }

    #[test]
    fn test_tool_use_block_serde() {
        let json = r#"{"type":"tool_use","id":"t1","name":"read_file","input":{"path":"/tmp/x"}}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        if let ContentBlock::ToolUse { id, name, input } = block {
            assert_eq!(id, "t1");
            assert_eq!(name, "read_file");
            assert_eq!(input["path"], "/tmp/x");
        } else {
            panic!("expected ToolUse block");
        }
    }

    #[test]
    fn test_tool_result_block_serde() {
        let json = r#"{"type":"tool_result","tool_use_id":"t1","content":"ok","is_error":false}"#;
        let block: ContentBlock = serde_json::from_str(json).unwrap();
        if let ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } = block
        {
            assert_eq!(tool_use_id, "t1");
            assert_eq!(content, "ok");
            assert!(!is_error);
        } else {
            panic!("expected ToolResult block");
        }
    }
}
