#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub session_id: String,
    pub role: MessageRole,
    pub content: MessageContent,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum MessageRole {
    User,
    Assistant,
    System,
}

#[derive(Debug, Clone)]
pub enum MessageContent {
    Text { text: String },
    Thinking { text: String },
    ToolCallStart { tool_name: String, call_id: String },
    ToolCallEnd { call_id: String, output: String, is_error: bool },
}
