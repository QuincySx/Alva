//! MCP Prompt types.
//!
//! Prompts are reusable prompt templates that MCP servers expose to clients.
//! They can include arguments for parameterization and may reference
//! resources in their content.

use serde::{Deserialize, Serialize};

use crate::resources::McpResourceContent;

/// An MCP prompt template.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPrompt {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<McpPromptArgument>,
}

/// An argument for an MCP prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptArgument {
    pub name: String,
    pub description: Option<String>,
    pub required: bool,
}

/// Result of getting (rendering) a prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptResult {
    pub description: Option<String>,
    pub messages: Vec<McpPromptMessage>,
}

/// A message within a rendered prompt result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpPromptMessage {
    /// "user" or "assistant"
    pub role: String,
    pub content: McpPromptContent,
}

/// Content within a prompt message -- either plain text or an embedded resource.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type")]
pub enum McpPromptContent {
    #[serde(rename = "text")]
    Text { text: String },
    #[serde(rename = "resource")]
    Resource { resource: McpResourceContent },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_serde_roundtrip() {
        let prompt = McpPrompt {
            name: "code-review".into(),
            description: Some("Review code for issues".into()),
            arguments: vec![
                McpPromptArgument {
                    name: "language".into(),
                    description: Some("Programming language".into()),
                    required: true,
                },
                McpPromptArgument {
                    name: "style".into(),
                    description: None,
                    required: false,
                },
            ],
        };

        let json = serde_json::to_string(&prompt).unwrap();
        let parsed: McpPrompt = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.name, "code-review");
        assert_eq!(parsed.arguments.len(), 2);
        assert!(parsed.arguments[0].required);
        assert!(!parsed.arguments[1].required);
    }

    #[test]
    fn prompt_with_no_arguments() {
        let prompt = McpPrompt {
            name: "summarize".into(),
            description: None,
            arguments: vec![],
        };

        let json = serde_json::to_string(&prompt).unwrap();
        let parsed: McpPrompt = serde_json::from_str(&json).unwrap();
        assert!(parsed.arguments.is_empty());
        assert!(parsed.description.is_none());
    }

    #[test]
    fn prompt_content_text_serde() {
        let content = McpPromptContent::Text {
            text: "Please review this code.".into(),
        };

        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"text\""));
        let parsed: McpPromptContent = serde_json::from_str(&json).unwrap();
        match parsed {
            McpPromptContent::Text { text } => assert_eq!(text, "Please review this code."),
            _ => panic!("expected Text variant"),
        }
    }

    #[test]
    fn prompt_content_resource_serde() {
        let content = McpPromptContent::Resource {
            resource: McpResourceContent {
                uri: "file:///src/main.rs".into(),
                mime_type: Some("text/x-rust".into()),
                text: Some("fn main() {}".into()),
                blob: None,
            },
        };

        let json = serde_json::to_string(&content).unwrap();
        assert!(json.contains("\"type\":\"resource\""));
        let parsed: McpPromptContent = serde_json::from_str(&json).unwrap();
        match parsed {
            McpPromptContent::Resource { resource } => {
                assert_eq!(resource.uri, "file:///src/main.rs");
                assert_eq!(resource.text.as_deref(), Some("fn main() {}"));
            }
            _ => panic!("expected Resource variant"),
        }
    }

    #[test]
    fn prompt_result_serde_roundtrip() {
        let result = McpPromptResult {
            description: Some("A code review prompt".into()),
            messages: vec![
                McpPromptMessage {
                    role: "user".into(),
                    content: McpPromptContent::Text {
                        text: "Review this file".into(),
                    },
                },
                McpPromptMessage {
                    role: "assistant".into(),
                    content: McpPromptContent::Text {
                        text: "I'll review the code now.".into(),
                    },
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        let parsed: McpPromptResult = serde_json::from_str(&json).unwrap();

        assert_eq!(parsed.messages.len(), 2);
        assert_eq!(parsed.messages[0].role, "user");
        assert_eq!(parsed.messages[1].role, "assistant");
    }
}
