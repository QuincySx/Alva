// INPUT:  serde, serde_json, uuid, chrono, crate::content::ContentBlock
// OUTPUT: pub enum MessageRole, pub struct UsageMetadata, pub struct Message, pub enum AgentMessage
// POS:    Core message types representing LLM conversation turns, token usage, and an agent-level message wrapper.
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::content::ContentBlock;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    System,
    Tool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsageMetadata {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
    /// Tokens consumed creating a new prompt cache entry on this call.
    /// Populated by providers that report cache metrics (Anthropic);
    /// `None` for providers that don't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_creation_input_tokens: Option<u32>,
    /// Tokens read from a prompt cache hit on this call. Same population
    /// rules as `cache_creation_input_tokens`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_read_input_tokens: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: String,
    pub role: MessageRole,
    pub content: Vec<ContentBlock>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_call_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub usage: Option<UsageMetadata>,
    pub timestamp: i64,
}

impl Message {
    pub fn user(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::User,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn system(text: impl Into<String>) -> Self {
        Self {
            id: uuid::Uuid::new_v4().to_string(),
            role: MessageRole::System,
            content: vec![ContentBlock::Text { text: text.into() }],
            tool_call_id: None,
            usage: None,
            timestamp: chrono::Utc::now().timestamp_millis(),
        }
    }

    pub fn has_tool_calls(&self) -> bool {
        self.content.iter().any(|b| b.is_tool_use())
    }

    pub fn text_content(&self) -> String {
        self.content
            .iter()
            .filter_map(|b| b.as_text())
            .collect::<Vec<_>>()
            .join("")
    }
}

// ---------------------------------------------------------------------------
// AgentMessage
// ---------------------------------------------------------------------------

/// Wraps either a standard LLM message or application-level messages
/// that flow through the agent event stream.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum AgentMessage {
    /// Standard LLM message (user, assistant, system, tool).
    Standard(Message),
    /// User mid-turn intervention — injected after current tool execution completes.
    Steering(Message),
    /// System/middleware follow-up — appended when agent would otherwise stop.
    FollowUp(Message),
    /// State marker (checkpoint, phase change) — never sent to LLM.
    Marker(Marker),
    /// Generic extension point for application-specific messages.
    Extension { type_name: String, data: Value },
}

/// Markers for state transitions and checkpoints.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "marker_type")]
pub enum Marker {
    CheckpointCreated { id: String },
    PhaseChange { from: String, to: String },
}

#[cfg(test)]
mod tests {
    //! Tests for message types:
    //!   * MessageRole serde rename_all=lowercase (LLM wire format)
    //!   * UsageMetadata Default + cache-fields skip-when-None
    //!   * Message ctors (user/system) + has_tool_calls + text_content
    //!
    //! Wire-format regressions here break ALL LLM providers silently;
    //! text_content powers UI displays that filter out reasoning /
    //! tool blocks.
    use super::*;
    use serde_json::{json, Value};

    // -- MessageRole serde -------------------------------------------------

    #[test]
    fn role_serializes_lowercase_for_all_variants() {
        // Pin: every provider's request format expects "user" /
        // "assistant" / "system" / "tool". Switching to PascalCase
        // would break every chat call silently.
        assert_eq!(
            serde_json::to_value(MessageRole::User).unwrap(),
            json!("user")
        );
        assert_eq!(
            serde_json::to_value(MessageRole::Assistant).unwrap(),
            json!("assistant")
        );
        assert_eq!(
            serde_json::to_value(MessageRole::System).unwrap(),
            json!("system")
        );
        assert_eq!(
            serde_json::to_value(MessageRole::Tool).unwrap(),
            json!("tool")
        );
    }

    #[test]
    fn role_deserializes_lowercase_for_all_variants() {
        let r: MessageRole = serde_json::from_value(json!("user")).unwrap();
        assert_eq!(r, MessageRole::User);
        let r: MessageRole = serde_json::from_value(json!("tool")).unwrap();
        assert_eq!(r, MessageRole::Tool);
    }

    // -- UsageMetadata -----------------------------------------------------

    #[test]
    fn usage_metadata_default_is_all_zero_no_cache() {
        let u = UsageMetadata::default();
        assert_eq!(u.input_tokens, 0);
        assert_eq!(u.output_tokens, 0);
        assert_eq!(u.total_tokens, 0);
        assert!(u.cache_creation_input_tokens.is_none());
        assert!(u.cache_read_input_tokens.is_none());
    }

    #[test]
    fn usage_metadata_omits_none_cache_fields_on_serialize() {
        // Pin: skip_serializing_if = "Option::is_none" — Anthropic-
        // specific cache fields must NOT appear in OpenAI requests
        // (which would reject unknown fields).
        let u = UsageMetadata::default();
        let v = serde_json::to_value(&u).unwrap();
        assert!(v.get("cache_creation_input_tokens").is_none());
        assert!(v.get("cache_read_input_tokens").is_none());
    }

    #[test]
    fn usage_metadata_includes_cache_fields_when_some() {
        let u = UsageMetadata {
            input_tokens: 10,
            output_tokens: 5,
            total_tokens: 15,
            cache_creation_input_tokens: Some(20),
            cache_read_input_tokens: Some(100),
        };
        let v = serde_json::to_value(&u).unwrap();
        assert_eq!(v.get("cache_creation_input_tokens"), Some(&json!(20)));
        assert_eq!(v.get("cache_read_input_tokens"), Some(&json!(100)));
    }

    // -- Message::user / Message::system ----------------------------------

    #[test]
    fn user_ctor_sets_role_and_text_content() {
        let m = Message::user("hello");
        assert_eq!(m.role, MessageRole::User);
        assert!(m.tool_call_id.is_none());
        assert!(m.usage.is_none());
        assert!(!m.id.is_empty(), "id must be populated (uuid)");
        // First content block is Text { "hello" }.
        assert_eq!(m.content.len(), 1);
        assert_eq!(m.text_content(), "hello");
    }

    #[test]
    fn system_ctor_sets_role_system() {
        let m = Message::system("you are helpful");
        assert_eq!(m.role, MessageRole::System);
        assert_eq!(m.text_content(), "you are helpful");
    }

    #[test]
    fn user_ctor_assigns_unique_ids() {
        // uuid::Uuid::new_v4 ⇒ each message gets its own id; pin
        // since something downstream might dedupe by id.
        let a = Message::user("a");
        let b = Message::user("b");
        assert_ne!(a.id, b.id);
    }

    // -- has_tool_calls / text_content ------------------------------------

    #[test]
    fn has_tool_calls_false_for_text_only_message() {
        let m = Message::user("hi");
        assert!(!m.has_tool_calls());
    }

    #[test]
    fn has_tool_calls_true_when_any_block_is_tool_use() {
        let mut m = Message::user("hi");
        m.content.push(ContentBlock::ToolUse {
            id: "id1".into(),
            name: "read_file".into(),
            input: Value::Null,
        });
        assert!(m.has_tool_calls());
    }

    #[test]
    fn text_content_concatenates_all_text_blocks() {
        let m = Message {
            id: "x".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "part-a ".into(),
                },
                ContentBlock::Text {
                    text: "part-b".into(),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        assert_eq!(m.text_content(), "part-a part-b");
    }

    #[test]
    fn text_content_ignores_non_text_blocks() {
        // Pin: tool_use / reasoning / image blocks must NOT bleed into
        // the user-visible text. UI export and copy-to-clipboard rely
        // on this.
        let m = Message {
            id: "x".into(),
            role: MessageRole::Assistant,
            content: vec![
                ContentBlock::Text {
                    text: "visible ".into(),
                },
                ContentBlock::ToolUse {
                    id: "t1".into(),
                    name: "tool".into(),
                    input: Value::Null,
                },
                ContentBlock::Reasoning {
                    text: "hidden thought".into(),
                    signature: None,
                },
                ContentBlock::Text {
                    text: "more".into(),
                },
            ],
            tool_call_id: None,
            usage: None,
            timestamp: 0,
        };
        assert_eq!(m.text_content(), "visible more");
        // Sanity: reasoning text really IS in the message, just not
        // exposed via text_content().
        assert!(m.has_tool_calls());
    }
}
