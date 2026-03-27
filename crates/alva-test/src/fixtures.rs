// INPUT:  alva_types::{ContentBlock, Message, MessageRole}, serde_json::Value, uuid
// OUTPUT: pub fn make_user_message, pub fn make_assistant_message, pub fn make_tool_call_message
// POS:    Provides factory functions for constructing test Message fixtures with various roles and content blocks.
use alva_types::{ContentBlock, Message, MessageRole};
use serde_json::Value;

pub fn make_user_message(text: &str) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::User,
        content: vec![ContentBlock::Text { text: text.into() }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

pub fn make_assistant_message(text: &str) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::Text { text: text.into() }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

pub fn make_tool_call_message(tool_name: &str, args: Value) -> Message {
    Message {
        id: uuid_str(),
        role: MessageRole::Assistant,
        content: vec![ContentBlock::ToolUse {
            id: uuid_str(),
            name: tool_name.into(),
            input: args,
        }],
        tool_call_id: None,
        usage: None,
        timestamp: 0,
    }
}

fn uuid_str() -> String {
    uuid::Uuid::new_v4().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use alva_types::MessageRole;

    #[test]
    fn test_make_user_message() {
        let msg = make_user_message("hello");
        assert_eq!(msg.role, MessageRole::User);
        assert_eq!(msg.content.len(), 1);
    }

    #[test]
    fn test_make_assistant_message() {
        let msg = make_assistant_message("hi there");
        assert_eq!(msg.role, MessageRole::Assistant);
    }

    #[test]
    fn test_make_tool_call_message() {
        let msg = make_tool_call_message("read_file", serde_json::json!({"path": "/tmp"}));
        assert_eq!(msg.role, MessageRole::Assistant);
        assert!(!msg.content.is_empty());
    }
}
